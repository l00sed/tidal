//! Audio source abstraction for MPV IPC control

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// MPV IPC command structure
#[derive(Debug, Serialize)]
struct MpvCommand {
    command: Vec<serde_json::Value>,
    request_id: u64,
}

/// MPV IPC response
#[derive(Debug, Deserialize)]
struct MpvResponse {
    #[serde(default)]
    data: Option<serde_json::Value>,
    request_id: Option<u64>,
    #[serde(default)]
    error: String,
}

/// Audio source representing an MPV instance
pub struct AudioSource {
    name: String,
    socket_path: PathBuf,
    stream: Option<UnixStream>,
    request_id: u64,
    // Cached state
    volume: f32,
    muted: bool,
    connected: bool,
}

impl AudioSource {
    pub fn new(name: impl Into<String>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            socket_path: socket_path.into(),
            stream: None,
            request_id: 0,
            volume: 100.0,
            muted: false,
            connected: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Connect to the MPV IPC socket
    pub async fn connect(&mut self) -> Result<()> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("Failed to connect to MPV socket: {:?}", self.socket_path))?;

        self.stream = Some(stream);
        self.connected = true;

        // Fetch initial state
        self.sync_state().await?;

        Ok(())
    }

    /// Disconnect from the MPV socket
    pub fn disconnect(&mut self) {
        self.stream = None;
        self.connected = false;
    }

    /// Send a command to MPV and get response
    async fn send_command(&mut self, args: Vec<serde_json::Value>) -> Result<Option<serde_json::Value>> {
        let stream = self.stream.as_mut().context("Not connected to MPV")?;

        self.request_id += 1;
        let cmd = MpvCommand {
            command: args,
            request_id: self.request_id,
        };

        let mut json = serde_json::to_string(&cmd)?;
        json.push('\n');

        let (reader, mut writer) = stream.split();
        writer.write_all(json.as_bytes()).await?;

        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        buf_reader.read_line(&mut response_line).await?;

        let response: MpvResponse = serde_json::from_str(&response_line)?;

        if response.error != "success" && !response.error.is_empty() {
            anyhow::bail!("MPV error: {}", response.error);
        }

        Ok(response.data)
    }

    /// Get a property from MPV
    async fn get_property(&mut self, property: &str) -> Result<serde_json::Value> {
        let result = self
            .send_command(vec![
                "get_property".into(),
                property.into(),
            ])
            .await?;

        result.context("No data returned from get_property")
    }

    /// Set a property in MPV
    async fn set_property(&mut self, property: &str, value: serde_json::Value) -> Result<()> {
        self.send_command(vec![
            "set_property".into(),
            property.into(),
            value,
        ])
        .await?;
        Ok(())
    }

    /// Sync local state with MPV
    pub async fn sync_state(&mut self) -> Result<()> {
        if let Ok(vol) = self.get_property("volume").await {
            if let Some(v) = vol.as_f64() {
                self.volume = v as f32;
            }
        }

        if let Ok(mute) = self.get_property("mute").await {
            if let Some(m) = mute.as_bool() {
                self.muted = m;
            }
        }

        Ok(())
    }

    /// Set volume (0-150)
    pub async fn set_volume(&mut self, volume: f32) -> Result<()> {
        let vol = volume.clamp(0.0, 150.0);
        self.set_property("volume", vol.into()).await?;
        self.volume = vol;
        Ok(())
    }

    /// Set mute state
    pub async fn set_mute(&mut self, muted: bool) -> Result<()> {
        self.set_property("mute", muted.into()).await?;
        self.muted = muted;
        Ok(())
    }

    /// Toggle mute
    pub async fn toggle_mute(&mut self) -> Result<()> {
        self.set_mute(!self.muted).await
    }

    /// Apply an audio filter (af)
    pub async fn set_audio_filter(&mut self, filter: &str) -> Result<()> {
        self.send_command(vec![
            "af".into(),
            "set".into(),
            filter.into(),
        ])
        .await?;
        Ok(())
    }

    /// Apply lowpass filter
    pub async fn set_lowpass(&mut self, frequency: f32) -> Result<()> {
        if frequency >= 20000.0 {
            // Effectively disabled
            self.clear_filter("lowpass").await
        } else {
            let filter = format!("lavfi=[lowpass=f={}]", frequency.clamp(20.0, 20000.0));
            self.set_audio_filter(&filter).await
        }
    }

    /// Apply highpass filter
    pub async fn set_highpass(&mut self, frequency: f32) -> Result<()> {
        if frequency <= 20.0 {
            // Effectively disabled
            self.clear_filter("highpass").await
        } else {
            let filter = format!("lavfi=[highpass=f={}]", frequency.clamp(20.0, 20000.0));
            self.set_audio_filter(&filter).await
        }
    }

    /// Clear a specific filter type
    async fn clear_filter(&mut self, _filter_type: &str) -> Result<()> {
        // Reset all audio filters
        self.send_command(vec!["af".into(), "clr".into(), "".into()])
            .await?;
        Ok(())
    }

    /// Apply combined EQ settings
    pub async fn apply_eq(&mut self, lpf_freq: f32, hpf_freq: f32) -> Result<()> {
        let mut filters = Vec::new();

        if hpf_freq > 20.0 {
            filters.push(format!("highpass=f={}", hpf_freq));
        }
        if lpf_freq < 20000.0 {
            filters.push(format!("lowpass=f={}", lpf_freq));
        }

        if filters.is_empty() {
            self.send_command(vec!["af".into(), "clr".into(), "".into()])
                .await?;
        } else {
            let filter = format!("lavfi=[{}]", filters.join(","));
            self.set_audio_filter(&filter).await?;
        }

        Ok(())
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

    pub fn sources_mut(&mut self) -> &mut [AudioSource] {
        &mut self.sources
    }

    pub fn get(&self, index: usize) -> Option<&AudioSource> {
        self.sources.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut AudioSource> {
        self.sources.get_mut(index)
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Connect all sources
    pub async fn connect_all(&mut self) {
        for source in &mut self.sources {
            if let Err(e) = source.connect().await {
                tracing::warn!("Failed to connect to {}: {}", source.name(), e);
            }
        }
    }
}

impl Default for AudioSourceManager {
    fn default() -> Self {
        Self::new()
    }
}
