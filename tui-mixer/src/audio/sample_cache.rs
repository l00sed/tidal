//! Sample cache for instant playback
//!
//! Preloads audio samples into memory for zero-latency triggering.

use crate::audio::effects::CustomEffects;
use rodio::mixer::Mixer;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::collections::HashMap;
use std::fs::File;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct CachedSample {
    pub samples: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl CachedSample {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
        let decoder = Decoder::try_from(file)
            .map_err(|e| format!("Failed to decode {}: {}", path.display(), e))?;
        let sample_rate = decoder.sample_rate().get();
        let channels = decoder.channels().get();
        let samples: Vec<f32> = decoder.collect();
        Ok(Self {
            samples: Arc::new(samples),
            sample_rate,
            channels,
        })
    }
}

#[derive(Clone)]
pub struct CachedSampleSource {
    samples: Arc<Vec<f32>>,
    sample_rate: u32,
    channels: u16,
    position: usize,
}

impl CachedSampleSource {
    pub fn new(cached: &CachedSample) -> Self {
        Self {
            samples: Arc::clone(&cached.samples),
            sample_rate: cached.sample_rate,
            channels: cached.channels,
            position: 0,
        }
    }
}

impl Iterator for CachedSampleSource {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        if self.position < self.samples.len() {
            let sample = self.samples[self.position];
            self.position += 1;
            Some(sample)
        } else {
            None
        }
    }
}

impl Source for CachedSampleSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len() - self.position)
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZeroU16::new(self.channels).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZeroU32::new(self.sample_rate).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        let total_samples = self.samples.len() / self.channels as usize;
        Some(std::time::Duration::from_secs_f64(
            total_samples as f64 / self.sample_rate as f64,
        ))
    }
}

pub struct SampleEngine {
    _device: MixerDeviceSink,
    mixer: Mixer,
    pub cache: HashMap<PathBuf, CachedSample>,
    players: Vec<Player>,
    max_voices: usize,
    recording_buffer: Option<Arc<std::sync::Mutex<Vec<f32>>>>,
    recording_sample_rate: u32,
    recording_channels: u16,
    // Per-pad recording buffers (for rack recording)
    pad_recording_buffers: Vec<Option<Arc<std::sync::Mutex<Vec<f32>>>>>,
    // Whether we're doing per-pad recording
    per_pad_recording: bool,
}

impl SampleEngine {
    pub fn new() -> Result<Self, String> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Failed to open audio output: {}", e))?;
        let mixer = device.mixer().clone();
        Ok(Self {
            _device: device,
            mixer,
            cache: HashMap::new(),
            players: Vec::new(),
            max_voices: 16,
            recording_buffer: None,
            recording_sample_rate: 44100,
            recording_channels: 2,
            pad_recording_buffers: vec![None; 16],
            per_pad_recording: false,
        })
    }

    /// Start per-pad recording - records each pad's audio separately
    /// Call this instead of start_recording() for rack recording
    pub fn start_pad_recording(&mut self, sample_rate: u32, channels: u16) {
        self.recording_buffer = None; // No global buffer
        self.recording_sample_rate = sample_rate;
        self.recording_channels = channels;
        self.per_pad_recording = true;
        // Initialize per-pad buffers
        self.pad_recording_buffers = (0..16)
            .map(|_| Some(Arc::new(std::sync::Mutex::new(Vec::new()))))
            .collect();
    }

    /// Stop per-pad recording and return the mixed stereo buffer.
    /// Mixes all non-empty per-pad buffers into a single stereo output.
    pub fn stop_pad_recording(&mut self) -> Option<Vec<f32>> {
        if !self.per_pad_recording {
            return None;
        }
        self.per_pad_recording = false;

        // Extract all per-pad buffers
        let pad_buffers: Vec<Vec<f32>> = self.pad_recording_buffers
            .iter_mut()
            .filter_map(|slot| {
                slot.take().and_then(|arc| {
                    match Arc::try_unwrap(arc) {
                        Ok(mutex) => {
                            let buf = mutex.into_inner().unwrap_or_default();
                            if buf.is_empty() { None } else { Some(buf) }
                        }
                        Err(arc) => {
                            let buf = arc.lock().map(|b| b.clone()).unwrap_or_default();
                            if buf.is_empty() { None } else { Some(buf) }
                        }
                    }
                })
            })
            .collect();

        if pad_buffers.is_empty() {
            return None;
        }

        // Find the longest buffer to determine mix length
        let max_len = pad_buffers.iter().map(|b| b.len()).max().unwrap_or(0);
        if max_len == 0 {
            return None;
        }

        // Mix all pad buffers together (stereo: interleaved L/R)
        let mut mixed = vec![0.0f32; max_len];
        for pad_buf in &pad_buffers {
            for (i, sample) in pad_buf.iter().enumerate() {
                mixed[i] += sample;
            }
        }

        Some(mixed)
    }

    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        self.recording_buffer.is_some()
    }

    #[allow(dead_code)]
    pub fn clear_recording(&mut self) {
        if let Some(ref buf) = self.recording_buffer {
            if let Ok(mut b) = buf.lock() {
                b.clear();
            }
        }
    }

    pub fn preload(&mut self, path: &Path) -> Result<(), String> {
        if !self.cache.contains_key(path) {
            let sample = CachedSample::load(path)?;
            self.cache.insert(path.to_path_buf(), sample);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_cached(&self, path: &Path) -> bool {
        self.cache.contains_key(path)
    }

    pub fn play(&mut self, path: &Path) -> Result<(), String> {
        if !self.cache.contains_key(path) {
            self.preload(path)?;
        }

        self.cleanup();
        self.evict_oldest();

        let cached = self
            .cache
            .get(path)
            .ok_or_else(|| "Sample not in cache".to_string())?;
        let source = CachedSampleSource::new(cached);
        let player = Player::connect_new(&self.mixer);
        self.append_source(&player, source);
        self.players.push(player);
        Ok(())
    }

    pub fn play_with_config(
        &mut self,
        path: &Path,
        config: Option<&crate::state::PadConfig>,
    ) -> Result<(), String> {
        if let Some(cfg) = config {
            if cfg.mute {
                return Ok(());
            }
        }

        if !self.cache.contains_key(path) {
            self.preload(path)?;
        }

        self.cleanup();
        self.evict_oldest();

        let player = Player::connect_new(&self.mixer);

        if let Some(cfg) = config {
            player.set_volume(cfg.volume);

            let cached = self
                .cache
                .get(path)
                .ok_or_else(|| "Sample not in cache".to_string())?;
            let source = CachedSampleSource::new(cached);

            // Apply effects chain
            let src = apply_dsp_chain(source, cfg);
            self.append_source(&player, src);
        } else {
            let cached = self
                .cache
                .get(path)
                .ok_or_else(|| "Sample not in cache".to_string())?;
            let source = CachedSampleSource::new(cached);
            self.append_source(&player, source);
        }

        self.players.push(player);
        Ok(())
    }

    /// Play a sample with DSP effects and per-pad recording support for rack recording
    pub fn play_with_config_and_recording(
        &mut self,
        path: &Path,
        config: Option<&crate::state::PadConfig>,
        pad_idx: usize,
    ) -> Result<(), String> {
        if let Some(cfg) = config {
            if cfg.mute {
                return Ok(());
            }
        }

        if !self.cache.contains_key(path) {
            self.preload(path)?;
        }

        self.cleanup();
        self.evict_oldest();

        let player = Player::connect_new(&self.mixer);

        if let Some(cfg) = config {
            player.set_volume(cfg.volume);

            let cached = self
                .cache
                .get(path)
                .ok_or_else(|| "Sample not in cache".to_string())?;
            let source = CachedSampleSource::new(cached);

            // Apply effects chain
            let src = apply_dsp_chain(source, cfg);
            self.append_source_with_pad_recording(&player, src, pad_idx);
        } else {
            let cached = self
                .cache
                .get(path)
                .ok_or_else(|| "Sample not in cache".to_string())?;
            let source = CachedSampleSource::new(cached);
            self.append_source_with_pad_recording(&player, source, pad_idx);
        }

        self.players.push(player);
        Ok(())
    }

    fn append_source<S: Source<Item = f32> + Send + 'static>(&self, player: &Player, src: S) {
        if let Some(ref recording_buf) = self.recording_buffer {
            player.append(RecordingSource::new(
                src,
                Arc::clone(recording_buf),
                self.recording_sample_rate,
                self.recording_channels,
            ));
        } else {
            player.append(src);
        }
    }

    /// Append source with per-pad recording support
    fn append_source_with_pad_recording<S: Source<Item = f32> + Send + 'static>(
        &self,
        player: &Player,
        src: S,
        pad_idx: usize,
    ) {
        if self.per_pad_recording && pad_idx < 16 {
            if let Some(ref pad_buf) = self.pad_recording_buffers[pad_idx] {
                player.append(RecordingSource::new(
                    src,
                    Arc::clone(pad_buf),
                    self.recording_sample_rate,
                    self.recording_channels,
                ));
            } else {
                player.append(src);
            }
        } else if let Some(ref recording_buf) = self.recording_buffer {
            player.append(RecordingSource::new(
                src,
                Arc::clone(recording_buf),
                self.recording_sample_rate,
                self.recording_channels,
            ));
        } else {
            player.append(src);
        }
    }

    fn evict_oldest(&mut self) {
        if self.players.len() >= self.max_voices {
            if let Some(oldest) = self.players.first() {
                oldest.stop();
            }
            self.players.remove(0);
        }
    }

    pub fn stop_all(&mut self) {
        for player in &self.players {
            player.stop();
        }
        self.players.clear();
    }

    #[allow(dead_code)]
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    #[allow(dead_code)]
    pub fn cache_stats(&self) -> (usize, usize) {
        let count = self.cache.len();
        let bytes: usize = self
            .cache
            .values()
            .map(|s| s.samples.len() * std::mem::size_of::<f32>())
            .sum();
        (count, bytes)
    }

    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    #[allow(dead_code)]
    pub fn cache(&mut self, path: &Path) -> Result<(), String> {
        self.preload(path)
    }

    #[allow(dead_code)]
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    fn cleanup(&mut self) {
        self.players.retain(|p| !p.empty());
    }
}

fn apply_dsp_chain<S: Source<Item = f32> + Send + 'static>(
    src: S,
    cfg: &crate::state::PadConfig,
) -> Box<dyn Source<Item = f32> + Send> {
    // Apply effects in order: HP → LP → EQ → Distortion → Chorus → Reverb

    let mut boxed: Box<dyn Source<Item = f32> + Send> = Box::new(src);

    // High-pass filter
    if cfg.high_pass > 20.1 {
        boxed = Box::new(boxed.high_pass(cfg.high_pass as u32));
    }

    // Low-pass filter
    if cfg.low_pass < 19999.0 {
        boxed = Box::new(boxed.low_pass(cfg.low_pass as u32));
    }

    // EQ bands: L/H use filters when cutting, M stays as simple gain
    if cfg.eq_low < 1.0 {
        let normalized = (1.0 - cfg.eq_low).clamp(0.0, 1.0);
        let freq = 20000.0 * (500.0f32 / 20000.0).powf(normalized);
        boxed = Box::new(boxed.low_pass(freq as u32));
    }
    if (cfg.eq_mid - 1.0).abs() > 0.01 {
        boxed = Box::new(boxed.amplify(cfg.eq_mid));
    }
    if cfg.eq_high < 1.0 {
        let normalized = (1.0 - cfg.eq_high).clamp(0.0, 1.0);
        let freq = 20.0 * (5000.0f32 / 20.0).powf(normalized);
        boxed = Box::new(boxed.high_pass(freq as u32));
    }

    // Distortion (hard-clipping with filtering)
    if cfg.distortion > 0.01 {
        boxed = Box::new(boxed.custom_distortion(cfg.distortion));
    }

    // Chorus (multi-voice with triangular LFO)
    if cfg.chorus > 0.01 {
        let rate = 0.4 + cfg.chorus * 1.6; // 0.4 - 2.0 Hz
        let depth = cfg.chorus; // 0 - 1.0 depth
        let mix = cfg.chorus * 0.4; // 0 - 0.4 wet/dry
        boxed = Box::new(boxed.custom_chorus(rate, depth, mix));
    }

    // Reverb (Freeverb algorithm)
    if cfg.reverb > 0.01 {
        boxed = Box::new(boxed.custom_reverb(cfg.reverb, cfg.reverb, cfg.reverb));
    }

    boxed
}

/// Apply DSP effects to a buffer of samples
/// This applies the same effects as apply_dsp_chain but works on pre-recorded audio.
fn apply_dsp_to_buffer(samples: &[f32], cfg: &crate::state::PadConfig) -> Vec<f32> {
    if samples.is_empty() {
        return samples.to_vec();
    }

    let mut result = samples.to_vec();

    // Apply effects in order: Volume → HP → LP → EQ → Distortion

    // Volume
    let volume = cfg.volume;
    for s in result.iter_mut() {
        *s *= volume;
    }

    // High-pass filter (simple first-order)
    if cfg.high_pass > 20.1 {
        let cutoff = cfg.high_pass as f64;
        let rc = 1.0 / (cutoff * std::f64::consts::TAU);
        let dt = 1.0 / 44100.0;
        let alpha = dt / (rc + dt);
        let mut prev = result[0];
        for s in result.iter_mut() {
            *s = (alpha * (*s as f64) + (1.0 - alpha) * (prev as f64)) as f32;
            prev = *s;
        }
    }

    // Low-pass filter (simple first-order)
    if cfg.low_pass < 19999.0 {
        let cutoff = cfg.low_pass as f64;
        let rc = 1.0 / (cutoff * std::f64::consts::TAU);
        let dt = 1.0 / 44100.0;
        let alpha = dt / (rc + dt);
        let mut prev = result[0];
        for s in result.iter_mut() {
            *s = ((1.0 - alpha) * (*s as f64) + alpha * (prev as f64)) as f32;
            prev = *s;
        }
    }

    // EQ bands: L/H use filters when cutting, M stays as simple gain
    if cfg.eq_low < 1.0 {
        let normalized = (1.0 - cfg.eq_low).clamp(0.0, 1.0);
        let freq = (20000.0 * (500.0f64 / 20000.0).powf(normalized as f64)) as f64;
        let rc = 1.0 / (freq * std::f64::consts::TAU);
        let dt = 1.0 / 44100.0;
        let alpha = dt / (rc + dt);
        let mut prev = result[0];
        for s in result.iter_mut() {
            *s = ((1.0 - alpha) * (*s as f64) + alpha * (prev as f64)) as f32;
            prev = *s;
        }
    }
    if (cfg.eq_mid - 1.0).abs() > 0.01 {
        let gain = cfg.eq_mid;
        for s in result.iter_mut() {
            *s *= gain;
        }
    }
    if cfg.eq_high < 1.0 {
        let normalized = (1.0 - cfg.eq_high).clamp(0.0, 1.0);
        let freq = (20.0 * (5000.0f64 / 20.0).powf(normalized as f64)) as f64;
        let rc = 1.0 / (freq * std::f64::consts::TAU);
        let dt = 1.0 / 44100.0;
        let alpha = dt / (rc + dt);
        let mut prev = result[0];
        for s in result.iter_mut() {
            *s = (alpha * (*s as f64) + (1.0 - alpha) * (prev as f64)) as f32;
            prev = *s;
        }
    }

    // Distortion (soft clipping)
    if cfg.distortion > 0.01 {
        let amount = cfg.distortion * 10.0; // Scale to reasonable range
        for s in result.iter_mut() {
            let x = *s * amount;
            *s = x.tanh() / amount;
        }
    }

    result
}

pub struct RecordingSource<S> {
    inner: S,
    buffer: Arc<std::sync::Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

impl<S> RecordingSource<S> {
    pub fn new(
        inner: S,
        buffer: Arc<std::sync::Mutex<Vec<f32>>>,
        sample_rate: u32,
        channels: u16,
    ) -> Self {
        Self {
            inner,
            buffer,
            sample_rate,
            channels,
        }
    }
}

impl<S: Source<Item = f32>> Iterator for RecordingSource<S> {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().inspect(|&sample| {
            if let Ok(mut buf) = self.buffer.lock() {
                buf.push(sample);
            }
        })
    }
}

impl<S: Source<Item = f32>> Source for RecordingSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZeroU16::new(self.channels).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZeroU32::new(self.sample_rate).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}

pub struct RackPlayer {
    mixer: Mixer,
    players: HashMap<usize, Player>,
    buffers: HashMap<usize, (Vec<f32>, u32, u16)>,
    // Reference to rack state for tempo access
    #[allow(dead_code)]
    rack_state: Option<std::sync::Arc<std::sync::Mutex<crate::state::RackState>>>,
}

/// A recorded trigger with the actual audio sample data
pub struct TriggerWithSample {
    /// Milliseconds from the start of the recording
    pub time_ms: u64,
    /// The recorded audio samples for this trigger
    pub samples: Vec<f32>,
    /// Pad configuration at the time of recording
    pub config: crate::state::PadConfig,
}

impl RackPlayer {
    pub fn new(mixer: Mixer) -> Self {
        Self {
            mixer,
            players: HashMap::new(),
            buffers: HashMap::new(),
            rack_state: None,
        }
    }

    /// Set the rack state reference for tempo access
    #[allow(dead_code)]
    pub fn set_rack_state(
        &mut self,
        rack_state: std::sync::Arc<std::sync::Mutex<crate::state::RackState>>,
    ) {
        self.rack_state = Some(rack_state);
    }

    pub fn set_loop_buffer(
        &mut self,
        rack_idx: usize,
        samples: Vec<f32>,
        sample_rate: u32,
        channels: u16,
    ) {
        self.buffers
            .insert(rack_idx, (samples, sample_rate, channels));
    }

    pub fn play_loop(&mut self, rack_idx: usize, volume: f32, tempo: f32) -> Result<(), String> {
        let (samples, sample_rate, channels) = self
            .buffers
            .get(&rack_idx)
            .ok_or_else(|| format!("No audio buffer for rack {}", rack_idx))?;

        if samples.is_empty() {
            return Err("Empty audio buffer".to_string());
        }

        if let Some(player) = self.players.remove(&rack_idx) {
            player.stop();
        }

        let source = CachedSampleSource {
            samples: Arc::new(samples.clone()),
            sample_rate: *sample_rate,
            channels: *channels,
            position: 0,
        };

        let player = Player::connect_new(&self.mixer);
        player.set_volume(volume);
        // Tempo: 120 BPM is the reference speed (1.0x)
        player.set_speed(tempo / 120.0);
        player.append(source.repeat_infinite());
        self.players.insert(rack_idx, player);

        Ok(())
    }

    pub fn stop_rack(&mut self, rack_idx: usize) {
        if let Some(player) = self.players.remove(&rack_idx) {
            player.stop();
        }
    }

    pub fn delete_rack(&mut self, rack_idx: usize) {
        self.stop_rack(rack_idx);
        self.buffers.remove(&rack_idx);
    }

    pub fn is_playing(&self, rack_idx: usize) -> bool {
        self.players
            .get(&rack_idx)
            .map(|p| !p.empty())
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn stop_all(&mut self) {
        for player in self.players.values() {
            player.stop();
        }
        self.players.clear();
    }

    pub fn clear_all_buffers(&mut self) {
        self.stop_all();
        self.buffers.clear();
    }

    pub fn cleanup(&mut self) {
        self.players.retain(|_, player| !player.empty());
    }

    #[allow(dead_code)]
    pub fn stop(&mut self, rack_idx: usize) {
        self.stop_rack(rack_idx);
    }

    #[allow(dead_code)]
    pub fn play(&mut self, rack_idx: usize) -> Result<(), String> {
        self.play_loop(rack_idx, 0.8, 120.0)
    }

    /// Render a rack using per-pad recordings with DSP applied
    /// This method takes trigger times, per-pad sample data, and pad configs,
    /// applies DSP effects, and creates a loop buffer that respects trigger timing.
    #[allow(dead_code)]
    pub fn render_rack_with_dsp(
        &mut self,
        rack_idx: usize,
        triggers: &Vec<TriggerWithSample>,
        _pad_configs: &[crate::state::PadConfig; 16],
        loop_beats: usize,
    ) -> Result<(), String> {
        if triggers.is_empty() {
            return Err("No triggers to render".to_string());
        }

        // Get tempo from the rack
        let tempo = if let Some(ref rack_state_arc) = self.rack_state {
            match rack_state_arc.lock() {
                Ok(rack_state) => rack_state
                    .racks
                    .get(rack_idx)
                    .map(|r| r.tempo)
                    .unwrap_or(120.0),
                Err(_) => 120.0,
            }
        } else {
            120.0
        };

        // Calculate samples per beat at 44.1kHz
        let sample_rate = 44100;
        let samples_per_beat = (sample_rate as f64 * 60.0 / tempo as f64) as usize;

        // Calculate total samples based on loop_beats
        // We need to loop after loop_beats beats
        let total_samples = loop_beats * samples_per_beat * 2; // Stereo
        let mut buffer = vec![0.0f32; total_samples];

        // Render each trigger
        for trigger in triggers {
            let start_sample = (trigger.time_ms as usize * sample_rate / 1000) * 2;

            // Get sample data
            let samples = &trigger.samples;
            let cfg = &trigger.config;

            // Apply DSP effects to the samples
            let processed_samples = apply_dsp_to_buffer(samples, cfg);

            // Mix into the buffer at the correct position
            for (i, sample) in processed_samples.iter().enumerate() {
                let pos = start_sample + i * 2; // Stereo: 2 samples per frame
                if pos < buffer.len() {
                    // Left channel
                    buffer[pos] += sample;
                    // Right channel (mono-to-stereo)
                    if pos + 1 < buffer.len() {
                        buffer[pos + 1] += sample;
                    }
                }
            }
        }

        // Normalize to prevent clipping
        let max_amplitude = buffer.iter().map(|s| s.abs()).fold(0.0, f32::max);
        if max_amplitude > 0.9 {
            let scale = 0.9 / max_amplitude;
            for s in buffer.iter_mut() {
                *s *= scale;
            }
        }

        self.buffers
            .insert(rack_idx, (buffer, sample_rate as u32, 2));
        Ok(())
    }

    #[allow(dead_code)]
    pub fn render_rack(
        &mut self,
        triggers: &[(u64, usize)],
        pad_samples: &[Option<(Arc<Vec<f32>>, u32, u16)>; 16],
        bpm: f32,
        _pad_config: &[crate::state::PadConfig; 16],
    ) -> Result<(), String> {
        if triggers.is_empty() {
            return Err("No triggers to render".to_string());
        }
        
        let samples_per_beat = (44100.0 * 60.0 / bpm) as usize;

        let total_beats = triggers.iter()
            .map(|(time_ms, _)| *time_ms as usize / (samples_per_beat * 1000 / 44100) + 1)
            .max()
            .unwrap_or(16);

        let total_samples = total_beats * samples_per_beat * 2;
        let mut buffer = vec![0.0f32; total_samples];

        for (time_ms, pad_idx) in triggers {
            if let Some((samples, _sr, _ch)) = pad_samples.get(*pad_idx).and_then(|p| p.as_ref()) {
                let start_sample = (*time_ms as usize * 44100 / 1000) * 2;
                for (i, sample) in samples.iter().enumerate() {
                    let pos = start_sample + i;
                    if pos < buffer.len() {
                        buffer[pos] += sample;
                    }
                }
            }
        }

        self.set_loop_buffer(0, buffer, 44100, 2);

        Ok(())
    }

    #[allow(dead_code)]
    pub fn set_volume(&mut self, rack_idx: usize, volume: f32) {
        if let Some(player) = self.players.get(&rack_idx) {
            player.set_volume(volume);
        }
    }

    pub fn set_tempo(&mut self, rack_idx: usize, tempo: f32) {
        if let Some(player) = self.players.get(&rack_idx) {
            player.set_speed(tempo / 120.0);
        }
    }
}
