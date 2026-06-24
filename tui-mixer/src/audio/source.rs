//! Audio source abstraction for MPV IPC control

/// Audio source representing an MPV instance
pub struct AudioSource {
    name: String,
}

impl AudioSource {
    pub fn new(name: impl Into<String>, _socket_path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Manager for multiple audio sources
pub struct AudioSourceManager {
    sources: Vec<AudioSource>,
}

impl AudioSourceManager {
    pub fn new() -> Self {
        Self { sources: Vec::new() }
    }

    pub fn add_source(&mut self, source: AudioSource) {
        self.sources.push(source);
    }

    pub fn sources(&self) -> &[AudioSource] {
        &self.sources
    }
}

impl Default for AudioSourceManager {
    fn default() -> Self {
        Self::new()
    }
}
