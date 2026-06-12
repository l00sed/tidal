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
        }
    }
    
    /// Check if pad is visually active (triggered or playing loop)
    pub fn is_active(&self) -> bool {
        self.triggered || self.playing
    }

    /// Get the key character for this pad
    pub fn key_char(&self) -> char {
        let row = self.index / 4;
        let col = self.index % 4;
        PAD_KEYS[row][col]
    }

    /// Get row and column from index
    pub fn position(&self) -> (usize, usize) {
        (self.index / 4, self.index % 4)
    }

    /// Check if this pad has a sample assigned
    pub fn has_sample(&self) -> bool {
        self.sample_path.is_some()
    }

    /// Get display name (truncated if needed)
    pub fn display_name(&self, max_len: usize) -> String {
        if self.name.len() > max_len {
            format!("{}…", &self.name[..max_len - 1])
        } else {
            self.name.clone()
        }
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
    /// Master pad volume
    pub master_volume: f32,
}

impl SamplePadGrid {
    pub fn new() -> Self {
        let pads = std::array::from_fn(|i| SamplePad::new(i));
        
        Self {
            pads,
            selected_pad: 0,
            active: false,
            config_mode: false,
            master_volume: 1.0,
        }
    }

    /// Get pad by index
    pub fn get(&self, index: usize) -> Option<&SamplePad> {
        self.pads.get(index)
    }

    /// Get mutable pad by index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut SamplePad> {
        self.pads.get_mut(index)
    }

    /// Get pad at row, col position
    pub fn get_at(&self, row: usize, col: usize) -> Option<&SamplePad> {
        if row < 4 && col < 4 {
            Some(&self.pads[row * 4 + col])
        } else {
            None
        }
    }

    /// Get mutable pad at row, col position
    pub fn get_at_mut(&mut self, row: usize, col: usize) -> Option<&mut SamplePad> {
        if row < 4 && col < 4 {
            Some(&mut self.pads[row * 4 + col])
        } else {
            None
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
        if let Some(pad) = self.pads.get_mut(index) {
            match pad.play_mode {
                PlayMode::OneShot => {
                    // Momentary trigger - just flash
                    pad.triggered = true;
                    pad.trigger_frames = 3; // Flash for 3 frames (~150ms at 20fps)
                    // In real implementation, would start playback
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
        if let Some(pad) = self.pads.get_mut(index) {
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

    /// Move selection (round-robin navigation)
    pub fn move_selection(&mut self, delta_row: i32, delta_col: i32) {
        let (row, col) = (self.selected_pad / 4, self.selected_pad % 4);
        // Use modular arithmetic for round-robin
        let new_row = ((row as i32 + delta_row).rem_euclid(4)) as usize;
        let new_col = ((col as i32 + delta_col).rem_euclid(4)) as usize;
        self.selected_pad = new_row * 4 + new_col;
    }

    /// Toggle active state
    pub fn toggle_active(&mut self) {
        self.active = !self.active;
        if !self.active {
            self.config_mode = false;
        }
    }

    /// Toggle configuration mode
    pub fn toggle_config_mode(&mut self) {
        if self.active {
            self.config_mode = !self.config_mode;
        }
    }

    /// Assign sample to selected pad
    pub fn assign_sample(&mut self, path: PathBuf, name: Option<String>) {
        self.assign_sample_to_pad(self.selected_pad, path, name);
    }
    
    /// Assign sample to specific pad by index
    pub fn assign_sample_to_pad(&mut self, index: usize, path: PathBuf, name: Option<String>) {
        if let Some(pad) = self.pads.get_mut(index) {
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

    /// Clear sample from selected pad
    pub fn clear_selected_sample(&mut self) {
        if let Some(pad) = self.pads.get_mut(self.selected_pad) {
            pad.sample_path = None;
            pad.name = format!("Pad {}", pad.index + 1);
            pad.playing = false;
        }
    }

    /// Cycle play mode for selected pad
    pub fn cycle_play_mode(&mut self) {
        if let Some(pad) = self.pads.get_mut(self.selected_pad) {
            pad.play_mode = pad.play_mode.next();
        }
    }

    /// Set pad color
    pub fn set_pad_color(&mut self, index: usize, r: u8, g: u8, b: u8) {
        if let Some(pad) = self.pads.get_mut(index) {
            pad.color = (r, g, b);
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
        assert_eq!(pad.position(), (1, 1)); // Row 1, Col 1
        assert_eq!(pad.key_char(), 't');
    }
}
