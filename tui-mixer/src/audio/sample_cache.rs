//! Sample cache for instant playback
//! 
//! Preloads audio samples into memory for zero-latency triggering.

use std::collections::HashMap;
use std::fs::File;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use rodio::mixer::Mixer;
use crate::audio::effects::CustomEffects;

#[derive(Clone)]
pub struct CachedSample {
    pub samples: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl CachedSample {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
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
    cache: HashMap<PathBuf, CachedSample>,
    players: Vec<Player>,
    max_voices: usize,
    recording_buffer: Option<Arc<std::sync::Mutex<Vec<f32>>>>,
    recording_sample_rate: u32,
    recording_channels: u16,
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
        })
    }

    pub fn start_recording(&mut self, sample_rate: u32, channels: u16) {
        self.recording_buffer = Some(Arc::new(std::sync::Mutex::new(Vec::new())));
        self.recording_sample_rate = sample_rate;
        self.recording_channels = channels;
    }

    pub fn stop_recording(&mut self) -> Option<Vec<f32>> {
        self.recording_buffer.take().and_then(|buf| {
            Arc::try_unwrap(buf)
                .ok()
                .and_then(|mutex| mutex.into_inner().ok())
        })
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

        let cached = self.cache.get(path)
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

            let cached = self.cache.get(path)
                .ok_or_else(|| "Sample not in cache".to_string())?;
            let source = CachedSampleSource::new(cached);

            // Apply effects chain
            let src = apply_dsp_chain(source, cfg);
            self.append_source(&player, src);
        } else {
            let cached = self.cache.get(path)
                .ok_or_else(|| "Sample not in cache".to_string())?;
            let source = CachedSampleSource::new(cached);
            self.append_source(&player, source);
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
        let bytes: usize = self.cache.values()
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
    
    // EQ bands (simple gain-based for now)
    if (cfg.eq_low - 1.0).abs() > 0.01 {
        boxed = Box::new(boxed.amplify(cfg.eq_low));
    }
    if (cfg.eq_mid - 1.0).abs() > 0.01 {
        boxed = Box::new(boxed.amplify(cfg.eq_mid));
    }
    if (cfg.eq_high - 1.0).abs() > 0.01 {
        boxed = Box::new(boxed.amplify(cfg.eq_high));
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

pub struct RecordingSource<S> {
    inner: S,
    buffer: Arc<std::sync::Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

impl<S> RecordingSource<S> {
    pub fn new(inner: S, buffer: Arc<std::sync::Mutex<Vec<f32>>>, sample_rate: u32, channels: u16) -> Self {
        Self { inner, buffer, sample_rate, channels }
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
}

impl RackPlayer {
    pub fn new(mixer: Mixer) -> Self {
        Self {
            mixer,
            players: HashMap::new(),
            buffers: HashMap::new(),
        }
    }

    pub fn set_loop_buffer(&mut self, rack_idx: usize, samples: Vec<f32>, sample_rate: u32, channels: u16) {
        self.buffers.insert(rack_idx, (samples, sample_rate, channels));
    }

    pub fn play_loop(&mut self, rack_idx: usize) -> Result<(), String> {
        let (samples, sample_rate, channels) = self.buffers.get(&rack_idx)
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
        self.players.get(&rack_idx).map(|p| !p.empty()).unwrap_or(false)
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
        self.play_loop(rack_idx)
    }

    #[allow(dead_code)]
    pub fn render_rack(
        &mut self,
        triggers: &[(u64, usize)],
        pad_samples: &[Option<(Arc<Vec<f32>>, u32, u16)>; 16],
        bpm: f32,
        _pad_config: &[crate::state::PadConfig; 16],
    ) -> Result<(), String> {
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
}
