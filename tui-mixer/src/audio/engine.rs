use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::decoder::{AtomicF64, AudioRingBuf, DecoderThread};
use crate::audio::dsp::{AtomicMeter, Biquad, FilterType, LfoOsc, pan_gains};

mod inner;

pub use inner::{AudioCommand, ControlState, DeckState};

const MAX_DECKS: usize = 3;

/// Stereo pair of biquad filters (one per channel).
struct StereoBiquad(Biquad, Biquad);

impl StereoBiquad {
    fn new(sr: f32) -> Self {
        Self(Biquad::new(sr), Biquad::new(sr))
    }
    fn set_params(&mut self, ft: FilterType, freq: f32, q: f32, gain_db: f32) {
        self.0.set_params(ft, freq, q, gain_db);
        self.1.set_params(ft, freq, q, gain_db);
    }
    fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.0.tick(l), self.1.tick(r))
    }
}

/// Per-deck DSP chain: filters, EQ, and LFO.
struct DspFilters {
    hpf: StereoBiquad,
    lpf: StereoBiquad,
    eq_lo: StereoBiquad,
    eq_mi: StereoBiquad,
    eq_hi: StereoBiquad,
    lfo: LfoOsc,
    lfo_active: bool,
    lfo_mix: f32,
    prev_l: f32,
    prev_r: f32,
}

impl DspFilters {
    fn new(sr: f32) -> Self {
        let mut f = Self {
            hpf: StereoBiquad::new(sr),
            lpf: StereoBiquad::new(sr),
            eq_lo: StereoBiquad::new(sr),
            eq_mi: StereoBiquad::new(sr),
            eq_hi: StereoBiquad::new(sr),
            lfo: LfoOsc::new(sr),
            lfo_active: false,
            lfo_mix: 0.0,
            prev_l: 0.0,
            prev_r: 0.0,
        };
        f.hpf.set_params(FilterType::HighPass, 20.0, 0.707, 0.0);
        f.lpf.set_params(FilterType::LowPass, 20000.0, 0.707, 0.0);
        f
    }

    /// Update filter coefficients. The LPF/HPF/EQ are set at a fixed frequency
    /// based on cutoff and freq. The LFO, when active, crossfades between
    /// dry (unfiltered) and processed (filtered) signals per-sample in process().
    fn update_params(&mut self, ctrl: &DeckState) {
        self.lfo.set_speed(ctrl.lfo_speed);
        self.lfo.set_shape(ctrl.lfo_shape);
        self.lfo_active = ctrl.lfo_speed > 0.001;

        // Compute filter frequency from freq_pos (log scale, 20–20000 Hz)
        let log_min = 20.0f32.log10();
        let log_max = 20000.0f32.log10();
        let actual_freq = 10.0f32.powf(log_min + ctrl.filter_freq * (log_max - log_min));

        // Crossfade zone: 300Hz–3kHz between LPF and HPF
        let blend = if actual_freq <= 300.0 {
            0.0
        } else if actual_freq >= 3000.0 {
            1.0
        } else {
            let t = (actual_freq - 300.0) / (3000.0 - 300.0);
            t * t * (3.0 - 2.0 * t)
        };

        let lpf_target = actual_freq + (20000.0 - actual_freq) * blend;
        let hpf_target = 20.0 + (actual_freq - 20.0) * blend;

        // Apply intensity (cutoff) as frequency sweep toward the target
        let intensity = ctrl.filter_cutoff.powf(1.2);
        let lpf_hz = (20000.0 - (20000.0 - lpf_target) * intensity).max(200.0).min(20000.0);
        let hpf_hz = (20.0 + (hpf_target - 20.0) * intensity).clamp(20.0, 20000.0);

        // Set filter coefficients ONCE per buffer — no per-sample modulation
        self.lpf.set_params(FilterType::LowPass, lpf_hz, 0.707, 0.0);
        self.hpf.set_params(FilterType::HighPass, hpf_hz, 0.707, 0.0);

        let eq_lo_gain = if ctrl.eq_low_kill { -48.0 } else { ctrl.eq_low };
        let eq_mi_gain = if ctrl.eq_mid_kill { -48.0 } else { ctrl.eq_mid };
        let eq_hi_gain = if ctrl.eq_high_kill { -48.0 } else { ctrl.eq_high };
        self.eq_lo.set_params(FilterType::Peaking, 80.0, 0.707, eq_lo_gain);
        self.eq_mi.set_params(FilterType::Peaking, 1000.0, 0.707, eq_mi_gain);
        self.eq_hi.set_params(FilterType::Peaking, 8000.0, 0.707, eq_hi_gain);
    }

    /// Process stereo sample through the DSP chain.
    /// When LFO is active, amplitude-modulate the filtered signal with a crossfade.
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (l_filt, r_filt) = {
            let (l, r) = self.hpf.tick(l, r);
            let (l, r) = self.lpf.tick(l, r);
            let (l, r) = self.eq_lo.tick(l, r);
            let (l, r) = self.eq_mi.tick(l, r);
            self.eq_hi.tick(l, r)
        };

        // Crossfade: ramp mix toward 1.0 when active, 0.0 when inactive
        let target = if self.lfo_active { 1.0 } else { 0.0 };
        let attack = 0.002;
        let release = 0.001;
        let rate = if target > self.lfo_mix { attack } else { release };
        self.lfo_mix += (target - self.lfo_mix).clamp(-rate, rate);

        if self.lfo_mix > 0.001 {
            if let Some(lfo_val) = self.lfo.tick() {
                let mix = self.lfo_mix;
                // Modulate the filtered signal, not raw input
                let lfo_l = l_filt * lfo_val;
                let lfo_r = r_filt * lfo_val;
                let dry_l = l_filt * (1.0 - mix) + lfo_l * mix;
                let dry_r = r_filt * (1.0 - mix) + lfo_r * mix;
                (dry_l, dry_r)
            } else {
                (l_filt, r_filt)
            }
        } else {
            (l_filt, r_filt)
        }
    }

    fn lfo_debug_line(&self) -> String {
        format!("LFO: act={} ph={:.3} sp={:.3}",
            self.lfo_active, self.lfo.phase, self.lfo.speed)
    }
}

/// Thread-safe handle to a ring buffer for a single deck.
struct SharedBuf(std::sync::RwLock<Option<Arc<AudioRingBuf>>>);
impl SharedBuf {
    fn new() -> Self { Self(std::sync::RwLock::new(None)) }
    fn set(&self, buf: Arc<AudioRingBuf>) { *self.0.write().unwrap() = Some(buf); }
    fn clear(&self) { *self.0.write().unwrap() = None; }
    fn read(&self, out: &mut [f32]) -> usize {
        let guard = self.0.read().unwrap();
        match *guard {
            Some(ref rb) => rb.read(out),
            None => 0,
        }
    }
}

/// The audio engine: opens cpal output and processes audio in a callback.
#[allow(dead_code)]
pub struct AudioEngine {
    pub state: Arc<ControlState>,
    pub meters: [Arc<AtomicMeter>; 3],
    pub master_meter: Arc<AtomicMeter>,
    pub time_pos: [Arc<AtomicF64>; 3],
    pub duration: [Arc<AtomicF64>; 3],
    pub lfo_debug: Arc<Mutex<String>>,
    cmd_tx: mpsc::Sender<AudioCommand>,
    _stream: Option<cpal::Stream>,
    decoders: [Mutex<Option<DecoderThread>>; 3],
    bufs: [Arc<SharedBuf>; 3],
    dsp: Arc<Mutex<[DspFilters; 3]>>,
}

impl AudioEngine {
    pub fn new() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AudioCommand>();
        let cmd_rx = Arc::new(Mutex::new(cmd_rx));
        let state = Arc::new(ControlState::new());

        let meters = [(); 3].map(|_| Arc::new(AtomicMeter::new()));
        let master_meter = Arc::new(AtomicMeter::new());
        let time_pos = [(); 3].map(|_| Arc::new(AtomicF64::new(0.0)));
        let duration = [(); 3].map(|_| Arc::new(AtomicF64::new(0.0)));

        // These don't depend on the device, define before the loop
        let bufs: [Arc<SharedBuf>; 3] = [(); 3].map(|_| Arc::new(SharedBuf::new()));
        let decoders: [Mutex<Option<DecoderThread>>; 3] = [
            Mutex::new(None), Mutex::new(None), Mutex::new(None),
        ];
        let dsp: Arc<Mutex<[DspFilters; 3]>> = Arc::new(Mutex::new([
            DspFilters::new(48000.0),
            DspFilters::new(48000.0),
            DspFilters::new(48000.0),
        ]));
        let lfo_debug = Arc::new(Mutex::new(String::new()));

        let host = cpal::default_host();

        // Try each output device until one works — skip virtual/failing devices
        let all_devices: Vec<_> = host.output_devices()
            .map_err(|e| format!("Output devices: {}", e))?
            .collect();

        if all_devices.is_empty() {
            return Err("No audio output device found".to_string());
        }

        // Prefer default, try alternatives if needed
        let default = host.default_output_device();
        let candidates: Vec<_> = if let Some(ref def) = default {
            std::iter::once(def).chain(all_devices.iter().filter(|d| {
                d.description().ok().map(|n| n.to_string()) != def.description().ok().map(|n| n.to_string())
            })).collect()
        } else {
            all_devices.iter().collect()
        };

        let mut last_err = String::new();
        let mut stream: Option<cpal::Stream> = None;

        for device in &candidates {
            let name = device.description().ok().map(|d| d.to_string()).unwrap_or_default();
            let configs: Vec<_> = match device.supported_output_configs() {
                Ok(c) => c.collect(),
                Err(_) => continue,
            };
            let picked = configs.iter()
                .find(|c| c.channels() >= 2 && c.sample_format() == cpal::SampleFormat::F32)
                .or_else(|| configs.first());
            let picked = match picked {
                Some(p) => p,
                None => continue,
            };

            let fmt = picked.sample_format();
            let sr: u32 = picked.max_sample_rate();
            let cfg = picked.clone().with_sample_rate(sr).config();
            let cb_state_inner = Arc::clone(&state);
            let cb_cmd_rx = Arc::clone(&cmd_rx);
            let cb_meters_inner = meters.clone();
            let cb_master_meter_inner = Arc::clone(&master_meter);
            let cb_bufs_inner = bufs.clone();
            let cb_dsp_inner = Arc::clone(&dsp);
            let cb_lfo_debug_inner = Arc::clone(&lfo_debug);

            let result = match fmt {
                cpal::SampleFormat::F32 => {
                    device.build_output_stream(
                        &cfg,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            audio_callback(data, &cb_state_inner, &cb_cmd_rx, &cb_meters_inner,
                                           &cb_master_meter_inner, &cb_bufs_inner, &cb_dsp_inner,
                                           &cb_lfo_debug_inner);
                        },
                        move |err| eprintln!("Audio: {}", err),
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    device.build_output_stream(
                        &cfg,
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            let frames = data.len() / cfg.channels as usize;
                            let mut float_buf = vec![0.0f32; frames * 2];
                            if cfg.channels >= 2 {
                                for f in 0..frames {
                                    for ci in 0..cfg.channels.min(2) {
                                        let idx = f * cfg.channels as usize + ci as usize;
                                        float_buf[f * 2 + ci as usize] = data[idx] as f32 / 32768.0;
                                    }
                                }
                            }
                            audio_callback(&mut float_buf, &cb_state_inner, &cb_cmd_rx, &cb_meters_inner,
                                           &cb_master_meter_inner, &cb_bufs_inner, &cb_dsp_inner,
                                           &cb_lfo_debug_inner);
                            for f in 0..frames {
                                for ci in 0..cfg.channels.min(2) {
                                    let idx = f * cfg.channels as usize + ci as usize;
                                    data[idx] = (float_buf[f * 2 + ci as usize].clamp(-1.0, 1.0) * 32767.0) as i16;
                                }
                            }
                        },
                        move |err| eprintln!("Audio: {}", err),
                        None,
                    )
                }
                cpal::SampleFormat::I32 => {
                    device.build_output_stream(
                        &cfg,
                        move |data: &mut [i32], _: &cpal::OutputCallbackInfo| {
                            let frames = data.len() / cfg.channels as usize;
                            let mut float_buf = vec![0.0f32; frames * 2];
                            if cfg.channels >= 2 {
                                for f in 0..frames {
                                    for ci in 0..cfg.channels.min(2) {
                                        let idx = f * cfg.channels as usize + ci as usize;
                                        float_buf[f * 2 + ci as usize] = data[idx] as f32 / 2147483648.0;
                                    }
                                }
                            }
                            audio_callback(&mut float_buf, &cb_state_inner, &cb_cmd_rx, &cb_meters_inner,
                                           &cb_master_meter_inner, &cb_bufs_inner, &cb_dsp_inner,
                                           &cb_lfo_debug_inner);
                            for f in 0..frames {
                                for ci in 0..cfg.channels.min(2) {
                                    let idx = f * cfg.channels as usize + ci as usize;
                                    data[idx] = (float_buf[f * 2 + ci as usize].clamp(-1.0, 1.0) * 2147483647.0) as i32;
                                }
                            }
                        },
                        move |err| eprintln!("Audio: {}", err),
                        None,
                    )
                }
                other => {
                    last_err = format!("Unsupported format {:?} on {}", other, name);
                    continue;
                }
            };

            match result {
                Ok(s) => {
                    match s.play() {
                        Ok(_) => {
                            stream = Some(s);
                            eprintln!("Audio: {} at {}Hz ch={}", name, sr, cfg.channels);
                            break;
                        }
                        Err(e) => {
                            last_err = format!("{}: {}", name, e);
                            eprintln!("Audio device '{}' play failed: {} — trying next", name, e);
                        }
                    }
                }
                Err(e) => {
                    last_err = format!("{}: {}", name, e);
                    eprintln!("Audio device '{}' build failed: {} — trying next", name, e);
                }
            }
        }

        let stream = stream.ok_or_else(|| format!("No working audio device: {}", last_err))?;

        Ok(Self {
            state, meters, master_meter, time_pos, duration, lfo_debug,
            cmd_tx, _stream: Some(stream),
            decoders, bufs, dsp,
        })
    }

    /// Load a file for a deck: creates a decoder thread and wires the ring buffer.
    /// I/O happens on the calling thread (UI thread) — do not call from audio callback.
    pub fn load_file(&self, ch: usize, path: String) {
        if ch >= MAX_DECKS { return; }
        match DecoderThread::load(Path::new(&path)) {
            Ok(decoder) => {
                self.bufs[ch].set(Arc::clone(&decoder.ring));
                decoder.play();
                *self.decoders[ch].lock().unwrap() = Some(decoder);
            }
            Err(e) => eprintln!("Audio: load error: {}", e),
        }
    }

    /// Whether a given deck has a decoder loaded (i.e. audio flowing through engine).
    pub fn has_decoder(&self, ch: usize) -> bool {
        self.decoders.get(ch).map(|m| m.lock().unwrap().is_some()).unwrap_or(false)
    }

    /// Stop decoder for a deck (drops the decoder thread, clears ring buffer).
    pub fn stop_decoder(&self, ch: usize) {
        if let Some(decoder) = self.decoders.get(ch) {
            *decoder.lock().unwrap() = None;
        }
        if ch < MAX_DECKS {
            self.bufs[ch].clear();
        }
    }
}

fn audio_callback(
    data: &mut [f32],
    state: &ControlState,
    cmd_rx: &Arc<Mutex<mpsc::Receiver<AudioCommand>>>,
    meters: &[Arc<AtomicMeter>],
    master_meter: &AtomicMeter,
    bufs: &[Arc<SharedBuf>; 3],
    dsp_state: &Mutex<[DspFilters; 3]>,
    lfo_debug: &Mutex<String>,
) {
    // Process pending commands (no state read lock held yet)
    if let Ok(rx) = cmd_rx.lock() {
        for cmd in rx.try_iter() {
            match cmd {
                AudioCommand::Stop(ch) => {
                    if ch < MAX_DECKS { bufs[ch].clear(); }
                }
                AudioCommand::Quit => return,
            }
        }
    }

    // Read state snapshot
    let ctrl = state.read();

    let frames = data.len() / 2;

    let cf = ctrl.master.crossfader.clamp(0.0, 1.0);
    let gain_a = (1.0 - cf).sqrt();
    let gain_b = cf.sqrt();

    let mut deck_buf = vec![0.0f32; frames * 2];

    let mut dm: [crate::audio::dsp::LevelMeter; 3] =
        [(); 3].map(|_| crate::audio::dsp::LevelMeter::new());
    let mut ml = crate::audio::dsp::LevelMeter::new();

    // Lock DSP state and update per-buffer parameters
    let mut dsp_guard = dsp_state.lock().unwrap();
    for d in 0..3 {
        dsp_guard[d].update_params(&ctrl.decks[d]);
    }

    let solo_active = ctrl.master.solo_active;

    for f in 0..frames {
        let mut mix_l = 0.0;
        let mut mix_r = 0.0;

        for d in 0..3 {
            let cd = &ctrl.decks[d];
            let n = bufs[d].read(&mut deck_buf);

            // Sample-and-hold: when ring buffer has fewer samples than the output buffer,
            // keep last known value to avoid sudden silence transitions (clicks).
            let (mut l, mut r) = if n == 0 {
                // Completely dry — output silence cleanly.
                // (prev values reset to 0 to avoid DC buildup.)
                dsp_guard[d].prev_l = 0.0;
                dsp_guard[d].prev_r = 0.0;
                (0.0, 0.0)
            } else if f * 2 < n {
                let l = deck_buf[f * 2];
                let r = deck_buf[f * 2 + 1];
                dsp_guard[d].prev_l = l;
                dsp_guard[d].prev_r = r;
                (l, r)
            } else {
                (dsp_guard[d].prev_l, dsp_guard[d].prev_r)
            };

            // DSP: filters, EQ, LFO
            let processed = dsp_guard[d].process(l, r);
            l = processed.0;
            r = processed.1;

            // Pan
            let (lg, rg) = pan_gains(cd.pan);

            // Volume: crossfader, master fader, mute/solo
            let ch_active = if solo_active { cd.solo } else { !cd.muted };
            let cf_gain = if d == 1 { gain_b } else if d == 2 { 1.0 } else { gain_a };
            let vol = if ch_active && !ctrl.master.muted {
                cd.volume * cf_gain * ctrl.master.fader
            } else {
                0.0
            };

            l *= vol * lg;
            r *= vol * rg;

            dm[d].push_stereo(l, r);

            mix_l += l;
            mix_r += r;
        }

        ml.push_stereo(mix_l, mix_r);
        data[f * 2] = mix_l;
        data[f * 2 + 1] = mix_r;
    }

    for d in 0..3 {
        let (pl, pr, rl, rr) = dm[d].read();
        meters[d].store(pl, pr, rl, rr);
    }
    let (mpl, mpr, mrl, mrr) = ml.read();
    master_meter.store(mpl, mpr, mrl, mrr);

    // Once per second, push LFO debug to the shared buffer for the TUI debug pane
    static DBG_FRAME: AtomicU64 = AtomicU64::new(0);
    if DBG_FRAME.fetch_add(1, Ordering::Relaxed) % 50 == 0 {
        let mut lines = String::new();
        for di in 0..3 {
            if di > 0 { lines.push(' '); }
            let dl = dsp_guard[di].lfo_debug_line();
            let ss = ctrl.decks[di].lfo_speed;
            lines.push_str(&format!("[{}]{} ss={:.3}", di, dl, ss));
        }
        if let Ok(mut s) = lfo_debug.lock() {
            *s = lines;
        }
    }
}
