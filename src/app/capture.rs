//! System audio capture (what the computer is playing), per platform:
//! - Windows: WASAPI loopback on the default output device (via cpal).
//! - macOS 14.2+: a Core Audio process tap on the default output device (via cpal). The
//!   first run asks for "System Audio Recording" permission.
//! - Linux: the PulseAudio / PipeWire monitor of the default output.

use super::audio::{Feed, SharedFeed};
use std::sync::{Arc, Mutex};

pub struct Capture {
    _inner: Inner,
}

#[cfg(not(target_os = "linux"))]
type Inner = cpal::Stream;

#[cfg(not(target_os = "linux"))]
pub fn start(feed: SharedFeed, status: Arc<Mutex<String>>) -> Result<Capture, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device found")?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("Can't read the output device: {e}"))?;
    let channels = supported.channels() as usize;
    let sr = supported.sample_rate() as f32;
    *feed.lock().unwrap() = Some(Feed::new(sr));
    let config = supported.config();

    macro_rules! build {
        ($t:ty) => {{
            let feed = feed.clone();
            let status = status.clone();
            let mut mono: Vec<f32> = Vec::with_capacity(8192);
            device.build_input_stream::<$t, _, _>(
                config,
                move |data: &[$t], _info: &cpal::InputCallbackInfo| {
                    mono.clear();
                    for frame in data.chunks(channels.max(1)) {
                        let sum: f32 = frame
                            .iter()
                            .map(|&s| <f32 as cpal::Sample>::from_sample::<$t>(s))
                            .sum();
                        mono.push(sum / channels as f32);
                    }
                    if let Ok(mut g) = feed.lock() {
                        if let Some(f) = g.as_mut() {
                            f.push(&mono, 0.0);
                        }
                    }
                },
                move |e| {
                    if let Ok(mut s) = status.lock() {
                        *s = format!("System audio stopped: {e}");
                    }
                },
                None,
            )
        }};
    }

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::U16 => build!(u16),
        other => return Err(format!("Unsupported audio format {other}")),
    }
    .map_err(|e| explain(&e.to_string()))?;
    stream.play().map_err(|e| explain(&e.to_string()))?;
    Ok(Capture { _inner: stream })
}

#[cfg(not(target_os = "linux"))]
fn explain(e: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("Can't capture system audio (needs macOS 14.2+ and System Audio Recording permission): {e}")
    } else {
        format!("Can't capture system audio: {e}")
    }
}

#[cfg(target_os = "linux")]
struct Inner {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(target_os = "linux")]
pub fn start(feed: SharedFeed, status: Arc<Mutex<String>>) -> Result<Capture, String> {
    use libpulse_binding::def::BufferAttr;
    use libpulse_binding::sample::{Format, Spec};
    use libpulse_binding::stream::Direction;
    use libpulse_simple_binding::Simple;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    const RATE: u32 = 48_000;
    const CHANNELS: u8 = 2;
    const FRAMES: usize = 512;

    *feed.lock().unwrap() = Some(Feed::new(RATE as f32));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let thread_stop = stop.clone();

    let thread = std::thread::Builder::new()
        .name("system-audio".into())
        .spawn(move || {
            let spec = Spec {
                format: Format::F32le,
                channels: CHANNELS,
                rate: RATE,
            };
            let attr = BufferAttr {
                maxlength: u32::MAX,
                tlength: u32::MAX,
                prebuf: u32::MAX,
                minreq: u32::MAX,
                fragsize: (FRAMES * CHANNELS as usize * 4) as u32,
            };
            let pa = match Simple::new(
                None,
                "Driftfield",
                Direction::Record,
                Some("@DEFAULT_MONITOR@"),
                "System audio visualizer",
                &spec,
                None,
                Some(&attr),
            ) {
                Ok(pa) => {
                    let _ = tx.send(Ok(()));
                    pa
                }
                Err(e) => {
                    let _ = tx.send(Err(format!(
                        "Can't capture system audio (needs PulseAudio or PipeWire): {e:?}"
                    )));
                    return;
                }
            };
            let mut bytes = vec![0u8; FRAMES * CHANNELS as usize * 4];
            let mut mono = vec![0.0f32; FRAMES];
            while !thread_stop.load(Ordering::Relaxed) {
                if let Err(e) = pa.read(&mut bytes) {
                    if let Ok(mut s) = status.lock() {
                        *s = format!("System audio stopped: {e:?}");
                    }
                    break;
                }
                for (m, frame) in mono.iter_mut().zip(bytes.chunks_exact(8)) {
                    let l = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                    let r = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                    *m = (l + r) * 0.5;
                }
                if let Ok(mut g) = feed.lock() {
                    if let Some(f) = g.as_mut() {
                        f.push(&mono, 0.0);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;

    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(())) => Ok(Capture {
            _inner: Inner {
                stop,
                thread: Some(thread),
            },
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("System audio didn't respond".into()),
    }
}
