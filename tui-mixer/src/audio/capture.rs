//! BlackHole audio capture with real-time DSP pipeline
//!
//! Captures audio from BlackHole 2ch (virtual loopback) and routes it through
//! volume, EQ, LPF/HPF, pan, and crossfader processing before sending to speakers.
//!
//! Architecture:
//! ```text
//! SC/Tidal audio → BlackHole 2ch → cpal input → ring buffer → DSP → cpal output → speakers
//! ```

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use crossbeam::queue::ArrayQueue;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Buffer size for the ring buffer (samples per channel)
/// With 48kHz and 256-sample frames, ~21ms of latency
const RING_BUFFER_SIZE: usize = 4096;

/// Shared DSP parameters updated by the UI
#[derive(Debug, Clone)]
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

/// Second-order biquad filter for LPF, HPF, and peaking EQ
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// Low-pass filter at given frequency
    fn lowpass(sample_rate: f32, freq: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / 2.0;
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cos_w0) / 2.0) / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: ((1.0 - cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// High-pass filter at given frequency
    fn highpass(sample_rate: f32, freq: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / 2.0;
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 + cos_w0) / 2.0) / a0,
            b1: -(1.0 + cos_w0) / a0,
            b2: ((1.0 + cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Peaking EQ filter at given frequency with gain in dB
    fn peaking_eq(sample_rate: f32, freq: f32, gain_db: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let a = 10.0f32.powf(gain_db / 40.0);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos_w0 / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha / a) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    /// Update coefficients without resetting state (for smooth parameter changes)
    fn update_lowpass(&mut self, sample_rate: f32, freq: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / 2.0;
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cos_w0) / 2.0) / a0;
        self.b1 = (1.0 - cos_w0) / a0;
        self.b2 = ((1.0 - cos_w0) / 2.0) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn update_highpass(&mut self, sample_rate: f32, freq: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / 2.0;
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 + cos_w0) / 2.0) / a0;
        self.b1 = -(1.0 + cos_w0) / a0;
        self.b2 = ((1.0 + cos_w0) / 2.0) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn update_peaking_eq(&mut self, sample_rate: f32, freq: f32, gain_db: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let a = 10.0f32.powf(gain_db / 40.0);
        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = -2.0 * cos_w0 / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = -2.0 * cos_w0 / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }
}

/// Per-deck DSP state with filters for left and right channels
struct DeckDsp {
    lpf: [Biquad; 2],
    hpf: [Biquad; 2],
    eq_low: [Biquad; 2],
    eq_mid: [Biquad; 2],
    eq_high: [Biquad; 2],
    sample_rate: f32,
}

impl DeckDsp {
    fn new(sample_rate: f32) -> Self {
        Self {
            lpf: [Biquad::lowpass(sample_rate, 20000.0), Biquad::lowpass(sample_rate, 20000.0)],
            hpf: [Biquad::highpass(sample_rate, 20.0), Biquad::highpass(sample_rate, 20.0)],
            eq_low: [Biquad::peaking_eq(sample_rate, 100.0, 0.0, 0.7), Biquad::peaking_eq(sample_rate, 100.0, 0.0, 0.7)],
            eq_mid: [Biquad::peaking_eq(sample_rate, 1000.0, 0.0, 0.7), Biquad::peaking_eq(sample_rate, 1000.0, 0.0, 0.7)],
            eq_high: [Biquad::peaking_eq(sample_rate, 8000.0, 0.0, 0.7), Biquad::peaking_eq(sample_rate, 8000.0, 0.0, 0.7)],
            sample_rate,
        }
    }

    /// Update filter coefficients when params change (coefficients only, no state reset)
    fn update_filters(
        &mut self,
        lpf_freq: f32,
        hpf_freq: f32,
        eq_low_db: f32,
        eq_mid_db: f32,
        eq_high_db: f32,
    ) {
        self.lpf[0].update_lowpass(self.sample_rate, lpf_freq);
        self.lpf[1].update_lowpass(self.sample_rate, lpf_freq);
        self.hpf[0].update_highpass(self.sample_rate, hpf_freq);
        self.hpf[1].update_highpass(self.sample_rate, hpf_freq);
        self.eq_low[0].update_peaking_eq(self.sample_rate, 100.0, eq_low_db, 0.7);
        self.eq_low[1].update_peaking_eq(self.sample_rate, 100.0, eq_low_db, 0.7);
        self.eq_mid[0].update_peaking_eq(self.sample_rate, 1000.0, eq_mid_db, 0.7);
        self.eq_mid[1].update_peaking_eq(self.sample_rate, 1000.0, eq_mid_db, 0.7);
        self.eq_high[0].update_peaking_eq(self.sample_rate, 8000.0, eq_high_db, 0.7);
        self.eq_high[1].update_peaking_eq(self.sample_rate, 8000.0, eq_high_db, 0.7);
    }

    /// Process stereo samples through all filters
    fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let l = self.hpf[0].process(self.eq_low[0].process(self.eq_mid[0].process(
            self.eq_high[0].process(self.lpf[0].process(left)),
        )));
        let r = self.hpf[1].process(self.eq_low[1].process(self.eq_mid[1].process(
            self.eq_high[1].process(self.lpf[1].process(right)),
        )));
        (l, r)
    }
}

/// Shared state between input and output audio callbacks
struct AudioState {
    /// Lock-free ring buffer for captured audio (interleaved L, R samples)
    ring_buffer: ArrayQueue<f32>,
    /// DSP parameters from UI
    params: Mutex<DspParams>,
    /// Whether the pipeline is active (play state)
    playing: AtomicBool,
    /// Per-deck DSP filter state (shared with output callback)
    deck_a_dsp: Mutex<DeckDsp>,
    deck_b_dsp: Mutex<DeckDsp>,
    /// Sample rate for filter coefficient calculation
    sample_rate: f32,
}

/// BlackHole audio capture pipeline
pub struct AudioCapture {
    state: Arc<AudioState>,
    _input_stream: Option<Stream>,
    _output_stream: Option<Stream>,
}

impl AudioCapture {
    /// Create a new audio capture instance
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();

        // Find BlackHole device
        let input_device = host
            .input_devices()
            .map_err(|e| format!("Failed to list input devices: {}", e))?
            .find(|d| {
                d.name()
                    .map(|n| n.contains("BlackHole"))
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                "BlackHole not found. Install BlackHole 2ch: brew install blackhole-2ch".to_string()
            })?;

        // Get input config
        let input_config = input_device
            .default_input_config()
            .map_err(|e| format!("No default input config: {}", e))?;

        let sample_rate = input_config.sample_rate().0 as f32;
        let channels = input_config.channels() as usize;

        // Find output device (speakers)
        let output_device = host
            .default_output_device()
            .ok_or("No output device found")?;

        let output_config = output_device
            .default_output_config()
            .map_err(|e| format!("No default output config: {}", e))?;

        let state = Arc::new(AudioState {
            ring_buffer: ArrayQueue::new(RING_BUFFER_SIZE),
            params: Mutex::new(DspParams::default()),
            playing: AtomicBool::new(true),
            deck_a_dsp: Mutex::new(DeckDsp::new(sample_rate)),
            deck_b_dsp: Mutex::new(DeckDsp::new(sample_rate)),
            sample_rate,
        });

        let state_input = state.clone();
        let state_output = state.clone();

        // Build input stream (capture from BlackHole)
        let input_channels = channels;
        let input_stream = match input_config.sample_format() {
            cpal::SampleFormat::F32 => {
                input_device
                    .build_input_stream(
                        &StreamConfig {
                            channels: input_channels as u16,
                            sample_rate: input_config.sample_rate(),
                            buffer_size: cpal::BufferSize::Default,
                        },
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            // Push captured samples to ring buffer
                            // If stereo, push L,R pairs; if mono, duplicate to both channels
                            if input_channels >= 2 {
                                for chunk in data.chunks(2) {
                                    let l = chunk[0];
                                    let r = chunk.get(1).copied().unwrap_or(l);
                                    let _ = state_input.ring_buffer.push(l);
                                    let _ = state_input.ring_buffer.push(r);
                                }
                            } else {
                                for &sample in data {
                                    let _ = state_input.ring_buffer.push(sample);
                                    let _ = state_input.ring_buffer.push(sample);
                                }
                            }
                        },
                        |err| eprintln!("Input stream error: {}", err),
                        None,
                    )
                    .map_err(|e| format!("Failed to build input stream: {}", e))?
            }
            cpal::SampleFormat::I16 => {
                input_device
                    .build_input_stream(
                        &StreamConfig {
                            channels: input_channels as u16,
                            sample_rate: input_config.sample_rate(),
                            buffer_size: cpal::BufferSize::Default,
                        },
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            // Convert i16 to f32 and push
                            if input_channels >= 2 {
                                for chunk in data.chunks(2) {
                                    let l = chunk[0] as f32 / i16::MAX as f32;
                                    let r = chunk.get(1).copied().unwrap_or(chunk[0]) as f32 / i16::MAX as f32;
                                    let _ = state_input.ring_buffer.push(l);
                                    let _ = state_input.ring_buffer.push(r);
                                }
                            } else {
                                for &sample in data {
                                    let s = sample as f32 / i16::MAX as f32;
                                    let _ = state_input.ring_buffer.push(s);
                                    let _ = state_input.ring_buffer.push(s);
                                }
                            }
                        },
                        |err| eprintln!("Input stream error: {}", err),
                        None,
                    )
                    .map_err(|e| format!("Failed to build input stream: {}", e))?
            }
            fmt => return Err(format!("Unsupported sample format: {:?}", fmt)),
        };

        let output_channels = output_config.channels() as usize;

        // Build output stream (play to speakers)
        let output_stream = match output_config.sample_format() {
            cpal::SampleFormat::F32 => {
                output_device
                    .build_output_stream(
                        &StreamConfig {
                            channels: output_config.channels(),
                            sample_rate: output_config.sample_rate(),
                            buffer_size: cpal::BufferSize::Default,
                        },
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            let playing = state_output.playing.load(Ordering::Relaxed);
                            let params = state_output.params.lock().unwrap().clone();

                            // Update filter coefficients from params
                            {
                                let mut dsp_a = state_output.deck_a_dsp.lock().unwrap();
                                dsp_a.update_filters(
                                    params.lpf_freq_a,
                                    params.hpf_freq_a,
                                    params.eq_low_a,
                                    params.eq_mid_a,
                                    params.eq_high_a,
                                );
                            }
                            {
                                let mut dsp_b = state_output.deck_b_dsp.lock().unwrap();
                                dsp_b.update_filters(
                                    params.lpf_freq_b,
                                    params.hpf_freq_b,
                                    params.eq_low_b,
                                    params.eq_mid_b,
                                    params.eq_high_b,
                                );
                            }

                            // Process stereo output
                            for frame in data.chunks_mut(output_channels) {
                                if !playing || state_output.ring_buffer.len() < 2 {
                                    // Silence when not playing or buffer underrun
                                    for sample in frame.iter_mut() {
                                        *sample = 0.0;
                                    }
                                    continue;
                                }

                                // Read L,R from ring buffer (single BlackHole stereo input)
                                let left = state_output.ring_buffer.pop().unwrap_or(0.0);
                                let right = state_output.ring_buffer.pop().unwrap_or(0.0);

                                // Crossfader gains
                                let xf = params.crossfader;
                                let gain_a = if xf <= 0.5 {
                                    1.0
                                } else {
                                    (1.0 - (xf - 0.5) * 2.0).max(0.0)
                                };
                                let gain_b = if xf >= 0.5 {
                                    1.0
                                } else {
                                    (0.5 + xf).min(1.0)
                                };

                                // Apply deck A filters + volume + crossfader
                                let (filt_l_a, filt_r_a) = {
                                    let mut dsp = state_output.deck_a_dsp.lock().unwrap();
                                    dsp.process_stereo(left, right)
                                };
                                let out_a_l = filt_l_a * params.volume_a * gain_a;
                                let out_a_r = filt_r_a * params.volume_a * gain_a;

                                // Apply deck B filters + volume + crossfader
                                let (filt_l_b, filt_r_b) = {
                                    let mut dsp = state_output.deck_b_dsp.lock().unwrap();
                                    dsp.process_stereo(left, right)
                                };
                                let out_b_l = filt_l_b * params.volume_b * gain_b;
                                let out_b_r = filt_r_b * params.volume_b * gain_b;

                                // Mix both decks
                                let mixed_l = out_a_l + out_b_l;
                                let mixed_r = out_a_r + out_b_r;

                                // Apply pan (deck A pan for now)
                                let pan_a = (params.pan_a + 1.0) / 2.0;
                                let pan_l = mixed_l * (1.0 - pan_a);
                                let pan_r = mixed_r * pan_a;

                                // Master volume
                                let master = params.master_volume;

                                // Write to output
                                if output_channels >= 2 {
                                    frame[0] = pan_l * master;
                                    frame[1] = pan_r * master;
                                } else {
                                    frame[0] = (pan_l + pan_r) * 0.5 * master;
                                }
                            }
                        },
                        |err| eprintln!("Output stream error: {}", err),
                        None,
                    )
                    .map_err(|e| format!("Failed to build output stream: {}", e))?
            }
            cpal::SampleFormat::I16 => {
                output_device
                    .build_output_stream(
                        &StreamConfig {
                            channels: output_config.channels(),
                            sample_rate: output_config.sample_rate(),
                            buffer_size: cpal::BufferSize::Default,
                        },
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            let playing = state_output.playing.load(Ordering::Relaxed);
                            let params = state_output.params.lock().unwrap().clone();

                            // Update filter coefficients from params
                            {
                                let mut dsp_a = state_output.deck_a_dsp.lock().unwrap();
                                dsp_a.update_filters(
                                    params.lpf_freq_a,
                                    params.hpf_freq_a,
                                    params.eq_low_a,
                                    params.eq_mid_a,
                                    params.eq_high_a,
                                );
                            }
                            {
                                let mut dsp_b = state_output.deck_b_dsp.lock().unwrap();
                                dsp_b.update_filters(
                                    params.lpf_freq_b,
                                    params.hpf_freq_b,
                                    params.eq_low_b,
                                    params.eq_mid_b,
                                    params.eq_high_b,
                                );
                            }

                            for frame in data.chunks_mut(output_channels) {
                                if !playing || state_output.ring_buffer.len() < 2 {
                                    for sample in frame.iter_mut() {
                                        *sample = 0;
                                    }
                                    continue;
                                }

                                let left = state_output.ring_buffer.pop().unwrap_or(0.0);
                                let right = state_output.ring_buffer.pop().unwrap_or(0.0);

                                let xf = params.crossfader;
                                let gain_a = if xf <= 0.5 {
                                    1.0
                                } else {
                                    (1.0 - (xf - 0.5) * 2.0).max(0.0)
                                };
                                let gain_b = if xf >= 0.5 {
                                    1.0
                                } else {
                                    (0.5 + xf).min(1.0)
                                };

                                // Apply deck A filters + volume + crossfader
                                let (filt_l_a, filt_r_a) = {
                                    let mut dsp = state_output.deck_a_dsp.lock().unwrap();
                                    dsp.process_stereo(left, right)
                                };
                                let out_a_l = filt_l_a * params.volume_a * gain_a;
                                let out_a_r = filt_r_a * params.volume_a * gain_a;

                                // Apply deck B filters + volume + crossfader
                                let (filt_l_b, filt_r_b) = {
                                    let mut dsp = state_output.deck_b_dsp.lock().unwrap();
                                    dsp.process_stereo(left, right)
                                };
                                let out_b_l = filt_l_b * params.volume_b * gain_b;
                                let out_b_r = filt_r_b * params.volume_b * gain_b;

                                // Mix both decks
                                let mixed_l = out_a_l + out_b_l;
                                let mixed_r = out_a_r + out_b_r;

                                let pan_a = (params.pan_a + 1.0) / 2.0;
                                let pan_l = mixed_l * (1.0 - pan_a);
                                let pan_r = mixed_r * pan_a;

                                let master = params.master_volume;
                                let l_i16 = (pan_l * master * i16::MAX as f32) as i16;
                                let r_i16 = (pan_r * master * i16::MAX as f32) as i16;

                                if output_channels >= 2 {
                                    frame[0] = l_i16;
                                    frame[1] = r_i16;
                                } else {
                                    frame[0] = ((l_i16 as i32 + r_i16 as i32) / 2) as i16;
                                }
                            }
                        },
                        |err| eprintln!("Output stream error: {}", err),
                        None,
                    )
                    .map_err(|e| format!("Failed to build output stream: {}", e))?
            }
            fmt => return Err(format!("Unsupported output format: {:?}", fmt)),
        };

        // Start streams
        input_stream
            .play()
            .map_err(|e| format!("Failed to start input stream: {}", e))?;
        output_stream
            .play()
            .map_err(|e| format!("Failed to start output stream: {}", e))?;

        Ok(Self {
            state,
            _input_stream: Some(input_stream),
            _output_stream: Some(output_stream),
        })
    }

    /// Update DSP parameters
    pub fn set_params(&self, new_params: DspParams) {
        if let Ok(mut p) = self.state.params.lock() {
            *p = new_params;
        }
    }

    /// Get current sample rate
    pub fn sample_rate(&self) -> f32 {
        self.state.sample_rate
    }

    /// Pause audio output (silence)
    pub fn pause(&self) {
        self.state.playing.store(false, Ordering::Relaxed);
    }

    /// Resume audio output
    pub fn resume(&self) {
        self.state.playing.store(true, Ordering::Relaxed);
    }

    /// Check if pipeline is playing
    pub fn is_playing(&self) -> bool {
        self.state.playing.load(Ordering::Relaxed)
    }

    /// List available BlackHole devices
    pub fn list_devices() -> Result<Vec<String>, String> {
        let host = cpal::default_host();
        let devices = host
            .input_devices()
            .map_err(|e| format!("Failed to list devices: {}", e))?;

        Ok(devices
            .filter_map(|d| d.name().ok().filter(|n| n.contains("BlackHole")))
            .collect())
    }
}
