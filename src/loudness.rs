//! Integrated loudness (EBU R128 / ITU-R BS.1770) of a whole track, measured while it's
//! decoded for its beat map, so every played track can be leveled even without
//! ReplayGain tags.
//!
//! The measurement runs on the mono mix the analysis already has. For the usual
//! stereo track (channels mostly alike) that reads 3 dB under the stereo measure, so
//! 3.01 dB is added back (not for a mono file, which BS.1770 measures as one channel); tracks are only ever compared with each other, so the small
//! error on very wide stereo doesn't matter.

use serde::{Deserialize, Serialize};

/// Gating block and step (BS.1770: 400 ms blocks overlapping by 75%).
const BLOCK: f64 = 0.4;
const STEP: f64 = 0.1;
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loudness {
    /// Integrated loudness, LUFS.
    pub lufs: f32,
    /// Sample peak of any channel, dBFS.
    pub peak_db: f32,
}

impl Loudness {
    /// The gain (dB) that brings this track to `target` LUFS without clipping: never
    /// past 1 dB under full scale, and within ±15 dB.
    pub fn gain_to(&self, target: f32) -> f32 {
        (target - self.lufs)
            .min(-1.0 - self.peak_db)
            .clamp(-15.0, 15.0)
    }
}

/// A biquad with explicit coefficients (normalized by a0).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 3]) -> Self {
        Biquad {
            b: b.map(|x| x / a[0]),
            a: [a[1] / a[0], a[2] / a[0]],
            z: [0.0; 2],
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The K-weighting filter (a high shelf, then a high pass), at any sample rate. These
/// are De Man's analog-matched designs (as in libebur128); at 48 kHz they give
/// BS.1770's published coefficients exactly.
fn k_weighting(sr: f64) -> [Biquad; 2] {
    use std::f64::consts::PI;
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / sr).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let shelf = Biquad::new(
        [
            vh + vb * k / q + k * k,
            2.0 * (k * k - vh),
            vh - vb * k / q + k * k,
        ],
        [
            1.0 + k / q + k * k,
            2.0 * (k * k - 1.0),
            1.0 - k / q + k * k,
        ],
    );
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / sr).tan();
    let high_pass = Biquad::new(
        [1.0, -2.0, 1.0],
        [
            1.0 + k / q + k * k,
            2.0 * (k * k - 1.0),
            1.0 - k / q + k * k,
        ],
    );
    [shelf, high_pass]
}

#[cfg(test)]
#[test]
fn k_weighting_matches_bs1770_at_48k() {
    let [shelf, hp] = k_weighting(48_000.0);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(close(shelf.b[0], 1.53512485958697) && close(shelf.a[0], -1.69065929318241));
    assert!(close(hp.a[0], -1.99004745483398) && close(hp.a[1], 0.99007225036621));
}

/// Streaming loudness meter: `push` interleaved samples, then `finish`.
pub struct Meter {
    sample_rate: f64,
    /// One K-weighting chain per channel.
    filters: Vec<[Biquad; 2]>,
    step_len: usize,
    acc: f64,
    n: usize,
    /// Channel-summed mean square of each 100 ms step.
    steps: Vec<f64>,
    peak: f32,
}

impl Meter {
    pub fn new(sample_rate: f32) -> Self {
        Meter {
            sample_rate: sample_rate as f64,
            filters: Vec::new(),
            step_len: ((sample_rate as f64 * STEP) as usize).max(1),
            acc: 0.0,
            n: 0,
            steps: Vec::new(),
            peak: 0.0,
        }
    }

    /// Interleaved frames of `channels` channels. Every channel counts fully (BS.1770
    /// weights the surround pair a little more; music libraries are stereo).
    pub fn push(&mut self, interleaved: &[f32], channels: usize) {
        let channels = channels.max(1);
        if self.filters.len() != channels {
            self.filters = vec![k_weighting(self.sample_rate); channels];
        }
        for frame in interleaved.chunks_exact(channels) {
            for (&x, chain) in frame.iter().zip(&mut self.filters) {
                self.peak = self.peak.max(x.abs());
                let y = chain.iter_mut().fold(x as f64, |v, f| f.process(v));
                self.acc += y * y;
            }
            self.n += 1;
            if self.n == self.step_len {
                self.steps.push(self.acc / self.n as f64);
                self.acc = 0.0;
                self.n = 0;
            }
        }
    }

    /// None if the track is too short or silent.
    pub fn finish(self) -> Option<Loudness> {
        let per_block = (BLOCK / STEP).round() as usize;
        let blocks: Vec<f64> = self
            .steps
            .windows(per_block)
            .map(|w| w.iter().sum::<f64>() / per_block as f64)
            .collect();
        let lufs = |power: f64| -0.691 + 10.0 * power.log10();
        let mean_above = |gate: f64| {
            let kept: Vec<f64> = blocks.iter().copied().filter(|&p| lufs(p) > gate).collect();
            (!kept.is_empty()).then(|| kept.iter().sum::<f64>() / kept.len() as f64)
        };
        let ungated = mean_above(ABSOLUTE_GATE)?;
        let gated = mean_above(lufs(ungated) + RELATIVE_GATE)?;
        Some(Loudness {
            lufs: lufs(gated) as f32,
            peak_db: 20.0 * self.peak.max(1e-9).log10(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(sr: f32, freq: f32, db: f32, secs: f32) -> Vec<f32> {
        let amp = 10f32.powf(db / 20.0);
        (0..(sr * secs) as usize)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / sr).sin())
            .collect()
    }

    fn measure(sr: f32, x: &[f32]) -> Loudness {
        // The same signal in both channels.
        let stereo: Vec<f32> = x.iter().flat_map(|&v| [v, v]).collect();
        let mut m = Meter::new(sr);
        for c in stereo.chunks(4096) {
            m.push(c, 2);
        }
        m.finish().expect("loud enough")
    }

    #[test]
    fn a_1khz_tone_reads_its_level_at_any_rate() {
        // BS.1770's reference: a 1 kHz sine in both channels reads its level in dBFS.
        for sr in [44_100.0, 48_000.0, 96_000.0] {
            let l = measure(sr, &tone(sr, 1000.0, -20.0, 5.0));
            assert!((l.lufs + 20.0).abs() < 0.1, "{sr}: {} LUFS", l.lufs);
            assert!((l.peak_db + 20.0).abs() < 0.1, "{sr}: peak {}", l.peak_db);
        }
    }

    #[test]
    fn silence_is_gated_out_and_bass_counts_less() {
        let sr = 48_000.0;
        let mut x = vec![0.0; sr as usize * 10];
        x.extend(tone(sr, 1000.0, -20.0, 5.0));
        // Blocks straddling the edge pull it a little (that's the standard's gating).
        let l = measure(sr, &x);
        assert!((l.lufs + 20.0).abs() < 0.3, "{} LUFS", l.lufs);
        // K-weighting: 40 Hz reads well under 1 kHz at the same level.
        let low = measure(sr, &tone(sr, 40.0, -20.0, 5.0));
        assert!(low.lufs < -22.0, "{} LUFS", low.lufs);
        assert!(Meter::new(sr).finish().is_none());
        // A mono file is one channel, 3 dB under the same signal in two.
        let mut m = Meter::new(sr);
        m.push(&tone(sr, 1000.0, -20.0, 5.0), 1);
        assert!((m.finish().unwrap().lufs + 23.0).abs() < 0.1);
    }

    #[test]
    fn gain_meets_the_target_without_clipping() {
        let loud = Loudness {
            lufs: -7.0,
            peak_db: -0.1,
        };
        assert_eq!(loud.gain_to(-14.0), -7.0);
        // A quiet track is lifted only as far as its peak allows.
        let quiet = Loudness {
            lufs: -20.0,
            peak_db: -3.0,
        };
        assert_eq!(quiet.gain_to(-14.0), 2.0);
    }
}
