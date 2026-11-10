//! The demo beat: a small synthesized house loop at 120 bpm (Am–F–C–G), with a two-bar
//! breakdown at the end of every 8-bar phrase. Rendered sample by sample, mono.

use crate::dsp::{Biquad, FilterKind};
use crate::rng::Rng;
use std::f32::consts::TAU;

pub const BPM: f32 = 120.0;
const STEPS_PER_BEAT: f32 = 4.0;
const CHORDS: [[i32; 3]; 4] = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
const BASS_STEPS: [usize; 7] = [0, 3, 6, 8, 10, 11, 14];

fn mtof(m: i32) -> f32 {
    440.0 * 2f32.powf((m - 69) as f32 / 12.0)
}

/// Exponential attack/decay envelope reaching ~-80 dB at `attack + decay`.
#[derive(Clone, Copy)]
struct Env {
    attack: f32,
    decay: f32,
    peak: f32,
}

impl Env {
    fn at(&self, t: f32) -> f32 {
        if t < 0.0 {
            0.0
        } else if t < self.attack {
            self.peak * t / self.attack
        } else {
            self.peak * (-9.21 * (t - self.attack) / self.decay).exp()
        }
    }
    fn done(&self, t: f32) -> bool {
        t > self.attack + self.decay
    }
}

enum Kind {
    Kick {
        phase: f32,
    },
    Noise {
        filter: Biquad,
    },
    Tone {
        phase: f32,
        freq: f32,
    },
    Bass {
        phase: f32,
        freq: f32,
        filter: Biquad,
        len: f32,
    },
    Pad {
        phases: [f32; 2],
        freq: f32,
        filter: Biquad,
        len: f32,
    },
}

struct Voice {
    t: f32,
    env: Env,
    kind: Kind,
}

pub struct DemoSynth {
    sr: f32,
    samples_per_step: f64,
    next_step_at: f64,
    sample: u64,
    step: u64,
    voices: Vec<Voice>,
    noise: Rng,
}

impl DemoSynth {
    pub fn new(sample_rate: f32) -> Self {
        DemoSynth {
            sr: sample_rate,
            samples_per_step: sample_rate as f64 * 60.0 / (BPM * STEPS_PER_BEAT) as f64,
            next_step_at: 0.0,
            sample: 0,
            step: 0,
            voices: Vec::with_capacity(64),
            noise: Rng::new(0xD12F7),
        }
    }

    /// Renders mono samples into `out`.
    pub fn render(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            while self.sample as f64 >= self.next_step_at {
                self.trigger(self.step);
                self.step += 1;
                self.next_step_at += self.samples_per_step;
            }
            *o = self.tick();
            self.sample += 1;
        }
    }

    fn trigger(&mut self, s: u64) {
        let i = (s % 16) as usize;
        let bar = s / 16;
        let phrase = bar % 8;
        let chord = CHORDS[(bar % 4) as usize];
        let breakdown = phrase >= 6;
        let step_len = (self.samples_per_step / self.sr as f64) as f32;
        if i == 0 {
            let cutoff = if breakdown { 2400.0 } else { 1100.0 };
            for &m in &chord {
                self.add(
                    Env {
                        attack: 0.25,
                        decay: step_len * 16.0,
                        peak: 0.028,
                    },
                    Kind::Pad {
                        phases: [0.0, 0.37],
                        freq: mtof(m),
                        filter: Biquad::new(FilterKind::LowPass, self.sr, cutoff, 0.707),
                        len: step_len * 16.0,
                    },
                );
            }
        }
        if !breakdown {
            if i.is_multiple_of(4) {
                self.add(
                    Env {
                        attack: 0.003,
                        decay: 0.34,
                        peak: 1.0,
                    },
                    Kind::Kick { phase: 0.0 },
                );
            }
            if i == 4 || i == 12 {
                self.snare();
            }
            if BASS_STEPS.contains(&i) {
                let up = if i == 6 || i == 14 { 12 } else { 0 };
                let len = step_len * 1.6;
                self.add(
                    Env {
                        attack: 0.005,
                        decay: len,
                        peak: 0.32,
                    },
                    Kind::Bass {
                        phase: 0.0,
                        freq: mtof(chord[0] - 24 + up),
                        filter: Biquad::new(FilterKind::LowPass, self.sr, 900.0, 6.0),
                        len,
                    },
                );
            }
        } else if phrase == 7 && i >= 8 && i.is_multiple_of(2) {
            self.snare();
        }
        let hat = if breakdown { 0.06 } else { 0.12 };
        if i % 2 == 1 {
            self.noise_hit(FilterKind::HighPass, 7500.0, hat, 0.035);
        }
        if i % 4 == 2 {
            self.noise_hit(FilterKind::HighPass, 6500.0, hat * 0.85, 0.16);
        }
    }

    fn snare(&mut self) {
        self.noise_hit(FilterKind::BandPass, 1900.0, 0.5, 0.17);
        self.add(
            Env {
                attack: 0.002,
                decay: 0.08,
                peak: 0.25,
            },
            Kind::Tone {
                phase: 0.0,
                freq: 200.0,
            },
        );
    }

    fn noise_hit(&mut self, kind: FilterKind, freq: f32, peak: f32, decay: f32) {
        let q = if kind == FilterKind::BandPass {
            0.8
        } else {
            0.7
        };
        self.add(
            Env {
                attack: 0.002,
                decay,
                peak,
            },
            Kind::Noise {
                filter: Biquad::new(kind, self.sr, freq, q),
            },
        );
    }

    fn add(&mut self, env: Env, kind: Kind) {
        if self.voices.len() < 96 {
            self.voices.push(Voice { t: 0.0, env, kind });
        }
    }

    fn tick(&mut self) -> f32 {
        let dt = 1.0 / self.sr;
        let mut mix = 0.0;
        let noise = &mut self.noise;
        let sr = self.sr;
        for v in self.voices.iter_mut() {
            let e = v.env.at(v.t);
            let s = match &mut v.kind {
                Kind::Kick { phase } => {
                    let f = 150.0 * (42.0f32 / 150.0).powf((v.t / 0.12).min(1.0));
                    *phase = (*phase + f * dt).fract();
                    (*phase * TAU).sin()
                }
                Kind::Noise { filter } => filter.process(noise.f32() * 2.0 - 1.0),
                Kind::Tone { phase, freq } => {
                    *phase = (*phase + *freq * dt).fract();
                    1.0 - 4.0 * (*phase - 0.5).abs() // triangle
                }
                Kind::Bass {
                    phase,
                    freq,
                    filter,
                    len,
                } => {
                    *phase = (*phase + *freq * dt).fract();
                    if ((v.t * sr) as u32).is_multiple_of(32) {
                        let c = 900.0 * (140.0f32 / 900.0).powf((v.t / *len).min(1.0));
                        filter.set(FilterKind::LowPass, sr, c, 6.0);
                    }
                    filter.process(*phase * 2.0 - 1.0)
                }
                Kind::Pad {
                    phases,
                    freq,
                    filter,
                    len,
                } => {
                    let mut s = 0.0;
                    for (k, p) in phases.iter_mut().enumerate() {
                        let det = if k == 0 { 0.9965 } else { 1.0035 };
                        *p = (*p + *freq * det * dt).fract();
                        s += *p * 2.0 - 1.0;
                    }
                    // Pads use a linear fade-in/out instead of the percussive envelope.
                    let g = (v.t / 0.25).min(1.0) * ((*len - v.t) / 0.3).clamp(0.0, 1.0);
                    v.t += dt;
                    mix += filter.process(s) * 0.028 * g;
                    continue;
                }
            };
            mix += s * e;
            v.t += dt;
        }
        self.voices.retain(|v| match v.kind {
            Kind::Pad { len, .. } => v.t < len,
            _ => !v.env.done(v.t),
        });
        (mix * 0.9).tanh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_audible_bounded_audio() {
        let mut s = DemoSynth::new(48_000.0);
        let mut buf = vec![0.0; 48_000 * 2];
        s.render(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let rms = (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt();
        assert!(peak <= 1.0, "clipped: {peak}");
        assert!(rms > 0.05, "too quiet: {rms}");
        assert!(buf.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn same_output_regardless_of_buffer_size() {
        let mut a = DemoSynth::new(44_100.0);
        let mut b = DemoSynth::new(44_100.0);
        let mut x = vec![0.0; 44_100];
        let mut y = vec![0.0; 44_100];
        a.render(&mut x);
        for chunk in y.chunks_mut(333) {
            b.render(chunk);
        }
        assert_eq!(x, y);
    }

    #[test]
    fn voices_do_not_pile_up() {
        let mut s = DemoSynth::new(48_000.0);
        let mut buf = vec![0.0; 4_800];
        for _ in 0..200 {
            s.render(&mut buf);
        }
        assert!(s.voices.len() < 40, "{} voices alive", s.voices.len());
    }
}
