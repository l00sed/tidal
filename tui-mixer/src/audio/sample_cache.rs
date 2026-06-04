//! Sample cache for instant playback
//! 
//! Preloads audio samples into memory for zero-latency triggering.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

/// Cached audio sample data
#[derive(Clone)]
pub struct CachedSample {
    /// Raw audio samples (interleaved if stereo)
    pub samples: Arc<Vec<f32>>,
    /// Sample rate
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
}

impl CachedSample {
    /// Load and decode an audio file into memory
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
        
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| format!("Failed to decode {}: {}", path.display(), e))?;
        
        let sample_rate = decoder.sample_rate();
        let channels = decoder.channels();
        
        // Convert to f32 samples
        let samples: Vec<f32> = decoder
            .convert_samples::<f32>()
            .collect();
        
        Ok(Self {
            samples: Arc::new(samples),
            sample_rate,
            channels,
        })
    }
}

/// A source that plays from cached samples
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
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.samples.len() - self.position)
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        let total_samples = self.samples.len() / self.channels as usize;
        Some(std::time::Duration::from_secs_f64(
            total_samples as f64 / self.sample_rate as f64
        ))
    }
}

/// Sample playback engine with caching
pub struct SampleEngine {
    /// Audio output stream (must be kept alive)
    _stream: OutputStream,
    /// Stream handle for creating sinks
    stream_handle: OutputStreamHandle,
    /// Cached samples by path
    cache: HashMap<PathBuf, CachedSample>,
    /// Active sinks for polyphonic playback
    sinks: Vec<Sink>,
    /// Maximum concurrent voices
    max_voices: usize,
}

impl SampleEngine {
    /// Create a new sample engine
    pub fn new() -> Result<Self, String> {
        let (stream, stream_handle) = OutputStream::try_default()
            .map_err(|e| format!("Failed to open audio output: {}", e))?;
        
        Ok(Self {
            _stream: stream,
            stream_handle,
            cache: HashMap::new(),
            sinks: Vec::new(),
            max_voices: 16,
        })
    }
    
    /// Preload a sample into cache
    pub fn preload(&mut self, path: &Path) -> Result<(), String> {
        if !self.cache.contains_key(path) {
            let sample = CachedSample::load(path)?;
            self.cache.insert(path.to_path_buf(), sample);
        }
        Ok(())
    }
    
    /// Check if a sample is cached
    pub fn is_cached(&self, path: &Path) -> bool {
        self.cache.contains_key(path)
    }
    
    /// Play a sample (loads into cache if not already cached)
    pub fn play(&mut self, path: &Path) -> Result<(), String> {
        // Load if not cached
        if !self.cache.contains_key(path) {
            self.preload(path)?;
        }
        
        // Get cached sample
        let cached = self.cache.get(path)
            .ok_or_else(|| "Sample not in cache".to_string())?;
        
        // Clean up finished sinks
        self.sinks.retain(|sink| !sink.empty());
        
        // Limit voices
        if self.sinks.len() >= self.max_voices {
            // Stop oldest sink
            if let Some(oldest) = self.sinks.first() {
                oldest.stop();
            }
            self.sinks.remove(0);
        }
        
        // Create new sink and play
        let sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| format!("Failed to create sink: {}", e))?;
        
        let source = CachedSampleSource::new(cached);
        sink.append(source);
        
        self.sinks.push(sink);
        
        Ok(())
    }
    
    /// Stop all playing samples
    pub fn stop_all(&mut self) {
        for sink in &self.sinks {
            sink.stop();
        }
        self.sinks.clear();
    }
    
    /// Clear the cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
    
    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let count = self.cache.len();
        let bytes: usize = self.cache.values()
            .map(|s| s.samples.len() * std::mem::size_of::<f32>())
            .sum();
        (count, bytes)
    }
}
