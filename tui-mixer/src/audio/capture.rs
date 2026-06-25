//! System audio capture via Apple ScreenCaptureKit
//!
//! Captures the system audio output mix (everything going to speakers)
//! and computes per-channel peak/RMS for master metering.
//! No BlackHole or virtual audio device required.
//!
//! Requires macOS 13.0+ and Screen Recording permission.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use screencapturekit::cm::CMSampleBufferExt;
use screencapturekit::prelude::*;

/// Real-time meter levels computed from captured audio
#[derive(Debug, Clone, Copy, Default)]
pub struct MeterLevels {
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
}

struct AtomicMeter {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
}

impl AtomicMeter {
    fn new() -> Self {
        Self {
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
            rms_l: AtomicU32::new(0),
            rms_r: AtomicU32::new(0),
        }
    }

    fn update_peak(atom: &AtomicU32, new_val: f32) {
        let new_bits = new_val.to_bits();
        let mut current = atom.load(Ordering::Relaxed);
        loop {
            if new_bits <= current {
                return;
            }
            match atom.compare_exchange_weak(current, new_bits, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn store_rms(&self, rms_l: f32, rms_r: f32) {
        self.rms_l.store(rms_l.to_bits(), Ordering::Relaxed);
        self.rms_r.store(rms_r.to_bits(), Ordering::Relaxed);
    }

    fn load(&self) -> MeterLevels {
        MeterLevels {
            peak_left: f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            peak_right: f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
            rms_left: f32::from_bits(self.rms_l.load(Ordering::Relaxed)),
            rms_right: f32::from_bits(self.rms_r.load(Ordering::Relaxed)),
        }
    }

    fn decay_peaks(&self, factor: f32) {
        let decay = |atom: &AtomicU32| {
            let current = f32::from_bits(atom.load(Ordering::Relaxed));
            let decayed = current * factor;
            atom.store(decayed.to_bits(), Ordering::Relaxed);
        };
        decay(&self.peak_l);
        decay(&self.peak_r);
    }
}

/// Audio output handler that computes per-channel peak/RMS from system audio
struct AudioMeterHandler {
    meters: Arc<AtomicMeter>,
}

impl SCStreamOutputTrait for AudioMeterHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }

        let audio_list = match sample.audio_buffer_list() {
            Some(list) => list,
            None => return,
        };

        // ScreenCaptureKit typically provides interleaved stereo (one buffer, 2 channels)
        // but could also provide deinterleaved (two buffers, 1 channel each)
        match audio_list.num_buffers() {
            1 => {
                // Interleaved: L, R, L, R, ... (or possibly >2 channels)
                if let Some(buf) = audio_list.get(0) {
                    let data = buf.data();
                    let channels = buf.number_channels as usize;
                    if channels == 0 {
                        return;
                    }
                    let sample_size = std::mem::size_of::<f32>();
                    let total_samples = data.len() / sample_size;
                    let num_frames = total_samples / channels;
                    if num_frames == 0 {
                        return;
                    }
                    let mut sum_l_sq: f64 = 0.0;
                    let mut sum_r_sq: f64 = 0.0;
                    let mut peak_l: f32 = 0.0;
                    let mut peak_r: f32 = 0.0;
                    for frame in 0..num_frames {
                        let base = frame * channels * sample_size;
                        if base + sample_size > data.len() {
                            break;
                        }
                        let l = f32::from_ne_bytes(data[base..base + sample_size].try_into().unwrap_or([0; 4])).abs();
                        let r = if channels >= 2 && base + 2 * sample_size <= data.len() {
                            f32::from_ne_bytes(data[base + sample_size..base + 2 * sample_size].try_into().unwrap_or([0; 4])).abs()
                        } else {
                            l // mono: duplicate to both channels
                        };
                        peak_l = peak_l.max(l);
                        peak_r = peak_r.max(r);
                        sum_l_sq += (l as f64) * (l as f64);
                        sum_r_sq += (r as f64) * (r as f64);
                    }
                    let rms_l = (sum_l_sq / num_frames as f64).sqrt() as f32;
                    let rms_r = (sum_r_sq / num_frames as f64).sqrt() as f32;
                    AtomicMeter::update_peak(&self.meters.peak_l, peak_l.min(1.0));
                    AtomicMeter::update_peak(&self.meters.peak_r, peak_r.min(1.0));
                    self.meters.store_rms(rms_l.min(1.0), rms_r.min(1.0));
                }
            }
            2 => {
                // Deinterleaved: buffer 0 = L, buffer 1 = R
                let (data_l, peak_l, rms_l) = compute_channel(audio_list.get(0));
                let (data_r, peak_r, rms_r) = compute_channel(audio_list.get(1));
                if data_l || data_r {
                    AtomicMeter::update_peak(&self.meters.peak_l, peak_l.min(1.0));
                    AtomicMeter::update_peak(&self.meters.peak_r, peak_r.min(1.0));
                    self.meters.store_rms(rms_l.min(1.0), rms_r.min(1.0));
                }
            }
            _ => {
                // Unusual: just read first two buffers if available
                let (has_l, peak_l, rms_l) = compute_channel(audio_list.get(0));
                let (has_r, peak_r, rms_r) = compute_channel(audio_list.get(1));
                if has_l || has_r {
                    AtomicMeter::update_peak(&self.meters.peak_l, peak_l.min(1.0));
                    AtomicMeter::update_peak(&self.meters.peak_r, peak_r.min(1.0));
                    self.meters.store_rms(rms_l.min(1.0), rms_r.min(1.0));
                }
            }
        }
    }
}

/// Compute peak and RMS for a single audio buffer.
/// Returns (has_data, peak, rms).
fn compute_channel(buf: Option<&screencapturekit::cm::AudioBuffer>) -> (bool, f32, f32) {
    let buf = match buf {
        Some(b) => b,
        None => return (false, 0.0, 0.0),
    };
    let data = buf.data();
    let sample_size = std::mem::size_of::<f32>();
    let num_samples = data.len() / sample_size;
    if num_samples == 0 {
        return (false, 0.0, 0.0);
    }
    let mut sum_sq: f64 = 0.0;
    let mut peak: f32 = 0.0;
    for i in 0..num_samples {
        let byte_offset = i * sample_size;
        if byte_offset + sample_size > data.len() {
            break;
        }
        let s = f32::from_ne_bytes(data[byte_offset..byte_offset + sample_size].try_into().unwrap_or([0; 4])).abs();
        peak = peak.max(s);
        sum_sq += (s as f64) * (s as f64);
    }
    let rms = (sum_sq / num_samples as f64).sqrt() as f32;
    (true, peak, rms)
}

pub struct AudioCapture {
    meters: Arc<AtomicMeter>,
    playing: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
}

impl AudioCapture {
    /// Create a new audio capture using ScreenCaptureKit system audio.
    ///
    /// Captures all audio going to speakers (TidalCycles, MPV, system sounds).
    /// Requires macOS 13.0+ and Screen Recording permission in System Settings.
    pub fn new() -> anyhow::Result<Self> {
        let meters = Arc::new(AtomicMeter::new());
        let playing = Arc::new(AtomicBool::new(true));

        let meters_clone = meters.clone();
        let playing_clone = playing.clone();

        let _thread = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                if let Err(e) = Self::capture_loop(meters_clone, playing_clone) {
                    tracing::error!("Audio capture thread exited: {}", e);
                }
            })
            .map_err(|e| anyhow::anyhow!("Failed to spawn capture thread: {}", e))?;

        tracing::info!("System audio capture started (ScreenCaptureKit)");

        Ok(Self {
            meters,
            playing,
            _thread,
        })
    }

    fn capture_loop(meters: Arc<AtomicMeter>, playing: Arc<AtomicBool>) -> anyhow::Result<()> {
        let content = SCShareableContent::get()
            .map_err(|e| anyhow::anyhow!("Failed to get shareable content: {:?}", e))?;

        let display = content.displays().into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("No display found"))?;

        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();

        let config = SCStreamConfiguration::new()
            .with_captures_audio(true)
            .with_sample_rate(48000)
            .with_channel_count(2)
            .with_width(2)
            .with_height(2);

        let handler = AudioMeterHandler {
            meters: meters.clone(),
        };

        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(handler, SCStreamOutputType::Audio);

        stream.start_capture()
            .map_err(|e| anyhow::anyhow!("Failed to start capture: {:?}", e))?;

        tracing::info!("ScreenCaptureKit audio capture running");

        loop {
            if !playing.load(Ordering::Relaxed) {
                thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    pub fn pause(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.playing.store(true, Ordering::Relaxed);
    }

    /// Read the current meter levels from the capture thread.
    /// Also decays peak hold values so they fall over time.
    pub fn read_meters(&self) -> MeterLevels {
        self.meters.decay_peaks(0.92);
        self.meters.load()
    }
}
