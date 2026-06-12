//! Audio source discovery for various audio servers and devices

use std::path::PathBuf;
use std::process::Command;

/// Types of audio sources we can discover
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceType {
    /// MPV player via IPC socket
    Mpv,
    /// SuperCollider (scsynth)
    SuperCollider,
    /// PulseAudio/PipeWire sink input
    PulseAudio,
    /// PipeWire node
    PipeWire,
    /// JACK audio connection
    Jack,
    /// System microphone/input device
    Microphone,
    /// Generic audio file
    AudioFile,
}

impl SourceType {
    pub fn label(&self) -> &'static str {
        match self {
            SourceType::Mpv => "MPV",
            SourceType::SuperCollider => "SuperCollider",
            SourceType::PulseAudio => "PulseAudio",
            SourceType::PipeWire => "PipeWire",
            SourceType::Jack => "JACK",
            SourceType::Microphone => "Microphone",
            SourceType::AudioFile => "Audio File",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SourceType::Mpv => "▶",
            SourceType::SuperCollider => "~",
            SourceType::PulseAudio => "♪",
            SourceType::PipeWire => "⟲",
            SourceType::Jack => "⎇",
            SourceType::Microphone => "🎤",
            SourceType::AudioFile => "♫",
        }
    }
}

/// A discovered audio source
#[derive(Debug, Clone)]
pub struct DiscoveredSource {
    /// Display name
    pub name: String,
    /// Type of source
    pub source_type: SourceType,
    /// Connection identifier (socket path, sink name, etc.)
    pub identifier: String,
    /// Whether it's currently active/playing
    pub active: bool,
    /// Additional metadata
    pub metadata: Option<String>,
}

impl DiscoveredSource {
    pub fn display_name(&self) -> String {
        format!("{} {}", self.source_type.icon(), self.name)
    }
}

/// Audio source discovery manager
pub struct SourceDiscovery {
    /// Cached discovered sources
    sources: Vec<DiscoveredSource>,
    /// Common socket paths to check for MPV
    mpv_socket_paths: Vec<PathBuf>,
}

impl SourceDiscovery {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let mpv_socket_paths = vec![
            PathBuf::from("/tmp/mpv-socket"),
            PathBuf::from("/tmp/mpv.sock"),
            PathBuf::from("/tmp/mpvsocket"),
            PathBuf::from(format!("{home}/.config/mpv/socket")),
            PathBuf::from(format!("{home}/.mpv-socket")),
            // Common patterns
            PathBuf::from("/tmp/mpv-music.sock"),
            PathBuf::from("/tmp/mpv-video.sock"),
            PathBuf::from("/tmp/mpv-1.sock"),
            PathBuf::from("/tmp/mpv-2.sock"),
        ];

        Self {
            sources: Vec::new(),
            mpv_socket_paths,
        }
    }

    /// Add a custom socket path to check
    pub fn add_mpv_socket_path(&mut self, path: impl Into<PathBuf>) {
        self.mpv_socket_paths.push(path.into());
    }

    /// Discover all available audio sources
    pub fn discover_all(&mut self) -> &[DiscoveredSource] {
        self.sources.clear();

        // Discover each type
        self.discover_mpv_sockets();
        self.discover_supercollider();
        self.discover_pulseaudio();
        self.discover_pipewire();
        self.discover_jack();
        self.discover_microphones();

        &self.sources
    }

    /// Get cached sources without re-scanning
    pub fn sources(&self) -> &[DiscoveredSource] {
        &self.sources
    }

    /// Discover MPV IPC sockets
    fn discover_mpv_sockets(&mut self) {
        // Check known paths
        for path in &self.mpv_socket_paths {
            if path.exists() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("MPV")
                    .to_string();

                self.sources.push(DiscoveredSource {
                    name,
                    source_type: SourceType::Mpv,
                    identifier: path.to_string_lossy().to_string(),
                    active: true, // Socket exists, likely active
                    metadata: None,
                });
            }
        }

        // Also scan /tmp for any mpv*.sock patterns
        if let Ok(entries) = std::fs::read_dir("/tmp") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if (name.starts_with("mpv") && name.ends_with(".sock"))
                        || (name.starts_with("mpv") && name.contains("socket"))
                    {
                        // Check if we already have this one
                        let path_str = path.to_string_lossy().to_string();
                        if !self.sources.iter().any(|s| s.identifier == path_str) {
                            self.sources.push(DiscoveredSource {
                                name: name.to_string(),
                                source_type: SourceType::Mpv,
                                identifier: path_str,
                                active: true,
                                metadata: None,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Discover SuperCollider servers
    fn discover_supercollider(&mut self) {
        // Check if scsynth is running
        if let Ok(output) = Command::new("pgrep").arg("-x").arg("scsynth").output() {
            if output.status.success() {
                // scsynth typically listens on UDP 57110
                self.sources.push(DiscoveredSource {
                    name: "SuperCollider (scsynth)".to_string(),
                    source_type: SourceType::SuperCollider,
                    identifier: "udp://127.0.0.1:57110".to_string(),
                    active: true,
                    metadata: Some("Default SC server".to_string()),
                });
            }
        }

        // Check for sclang (might have custom server)
        if let Ok(output) = Command::new("pgrep").arg("-x").arg("sclang").output() {
            if output.status.success() && !self.sources.iter().any(|s| s.source_type == SourceType::SuperCollider) {
                self.sources.push(DiscoveredSource {
                    name: "SuperCollider (sclang)".to_string(),
                    source_type: SourceType::SuperCollider,
                    identifier: "sclang".to_string(),
                    active: true,
                    metadata: Some("SC language running".to_string()),
                });
            }
        }
    }

    /// Discover PulseAudio sink inputs
    fn discover_pulseaudio(&mut self) {
        // Use pactl to list sink inputs
        if let Ok(output) = Command::new("pactl").arg("list").arg("sink-inputs").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                self.parse_pulseaudio_output(&stdout);
            }
        }
    }

    fn parse_pulseaudio_output(&mut self, output: &str) {
        let mut current_name = None;
        let mut current_index = None;
        let mut current_app = None;

        for line in output.lines() {
            let line = line.trim();

            if line.starts_with("Sink Input #") {
                // Save previous if exists
                if let (Some(idx), Some(name)) = (current_index.take(), current_name.take()) {
                    self.sources.push(DiscoveredSource {
                        name,
                        source_type: SourceType::PulseAudio,
                        identifier: format!("sink-input:{}", idx),
                        active: true,
                        metadata: current_app.take(),
                    });
                }

                current_index = line
                    .strip_prefix("Sink Input #")
                    .and_then(|s| s.parse::<u32>().ok());
            } else if line.starts_with("application.name = ") {
                current_app = line
                    .strip_prefix("application.name = ")
                    .map(|s| s.trim_matches('"').to_string());
            } else if line.starts_with("media.name = ") {
                current_name = line
                    .strip_prefix("media.name = ")
                    .map(|s| s.trim_matches('"').to_string());
            }
        }

        // Don't forget the last one
        if let (Some(idx), Some(name)) = (current_index, current_name) {
            self.sources.push(DiscoveredSource {
                name,
                source_type: SourceType::PulseAudio,
                identifier: format!("sink-input:{}", idx),
                active: true,
                metadata: current_app,
            });
        }
    }

    /// Discover PipeWire nodes
    fn discover_pipewire(&mut self) {
        // Use pw-cli to list nodes
        if let Ok(output) = Command::new("pw-cli").arg("list-objects").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                self.parse_pipewire_output(&stdout);
            }
        }
    }

    fn parse_pipewire_output(&mut self, output: &str) {
        // PipeWire output is complex, look for audio streams
        for line in output.lines() {
            if line.contains("type: PipeWire:Interface:Node") 
                || line.contains("media.class = \"Audio/Sink\"")
                || line.contains("media.class = \"Stream/Output/Audio\"")
            {
                // Extract node info - simplified parsing
                if let Some(name_start) = line.find("node.name = ") {
                    let name = line[name_start + 12..]
                        .split(',')
                        .next()
                        .unwrap_or("PipeWire Node")
                        .trim()
                        .trim_matches('"');

                    // Avoid duplicates
                    if !self.sources.iter().any(|s| s.name == name && s.source_type == SourceType::PipeWire) {
                        self.sources.push(DiscoveredSource {
                            name: name.to_string(),
                            source_type: SourceType::PipeWire,
                            identifier: name.to_string(),
                            active: true,
                            metadata: None,
                        });
                    }
                }
            }
        }
    }

    /// Discover JACK connections
    fn discover_jack(&mut self) {
        // Check if JACK is running
        if let Ok(output) = Command::new("jack_lsp").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                self.parse_jack_output(&stdout);
            }
        }
    }

    fn parse_jack_output(&mut self, output: &str) {
        let mut seen_clients = std::collections::HashSet::new();

        for line in output.lines() {
            // JACK ports are formatted as "client:port"
            if let Some(colon_pos) = line.find(':') {
                let client = &line[..colon_pos];

                // Skip system ports
                if client == "system" || seen_clients.contains(client) {
                    continue;
                }

                seen_clients.insert(client.to_string());

                self.sources.push(DiscoveredSource {
                    name: client.to_string(),
                    source_type: SourceType::Jack,
                    identifier: client.to_string(),
                    active: true,
                    metadata: Some("JACK client".to_string()),
                });
            }
        }
    }

    /// Discover microphones/input devices
    fn discover_microphones(&mut self) {
        // Try PulseAudio sources first
        if let Ok(output) = Command::new("pactl").arg("list").arg("sources").arg("short").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 2 {
                        let name = parts[1];
                        // Filter to actual microphones (not monitors)
                        if !name.contains(".monitor") && 
                           (name.contains("input") || name.contains("mic") || name.contains("Mic")) {
                            self.sources.push(DiscoveredSource {
                                name: name.to_string(),
                                source_type: SourceType::Microphone,
                                identifier: name.to_string(),
                                active: true,
                                metadata: None,
                            });
                        }
                    }
                }
            }
        }

        // Fallback: check /proc/asound for ALSA devices (Linux)
        #[cfg(target_os = "linux")]
        if let Ok(entries) = std::fs::read_dir("/proc/asound") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("card") {
                    if let Ok(card_name) = std::fs::read_to_string(entry.path().join("id")) {
                        let card_name = card_name.trim();
                        if !self.sources.iter().any(|s| s.name == card_name && s.source_type == SourceType::Microphone) {
                            self.sources.push(DiscoveredSource {
                                name: card_name.to_string(),
                                source_type: SourceType::Microphone,
                                identifier: format!("alsa:{}", name_str),
                                active: true,
                                metadata: Some("ALSA device".to_string()),
                            });
                        }
                    }
                }
            }
        }

        // macOS: Use system_profiler for audio devices
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = Command::new("system_profiler")
                .arg("SPAudioDataType")
                .arg("-json")
                .output()
            {
                if output.status.success() {
                    // Parse JSON output for input devices
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                        if let Some(audio_data) = json.get("SPAudioDataType").and_then(|v| v.as_array()) {
                            for device in audio_data {
                                if let Some(name) = device.get("_name").and_then(|v| v.as_str()) {
                                    if device.get("coreaudio_input_source").is_some() {
                                        self.sources.push(DiscoveredSource {
                                            name: name.to_string(),
                                            source_type: SourceType::Microphone,
                                            identifier: format!("coreaudio:{}", name),
                                            active: true,
                                            metadata: Some("CoreAudio input".to_string()),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Filter sources by type
    pub fn sources_by_type(&self, source_type: SourceType) -> Vec<&DiscoveredSource> {
        self.sources
            .iter()
            .filter(|s| s.source_type == source_type)
            .collect()
    }

    /// Get sources suitable for mixer input (playback sources)
    pub fn playback_sources(&self) -> Vec<&DiscoveredSource> {
        self.sources
            .iter()
            .filter(|s| matches!(
                s.source_type,
                SourceType::Mpv | SourceType::SuperCollider | SourceType::PulseAudio | SourceType::PipeWire | SourceType::Jack
            ))
            .collect()
    }

    /// Get input sources (microphones)
    pub fn input_sources(&self) -> Vec<&DiscoveredSource> {
        self.sources
            .iter()
            .filter(|s| s.source_type == SourceType::Microphone)
            .collect()
    }
}

impl Default for SourceDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_creation() {
        let discovery = SourceDiscovery::new();
        assert!(discovery.sources.is_empty());
    }

    #[test]
    fn test_source_type_labels() {
        assert_eq!(SourceType::Mpv.label(), "MPV");
        assert_eq!(SourceType::SuperCollider.label(), "SuperCollider");
    }
}
