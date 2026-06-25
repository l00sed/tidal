//! Sample pad state and configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Key mapping for the 4x4 pad grid
/// Layout:
///   4 5 6 7
///   R T Y U
///   F G H J
///   V B N M
pub const PAD_KEYS: [[char; 4]; 4] = [
    ['4', '5', '6', '7'],
    ['r', 't', 'y', 'u'],
    ['f', 'g', 'h', 'j'],
    ['v', 'b', 'n', 'm'],
];

/// Per-pad DSP configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PadConfig {
    /// Volume level (0.0 to 1.0)
    pub volume: f32,
    /// Mute toggle
    pub mute: bool,
    /// High-pass filter cutoff (20.0 to 20000.0 Hz)
    pub high_pass: f32,
    /// Low-pass filter cutoff (20.0 to 20000.0 Hz)
    pub low_pass: f32,
    /// EQ low band gain (0.0 to 2.0, 1.0 = flat)
    pub eq_low: f32,
    /// EQ mid band gain (0.0 to 2.0, 1.0 = flat)
    pub eq_mid: f32,
    /// EQ high band gain (0.0 to 2.0, 1.0 = flat)
    pub eq_high: f32,
    /// Reverb send amount (0.0 to 1.0)
    pub reverb: f32,
    /// Chorus depth (0.0 to 1.0)
    pub chorus: f32,
    /// Distortion amount (0.0 to 1.0)
    pub distortion: f32,
}

impl Default for PadConfig {
    fn default() -> Self {
        Self {
            volume: 0.5,
            mute: false,
            high_pass: 20.0,
            low_pass: 20000.0,
            eq_low: 1.0,
            eq_mid: 1.0,
            eq_high: 1.0,
            reverb: 0.0,
            chorus: 0.0,
            distortion: 0.0,
        }
    }
}

/// Controls available in the pad config pane
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadControl {
    Sample,
    PlayMode,
    Volume,
    Mute,
    HighPass,
    LowPass,
    EqLow,
    EqMid,
    EqHigh,
    FiltersHeader,
    Reverb,
    Chorus,
    Distortion,
}

impl PadControl {
    /// All controls in navigation order
    pub fn all() -> &'static [PadControl] {
        &[
            PadControl::Sample,
            PadControl::PlayMode,
            PadControl::Volume,
            PadControl::Mute,
            PadControl::HighPass,
            PadControl::LowPass,
            PadControl::EqLow,
            PadControl::EqMid,
            PadControl::EqHigh,
            PadControl::FiltersHeader,
            PadControl::Reverb,
            PadControl::Chorus,
            PadControl::Distortion,
        ]
    }

    /// Whether this control is continuous (adjustable with hjkl)
    pub fn is_continuous(&self) -> bool {
        matches!(
            self,
            PadControl::Volume
                | PadControl::HighPass
                | PadControl::LowPass
                | PadControl::EqLow
                | PadControl::EqMid
                | PadControl::EqHigh
                | PadControl::Reverb
                | PadControl::Chorus
                | PadControl::Distortion
        )
    }

    /// Whether this control is a toggle
    pub fn is_toggle(&self) -> bool {
        matches!(self, PadControl::Mute)
    }

    /// Label for display
    pub fn label(&self) -> &'static str {
        match self {
            PadControl::Sample => "Sample",
            PadControl::PlayMode => "PlayMode",
            PadControl::Volume => "Volume",
            PadControl::Mute => "Mute",
            PadControl::HighPass => "High Pass",
            PadControl::LowPass => "Low Pass",
            PadControl::EqLow => "EQ Low",
            PadControl::EqMid => "EQ Mid",
            PadControl::EqHigh => "EQ High",
            PadControl::FiltersHeader => "Filters",
            PadControl::Reverb => "Reverb",
            PadControl::Chorus => "Chorus",
            PadControl::Distortion => "Distort",
        }
    }
}

/// A single sample pad
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplePad {
    /// Display name for the pad
    pub name: String,
    /// Path to the sample file
    pub sample_path: Option<PathBuf>,
    /// Color for the pad (RGB)
    pub color: (u8, u8, u8),
    /// Volume level (0.0 to 1.0)
    pub volume: f32,
    /// Is currently playing (for loops/toggle modes)
    #[serde(skip)]
    pub playing: bool,
    /// Was just triggered (for visual flash, decays after 1 frame)
    #[serde(skip)]
    pub triggered: bool,
    /// Trigger decay counter (frames until triggered clears)
    #[serde(skip)]
    pub trigger_frames: u8,
    /// Play mode
    pub play_mode: PlayMode,
    /// Pad index (0-15)
    pub index: usize,
    /// Per-pad DSP configuration
    pub config: PadConfig,
}

/// How the sample plays when triggered
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayMode {
    /// Play once and stop
    #[default]
    OneShot,
    /// Hold to play, release to stop
    Gate,
    /// Toggle play/stop
    Toggle,
    /// Loop continuously
    Loop,
}

impl PlayMode {
    pub fn label(&self) -> &'static str {
        match self {
            PlayMode::OneShot => "ONE",
            PlayMode::Gate => "GATE",
            PlayMode::Toggle => "TOG",
            PlayMode::Loop => "LOOP",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            PlayMode::OneShot => PlayMode::Gate,
            PlayMode::Gate => PlayMode::Toggle,
            PlayMode::Toggle => PlayMode::Loop,
            PlayMode::Loop => PlayMode::OneShot,
        }
    }
}

impl SamplePad {
    pub fn new(index: usize) -> Self {
        // Default colors based on row (like Launchpad)
        let color = match index / 4 {
            0 => (255, 100, 100), // Row 1: Red-ish
            1 => (100, 255, 100), // Row 2: Green-ish
            2 => (100, 100, 255), // Row 3: Blue-ish
            _ => (255, 255, 100), // Row 4: Yellow-ish
        };

        Self {
            name: format!("Pad {}", index + 1),
            sample_path: None,
            color,
            volume: 1.0,
            playing: false,
            triggered: false,
            trigger_frames: 0,
            play_mode: PlayMode::OneShot,
            index,
            config: PadConfig::default(),
        }
    }
    
    /// Get the key character for this pad
    pub fn key_char(&self) -> char {
        let row = self.index / 4;
        let col = self.index % 4;
        PAD_KEYS[row][col]
    }

    /// Check if this pad has a sample assigned
    pub fn has_sample(&self) -> bool {
        self.sample_path.is_some()
    }
}

/// The 4x4 sample pad grid
#[derive(Debug, Clone)]
pub struct SamplePadGrid {
    /// 16 pads in a 4x4 grid
    pub pads: [SamplePad; 16],
    /// Currently selected pad for editing
    pub selected_pad: usize,
    /// Is pad mode active (keys trigger pads instead of normal functions)
    pub active: bool,
    /// Is configuration mode active
    pub config_mode: bool,
    /// Currently selected control in config pane
    pub selected_control: PadControl,
    /// Is a config control being edited (level 3)
    pub editing_control: bool,
}

impl SamplePadGrid {
    pub fn new() -> Self {
        let pads = std::array::from_fn(SamplePad::new);
        
        Self {
            pads,
            selected_pad: 0,
            active: false,
            config_mode: false,
            selected_control: PadControl::Sample,
            editing_control: false,
        }
    }

    /// Find pad index by key character
    pub fn pad_index_for_key(&self, key: char) -> Option<usize> {
        let key_lower = key.to_ascii_lowercase();
        for (row_idx, row) in PAD_KEYS.iter().enumerate() {
            for (col_idx, &k) in row.iter().enumerate() {
                if k == key_lower {
                    return Some(row_idx * 4 + col_idx);
                }
            }
        }
        None
    }

    /// Trigger a pad by key
    pub fn trigger_by_key(&mut self, key: char) -> Option<usize> {
        if let Some(index) = self.pad_index_for_key(key) {
            self.trigger_pad(index);
            Some(index)
        } else {
            None
        }
    }

    /// Trigger a pad (momentary flash for one-shot, toggle for loop/toggle modes)
    pub fn trigger_pad(&mut self, index: usize) {
        if index < self.pads.len() {
            let pad = &mut self.pads[index];
            match pad.play_mode {
                PlayMode::OneShot => {
                    pad.triggered = true;
                    pad.trigger_frames = 3;
                }
                PlayMode::Gate => {
                    pad.triggered = true;
                    pad.playing = true;
                }
                PlayMode::Toggle => {
                    pad.playing = !pad.playing;
                    if pad.playing {
                        pad.triggered = true;
                        pad.trigger_frames = 3;
                    }
                }
                PlayMode::Loop => {
                    pad.playing = !pad.playing;
                    if pad.playing {
                        pad.triggered = true;
                        pad.trigger_frames = 3;
                    }
                }
            }
        }
    }

    /// Release a pad (for Gate mode)
    pub fn release_pad(&mut self, index: usize) {
        if index < self.pads.len() {
            let pad = &mut self.pads[index];
            if pad.play_mode == PlayMode::Gate {
                pad.playing = false;
                pad.triggered = false;
            }
        }
    }

    /// Stop all pads
    pub fn stop_all(&mut self) {
        for pad in &mut self.pads {
            pad.playing = false;
            pad.triggered = false;
            pad.trigger_frames = 0;
        }
    }

    /// Assign sample to specific pad by index
    pub fn assign_sample_to_pad(&mut self, index: usize, path: PathBuf, name: Option<String>) {
        if index < self.pads.len() {
            let pad = &mut self.pads[index];
            pad.sample_path = Some(path.clone());
            if let Some(n) = name {
                pad.name = n;
            } else {
                // Use filename as name
                pad.name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Sample")
                    .to_string();
            }
        }
    }

    /// Cycle play mode for selected pad
    pub fn cycle_play_mode(&mut self) {
        if let Some(pad) = self.pads.get_mut(self.selected_pad) {
            pad.play_mode = pad.play_mode.next();
        }
    }

    /// Move config control selection up
    pub fn config_control_up(&mut self) {
        let controls = PadControl::all();
        if let Some(pos) = controls.iter().position(|c| *c == self.selected_control) {
            if pos > 0 {
                let mut new_pos = pos - 1;
                // Skip over FiltersHeader (non-interactive)
                while new_pos > 0 && controls[new_pos] == PadControl::FiltersHeader {
                    new_pos -= 1;
                }
                self.selected_control = controls[new_pos];
            }
        }
    }

    /// Move config control selection down
    pub fn config_control_down(&mut self) {
        let controls = PadControl::all();
        if let Some(pos) = controls.iter().position(|c| *c == self.selected_control) {
            if pos + 1 < controls.len() {
                let mut new_pos = pos + 1;
                // Skip over FiltersHeader (non-interactive)
                while new_pos < controls.len() - 1 && controls[new_pos] == PadControl::FiltersHeader {
                    new_pos += 1;
                }
                self.selected_control = controls[new_pos];
            }
        }
    }

    /// Adjust the currently selected config control by delta
    pub fn adjust_selected_config(&mut self, delta: f32) {
        if let Some(pad) = self.pads.get_mut(self.selected_pad) {
            match self.selected_control {
                PadControl::Volume => {
                    pad.config.volume = (pad.config.volume + delta).clamp(0.0, 2.0);
                }
                PadControl::HighPass => {
                    // Multiplicative: ~10% per fine step, ~40% per coarse step
                    let factor = 1.0 + delta * 2.0;
                    pad.config.high_pass = (pad.config.high_pass * factor).clamp(20.0, 20000.0);
                }
                PadControl::LowPass => {
                    let factor = 1.0 + delta * 2.0;
                    pad.config.low_pass = (pad.config.low_pass * factor).clamp(20.0, 20000.0);
                }
                PadControl::EqLow => {
                    pad.config.eq_low = (pad.config.eq_low + delta).clamp(0.0, 2.0);
                }
                PadControl::EqMid => {
                    pad.config.eq_mid = (pad.config.eq_mid + delta).clamp(0.0, 2.0);
                }
                PadControl::EqHigh => {
                    pad.config.eq_high = (pad.config.eq_high + delta).clamp(0.0, 2.0);
                }
                PadControl::Reverb => {
                    pad.config.reverb = (pad.config.reverb + delta).clamp(0.0, 1.0);
                }
                PadControl::Chorus => {
                    pad.config.chorus = (pad.config.chorus + delta).clamp(0.0, 1.0);
                }
                PadControl::Distortion => {
                    pad.config.distortion = (pad.config.distortion + delta).clamp(0.0, 1.0);
                }
                _ => {}
            }
        }
    }

    /// Toggle the currently selected config control (for Mute, EffectsHeader)
    pub fn toggle_selected_config(&mut self) {
        if let Some(pad) = self.pads.get_mut(self.selected_pad) {
            match self.selected_control {
                PadControl::Mute => {
                    pad.config.mute = !pad.config.mute;
                }
                PadControl::FiltersHeader => {
                    // Header is not toggleable, do nothing
                }
                _ => {}
            }
        }
    }

    /// Reset the currently selected config control to default
    pub fn reset_selected_config(&mut self) {
        if let Some(pad) = self.pads.get_mut(self.selected_pad) {
            match self.selected_control {
                PadControl::Volume => pad.config.volume = 0.5,
                PadControl::Mute => pad.config.mute = false,
                PadControl::HighPass => pad.config.high_pass = 20.0,
                PadControl::LowPass => pad.config.low_pass = 20000.0,
                PadControl::EqLow => pad.config.eq_low = 1.0,
                PadControl::EqMid => pad.config.eq_mid = 1.0,
                PadControl::EqHigh => pad.config.eq_high = 1.0,
                PadControl::Reverb => pad.config.reverb = 0.0,
                PadControl::Chorus => pad.config.chorus = 0.0,
                PadControl::Distortion => pad.config.distortion = 0.0,
                _ => {}
            }
        }
    }

    /// Update playing states (decay triggers)
    pub fn update(&mut self) {
        for pad in &mut self.pads {
            // Decay trigger flash
            if pad.triggered && pad.trigger_frames > 0 {
                pad.trigger_frames -= 1;
                if pad.trigger_frames == 0 {
                    pad.triggered = false;
                }
            }
            
            // For one-shot, clear playing after trigger decay
            if pad.play_mode == PlayMode::OneShot && !pad.triggered {
                pad.playing = false;
            }
        }
    }
}

impl Default for SamplePadGrid {
    fn default() -> Self {
        Self::new()
    }
}

/// A recorded pad trigger event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RackTrigger {
    /// Milliseconds from the start of the recording
    pub time_ms: u64,
    /// Which pad was triggered
    pub pad_idx: usize,
}

/// Recording/playback mode for racks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RackMode {
    /// Not recording or playing
    Idle,
    /// Count-in before recording starts (1, 2... slow, 1, 2, 3, 4 fast, steady)
    CountIn {
        /// Current count step (0-based)
        step: u8,
        /// Frame counter for animation timing
        frame: u8,
    },
    /// Actively recording pad triggers
    Recording,
}

/// A rack: a recorded sequence of pad triggers that loops as a separate audio layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rack {
    /// Display name
    pub name: String,
    /// Recorded trigger events
    pub triggers: Vec<RackTrigger>,
    /// Volume level (0.0 to 1.0)
    pub volume: f32,
    /// Mute toggle
    pub mute: bool,
    /// Tempo in BPM (controls playback speed)
    pub tempo: f32,
    /// Whether currently playing
    #[serde(skip)]
    pub playing: bool,
}

impl Rack {
    pub fn new(index: usize) -> Self {
        Self {
            name: format!("Loop {}", index + 1),
            triggers: Vec::new(),
            volume: 0.8,
            mute: false,
            tempo: 120.0,
            playing: false,
        }
    }
}

/// State for rack management
#[derive(Debug, Clone)]
pub struct RackState {
    /// All racks
    pub racks: Vec<Rack>,
    /// Currently selected rack index (None = '+' button selected)
    pub selected_rack: Option<usize>,
    /// Current recording/playback mode
    pub mode: RackMode,
    /// Timestamp when recording started (in ms since program start)
    pub recording_start_ms: u64,
}

impl RackState {
    pub fn new() -> Self {
        Self {
            racks: Vec::new(),
            selected_rack: None,
            mode: RackMode::Idle,
            recording_start_ms: 0,
        }
    }

    /// Add a new rack and select it
    pub fn add_rack(&mut self) -> usize {
        let idx = self.racks.len();
        self.racks.push(Rack::new(idx));
        self.selected_rack = Some(idx);
        idx
    }

    /// Remove a rack by index
    pub fn remove_rack(&mut self, idx: usize) {
        if idx < self.racks.len() {
            self.racks.remove(idx);
            // Adjust selection
            if self.racks.is_empty() {
                self.selected_rack = None;
            } else if let Some(sel) = self.selected_rack {
                if sel >= self.racks.len() {
                    self.selected_rack = Some(self.racks.len() - 1);
                }
            }
        }
    }

    /// Move selection up (round-robin)
    pub fn select_up(&mut self) {
        if self.racks.is_empty() { return; }
        match self.selected_rack {
            Some(0) => self.selected_rack = Some(self.racks.len() - 1), // wrap to last
            Some(i) => self.selected_rack = Some(i - 1),
            None => self.selected_rack = Some(self.racks.len() - 1),
        }
    }

    /// Move selection down (round-robin)
    pub fn select_down(&mut self) {
        if self.racks.is_empty() { return; }
        match self.selected_rack {
            Some(i) if i + 1 < self.racks.len() => self.selected_rack = Some(i + 1),
            Some(_) => self.selected_rack = Some(0), // wrap to first
            None => self.selected_rack = Some(0),
        }
    }
}

impl Default for RackState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_keys() {
        let grid = SamplePadGrid::new();
        
        assert_eq!(grid.pad_index_for_key('4'), Some(0));
        assert_eq!(grid.pad_index_for_key('7'), Some(3));
        assert_eq!(grid.pad_index_for_key('r'), Some(4));
        assert_eq!(grid.pad_index_for_key('R'), Some(4)); // Case insensitive
        assert_eq!(grid.pad_index_for_key('m'), Some(15));
        assert_eq!(grid.pad_index_for_key('z'), None);
    }

    #[test]
    fn test_pad_position() {
        let pad = SamplePad::new(5);
        assert_eq!(pad.key_char(), 't');
    }
}
