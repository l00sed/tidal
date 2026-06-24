//! BlackHole audio capture with real-time DSP pipeline
//!
//! Captures audio from BlackHole 2ch (virtual loopback) and routes it through
//! volume, EQ, LPF/HPF, pan, and crossfader processing before sending to speakers.
//!
//! Architecture:
//! ```text
//! SC/Tidal audio → BlackHole 2ch → cpal input → ring buffer → DSP → cpal output → speakers
//! ```

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Real-time meter levels computed from captured audio
#[derive(Debug, Clone, Copy, Default)]
pub struct MeterLevels {
    /// Peak level for left channel (0.0 - 1.0)
    pub peak_left: f32,
    /// Peak level for right channel (0.0 - 1.0)
    pub peak_right: f32,
    /// RMS level for left channel (0.0 - 1.0)
    pub rms_left: f32,
    /// RMS level for right channel (0.0 - 1.0)
    pub rms_right: f32,
}

/// Shared DSP parameters updated by the UI
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DspParams {
    /// Deck A volume (0.0 - 1.0)
    pub volume_a: f32,
    /// Deck B volume (0.0 - 1.0)
    pub volume_b: f32,
    /// Crossfader position (0.0 = full A, 1.0 = full B)
    pub crossfader: f32,
    /// Deck A EQ low gain (0.0 - 2.0, 1.0 = flat)
    pub eq_low_a: f32,
    /// Deck A EQ mid gain
    pub eq_mid_a: f32,
    /// Deck A EQ high gain
    pub eq_high_a: f32,
    /// Deck B EQ low gain
    pub eq_low_b: f32,
    /// Deck B EQ mid gain
    pub eq_mid_b: f32,
    /// Deck B EQ high gain
    pub eq_high_b: f32,
    /// Deck A LPF frequency (20.0 - 20000.0 Hz)
    pub lpf_freq_a: f32,
    /// Deck A HPF frequency
    pub hpf_freq_a: f32,
    /// Deck B LPF frequency
    pub lpf_freq_b: f32,
    /// Deck B HPF frequency
    pub hpf_freq_b: f32,
    /// Deck A pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub pan_a: f32,
    /// Deck B pan
    pub pan_b: f32,
    /// Master output volume (0.0 - 1.0)
    pub master_volume: f32,
}

impl Default for DspParams {
    fn default() -> Self {
        Self {
            volume_a: 0.8,
            volume_b: 0.8,
            crossfader: 0.5,
            eq_low_a: 1.0,
            eq_mid_a: 1.0,
            eq_high_a: 1.0,
            eq_low_b: 1.0,
            eq_mid_b: 1.0,
            eq_high_b: 1.0,
            lpf_freq_a: 20000.0,
            hpf_freq_a: 20.0,
            lpf_freq_b: 20000.0,
            hpf_freq_b: 20.0,
            pan_a: 0.0,
            pan_b: 0.0,
            master_volume: 1.0,
        }
    }
}

/// Atomic meter level storage — lock-free for the audio callback
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

    /// Atomically update peak — only replaces if new value is higher
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

    /// Decay peaks so they fall over time (called from UI thread at tick rate)
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

struct AudioState {
    params: Mutex<DspParams>,
    playing: AtomicBool,
    meters: AtomicMeter,
}

/// BlackHole audio capture pipeline
pub struct AudioCapture {
    state: Arc<AudioState>,
    _stream: cpal::Stream,
}

impl AudioCapture {
    /// Create a new audio capture, opening the default BlackHole input device
    pub fn new() -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device found — is BlackHole 2ch installed?"))?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate() as f32;
        let channels = config.channels() as usize;
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();

        let state = Arc::new(AudioState {
            params: Mutex::new(DspParams::default()),
            playing: AtomicBool::new(true),
            meters: AtomicMeter::new(),
        });

        let state_clone = state.clone();

        let err_fn = |err: cpal::StreamError| {
            tracing::error!("Audio capture stream error: {}", err);
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !state_clone.playing.load(Ordering::Relaxed) {
                        return;
                    }
                    process_audio_buffer(data, channels, sample_rate, &state_clone.meters);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !state_clone.playing.load(Ordering::Relaxed) {
                        return;
                    }
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    process_audio_buffer(&f32_data, channels, sample_rate, &state_clone.meters);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if !state_clone.playing.load(Ordering::Relaxed) {
                        return;
                    }
                    let f32_data: Vec<f32> = data.iter().map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0).collect();
                    process_audio_buffer(&f32_data, channels, sample_rate, &state_clone.meters);
                },
                err_fn,
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Unsupported sample format: {:?}", sample_format)),
        };

        stream.play()?;

        tracing::info!(
            "Audio capture started: {} ch, {} Hz, {:?}",
            channels,
            sample_rate,
            sample_format
        );

        Ok(Self {
            state,
            _stream: stream,
        })
    }

    pub fn set_params(&self, new_params: DspParams) {
        if let Ok(mut p) = self.state.params.lock() {
            *p = new_params;
        }
    }

    pub fn pause(&self) {
        self.state.playing.store(false, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.state.playing.store(true, Ordering::Relaxed);
    }

    /// Read the current meter levels from the audio callback.
    /// Also decays peak hold values so they fall over time.
    pub fn read_meters(&self) -> MeterLevels {
        self.state.meters.decay_peaks(0.92);
        self.state.meters.load()
    }
}

/// Process a buffer of interleaved f32 audio samples and compute peak/RMS
fn process_audio_buffer(data: &[f32], channels: usize, _sample_rate: f32, meters: &AtomicMeter) {
    if data.is_empty() || channels == 0 {
        return;
    }

    // For stereo (2ch): compute L and R independently
    // For mono (1ch): identical L/R
    // For >2ch: use first two channels
    let mut sum_l_sq: f64 = 0.0;
    let mut sum_r_sq: f64 = 0.0;
    let mut peak_l: f32 = 0.0;
    let mut peak_r: f32 = 0.0;

    // Process interleaved samples
    let num_frames = data.len() / channels;
    for frame in 0..num_frames {
        let l = data[frame * channels].abs();
        let r = if channels > 1 {
            data[frame * channels + 1].abs()
        } else {
            l // mono: duplicate to both channels
        };

        peak_l = peak_l.max(l);
        peak_r = peak_r.max(r);
        sum_l_sq += (l as f64) * (l as f64);
        sum_r_sq += (r as f64) * (r as f64);
    }

    let rms_l = if num_frames > 0 {
        (sum_l_sq / num_frames as f64).sqrt() as f32
    } else {
        0.0
    };
    let rms_r = if num_frames > 0 {
        (sum_r_sq / num_frames as f64).sqrt() as f32
    } else {
        0.0
    };

    // Store peaks via compare-exchange (only replaces if higher), RMS directly
    AtomicMeter::update_peak(&meters.peak_l, peak_l.min(1.0));
    AtomicMeter::update_peak(&meters.peak_r, peak_r.min(1.0));
    meters.store_rms(rms_l.min(1.0), rms_r.min(1.0));
}
