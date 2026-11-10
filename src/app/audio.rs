//! Audio engine: system-audio capture plus the demo beat, each feeding its own analyzer.

use super::capture::{self, Capture};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use driftfield::analysis::{Analyzer, Levels};
use driftfield::demo::DemoSynth;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// An analyzer plus the clock mapping needed to know what the listener hears right now.
pub struct Feed {
    pub analyzer: Analyzer,
    /// (stream time of the last pushed sample, wall time it was pushed, output latency).
    anchor: Option<(f64, Instant, f64)>,
}

impl Feed {
    pub fn new(sample_rate: f32) -> Self {
        Feed {
            analyzer: Analyzer::new(sample_rate),
            anchor: None,
        }
    }

    /// Feed mono samples. `latency` is how long until the last of them reaches the speakers
    /// (zero for captured system audio, which has already played).
    pub fn push(&mut self, mono: &[f32], latency: f64) {
        self.analyzer.push(mono);
        self.anchor = Some((self.analyzer.time(), Instant::now(), latency));
    }

    /// Stream time the listener is hearing now, or None if audio has stopped arriving.
    fn heard_now(&self) -> Option<f64> {
        let (t, at, latency) = self.anchor?;
        let age = at.elapsed().as_secs_f64();
        (age < 0.5).then(|| t + age.min(0.25) - latency)
    }
}

pub type SharedFeed = Arc<Mutex<Option<Feed>>>;

#[derive(Clone, Debug, Default)]
pub struct AudioFrame {
    pub levels: Levels,
    pub beat: bool,
    /// Beat period in seconds.
    pub period: f64,
    /// Audio is arriving from the selected source.
    pub live: bool,
}

pub struct AudioEngine {
    system: Option<Capture>,
    system_feed: SharedFeed,
    system_status: Arc<Mutex<String>>,
    demo: Option<cpal::Stream>,
    demo_feed: SharedFeed,
    demo_error: Option<String>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let mut engine = AudioEngine {
            system: None,
            system_feed: Arc::new(Mutex::new(None)),
            system_status: Arc::new(Mutex::new(String::new())),
            demo: None,
            demo_feed: Arc::new(Mutex::new(None)),
            demo_error: None,
        };
        engine.start_system();
        engine
    }

    pub fn start_system(&mut self) {
        self.system = None;
        set(&self.system_status, "Connecting to system audio…");
        match capture::start(self.system_feed.clone(), self.system_status.clone()) {
            Ok(c) => {
                set(&self.system_status, "Listening to system audio");
                self.system = Some(c);
            }
            Err(e) => set(&self.system_status, &e),
        }
    }

    pub fn system_ok(&self) -> bool {
        self.system.is_some()
    }

    pub fn demo_on(&self) -> bool {
        self.demo.is_some()
    }

    pub fn set_demo(&mut self, on: bool) {
        if !on {
            self.demo = None;
            *self.demo_feed.lock().unwrap() = None;
            return;
        }
        if self.demo.is_some() {
            return;
        }
        match start_demo(self.demo_feed.clone()) {
            Ok(s) => {
                self.demo = Some(s);
                self.demo_error = None;
            }
            Err(e) => self.demo_error = Some(e),
        }
    }

    pub fn status(&self, present: bool) -> String {
        if self.demo_on() {
            return "Demo beat · house loop".into();
        }
        if let Some(e) = &self.demo_error {
            return format!("Demo beat failed: {e}");
        }
        let s = self.system_status.lock().unwrap().clone();
        if self.system_ok() && !present && s.starts_with("Listening") {
            return "Listening · nothing playing".into();
        }
        s
    }

    /// Call once per video frame.
    pub fn frame(&mut self) -> AudioFrame {
        let feed = if self.demo_on() {
            &self.demo_feed
        } else {
            &self.system_feed
        };
        let mut guard = feed.lock().unwrap();
        let Some(f) = guard.as_mut() else {
            return AudioFrame::default();
        };
        match f.heard_now() {
            Some(heard) => AudioFrame {
                beat: f.analyzer.poll_beat(heard),
                levels: f.analyzer.levels(),
                period: f.analyzer.period(),
                live: true,
            },
            None => AudioFrame {
                period: f.analyzer.period(),
                ..Default::default()
            },
        }
    }
}

fn set(status: &Mutex<String>, s: &str) {
    if let Ok(mut g) = status.lock() {
        *g = s.to_string();
    }
}

/// Plays the demo synth on the default output device and analyzes exactly what it plays.
fn start_demo(feed: SharedFeed) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device found")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = supported.channels() as usize;
    let sr = supported.sample_rate() as f32;
    *feed.lock().unwrap() = Some(Feed::new(sr));
    let config = supported.config();
    let mut synth = DemoSynth::new(sr);
    let mut mono: Vec<f32> = Vec::with_capacity(8192);

    macro_rules! build {
        ($t:ty) => {
            device.build_output_stream::<$t, _, _>(
                config,
                move |data: &mut [$t], info: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels.max(1);
                    mono.resize(frames, 0.0);
                    synth.render(&mut mono);
                    for (frame, &m) in data.chunks_mut(channels).zip(mono.iter()) {
                        let v = <$t as cpal::Sample>::from_sample::<f32>(m);
                        frame.iter_mut().for_each(|s| *s = v);
                    }
                    let ts = info.timestamp();
                    let latency = ts.playback.duration_since(ts.callback).as_secs_f64()
                        + frames as f64 / sr as f64;
                    if let Ok(mut g) = feed.lock() {
                        if let Some(f) = g.as_mut() {
                            f.push(&mono, latency);
                        }
                    }
                },
                |e| eprintln!("demo output error: {e}"),
                None,
            )
        };
    }

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::I32 => build!(i32),
        other => return Err(format!("unsupported output format {other}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
