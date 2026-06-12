//! Mixer channel state and controls

use serde::{Deserialize, Serialize};

/// Crossfader curve type for DJ mixing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossfaderCurve {
    /// Linear blend between channels
    #[default]
    Linear,
    /// Smooth S-curve for transitions
    Smooth,
    /// Sharp cut for scratching/beatjuggling
    Cut,
    /// Constant power (equal loudness)
    ConstantPower,
}

impl CrossfaderCurve {
    pub fn label(&self) -> &'static str {
        match self {
            CrossfaderCurve::Linear => "LINEAR",
            CrossfaderCurve::Smooth => "SMOOTH",
            CrossfaderCurve::Cut => "CUT",
            CrossfaderCurve::ConstantPower => "POWER",
        }
    }

    /// Calculate the gain for channel A and B given crossfader position (-1 to 1)
    /// Returns (gain_a, gain_b)
    pub fn calculate_gains(&self, position: f32) -> (f32, f32) {
        let pos = position.clamp(-1.0, 1.0);
        
        match self {
            CrossfaderCurve::Linear => {
                // Simple linear crossfade
                let gain_a = ((1.0 - pos) / 2.0).clamp(0.0, 1.0);
                let gain_b = ((1.0 + pos) / 2.0).clamp(0.0, 1.0);
                (gain_a, gain_b)
            }
            CrossfaderCurve::Smooth => {
                // S-curve using sine
                let t = (pos + 1.0) / 2.0; // 0 to 1
                let angle = t * std::f32::consts::FRAC_PI_2;
                let gain_a = angle.cos();
                let gain_b = angle.sin();
                (gain_a, gain_b)
            }
            CrossfaderCurve::Cut => {
                // Sharp cut - mostly full volume until the edges
                let cut_point = 0.9;
                let gain_a = if pos < -cut_point {
                    1.0
                } else if pos > cut_point {
                    0.0
                } else {
                    let t = (pos + cut_point) / (2.0 * cut_point);
                    1.0 - t
                };
                let gain_b = if pos > cut_point {
                    1.0
                } else if pos < -cut_point {
                    0.0
                } else {
                    let t = (pos + cut_point) / (2.0 * cut_point);
                    t
                };
                (gain_a, gain_b)
            }
            CrossfaderCurve::ConstantPower => {
                // Equal power crossfade - maintains perceived loudness
                let t = (pos + 1.0) / 2.0; // 0 to 1
                let angle = t * std::f32::consts::FRAC_PI_2;
                let gain_a = (std::f32::consts::FRAC_PI_4 * (1.0 - t * 2.0).abs()).cos().sqrt();
                let gain_b = (std::f32::consts::FRAC_PI_4 * (t * 2.0 - 1.0).abs()).cos().sqrt();
                // Actually use proper constant power
                let gain_a = angle.cos().sqrt();
                let gain_b = angle.sin().sqrt();
                (gain_a, gain_b)
            }
        }
    }

    pub fn next(&self) -> Self {
        match self {
            CrossfaderCurve::Linear => CrossfaderCurve::Smooth,
            CrossfaderCurve::Smooth => CrossfaderCurve::Cut,
            CrossfaderCurve::Cut => CrossfaderCurve::ConstantPower,
            CrossfaderCurve::ConstantPower => CrossfaderCurve::Linear,
        }
    }
}

/// DJ section controls
#[derive(Debug, Clone)]
pub struct DjSection {
    /// Crossfader position (-1.0 = full A, 0.0 = center, 1.0 = full B)
    pub crossfader: f32,
    /// Crossfader curve type
    pub crossfader_curve: CrossfaderCurve,
    /// Cue/Master mix (0.0 = full cue, 1.0 = full master)
    pub cue_mix: f32,
    /// Headphone volume
    pub headphone_volume: f32,
    /// Booth/monitor output level
    pub booth_volume: f32,
    /// Which channel is assigned to deck A (usually 0)
    pub deck_a_channel: usize,
    /// Which channel is assigned to deck B (usually 1)
    pub deck_b_channel: usize,
}

impl DjSection {
    pub fn new() -> Self {
        Self {
            crossfader: 0.0, // Center
            crossfader_curve: CrossfaderCurve::Linear,
            cue_mix: 0.5,
            headphone_volume: 0.75,
            booth_volume: 0.75,
            deck_a_channel: 0,
            deck_b_channel: 1,
        }
    }

    /// Get the effective gain for each deck based on crossfader
    pub fn deck_gains(&self) -> (f32, f32) {
        self.crossfader_curve.calculate_gains(self.crossfader)
    }
}

impl Default for DjSection {
    fn default() -> Self {
        Self::new()
    }
}

/// A single mixer channel with all its controls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerChannel {
    /// Channel name/label
    pub name: String,
    /// Channel index (0-based)
    pub index: usize,
    /// Main fader level (0.0 to 1.0, can go to 1.5 for +6dB boost)
    pub fader: f32,
    /// Mute state
    pub muted: bool,
    /// Solo state
    pub solo: bool,
    /// Pan position (-1.0 left, 0.0 center, 1.0 right)
    pub pan: f32,
    /// Low-pass filter frequency (20-20000 Hz)
    pub lpf_freq: f32,
    /// High-pass filter frequency (20-20000 Hz)
    pub hpf_freq: f32,
    /// Low shelf EQ gain (-15 to +15 dB)
    pub eq_low: f32,
    /// Mid EQ gain (-15 to +15 dB)
    pub eq_mid: f32,
    /// High shelf EQ gain (-15 to +15 dB)
    pub eq_high: f32,
    /// EQ Low kill switch (true = cut to -∞)
    #[serde(default)]
    pub eq_low_kill: bool,
    /// EQ Mid kill switch (true = cut to -∞)
    #[serde(default)]
    pub eq_mid_kill: bool,
    /// EQ High kill switch (true = cut to -∞)
    #[serde(default)]
    pub eq_high_kill: bool,
    /// Pre-fader listen (PFL) / Cue
    pub pfl: bool,
    /// Channel is connected to audio source
    pub connected: bool,
    /// Source identifier (socket path, device name, etc.)
    #[serde(default)]
    pub source_id: Option<String>,
    /// Peak meter level (0.0 to 1.0)
    pub peak_level: f32,
    /// RMS level for metering (0.0 to 1.0)
    pub rms_level: f32,
    /// Is the source currently playing audio
    #[serde(default)]
    pub playing: bool,
    /// Playback speed multiplier (1.0 = normal, 0.0 = stopped, 2.0 = 2x speed)
    #[serde(default = "default_playback_speed")]
    pub playback_speed: f32,
}

fn default_playback_speed() -> f32 {
    1.0
}

impl MixerChannel {
    pub fn new(name: impl Into<String>, index: usize) -> Self {
        Self {
            name: name.into(),
            index,
            fader: 0.75, // Unity gain at 75%
            muted: false,
            solo: false,
            pan: 0.0,
            lpf_freq: 20000.0, // Wide open
            hpf_freq: 20.0,    // Wide open
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
            eq_low_kill: false,
            eq_mid_kill: false,
            eq_high_kill: false,
            pfl: false,
            connected: false,
            source_id: None,
            peak_level: 0.0,
            rms_level: 0.0,
            playing: false,
            playback_speed: 1.0,
        }
    }

    /// Convert fader position to dB
    pub fn fader_db(&self) -> f32 {
        if self.fader <= 0.0 {
            f32::NEG_INFINITY
        } else {
            // Logarithmic scale: 0.75 = 0dB, 1.0 = +6dB, 0.0 = -inf
            20.0 * (self.fader / 0.75).log10()
        }
    }

    /// Convert fader to volume percentage (0-150)
    pub fn fader_to_volume(&self) -> f32 {
        (self.fader * 100.0 / 0.75).clamp(0.0, 150.0)
    }

    /// Reset EQ to flat
    pub fn reset_eq(&mut self) {
        self.eq_low = 0.0;
        self.eq_mid = 0.0;
        self.eq_high = 0.0;
    }

    /// Reset filters to wide open
    pub fn reset_filters(&mut self) {
        self.lpf_freq = 20000.0;
        self.hpf_freq = 20.0;
    }

    /// Reset all controls to defaults
    pub fn reset_all(&mut self) {
        self.fader = 0.75;
        self.muted = false;
        self.solo = false;
        self.pan = 0.0;
        self.pfl = false;
        self.reset_eq();
        self.reset_filters();
    }
}

/// Which control is currently selected within a channel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelControl {
    PlayPause,
    Bpm,
    Fader,
    Pan,
    LowPassFilter,
    HighPassFilter,
    EqLow,
    EqLowKill,
    EqMid,
    EqMidKill,
    EqHigh,
    EqHighKill,
    Mute,
    Solo,
    Pfl,
}

impl ChannelControl {
    pub fn all() -> &'static [ChannelControl] {
        &[
            ChannelControl::PlayPause,
            ChannelControl::Bpm,
            ChannelControl::EqHigh,
            ChannelControl::EqHighKill,
            ChannelControl::EqMid,
            ChannelControl::EqMidKill,
            ChannelControl::EqLow,
            ChannelControl::EqLowKill,
            ChannelControl::HighPassFilter,
            ChannelControl::LowPassFilter,
            ChannelControl::Pan,
            ChannelControl::Fader,
            ChannelControl::Mute,
            ChannelControl::Solo,
            ChannelControl::Pfl,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            ChannelControl::PlayPause => "PLAY",
            ChannelControl::Bpm => "BPM",
            ChannelControl::Fader => "FADER",
            ChannelControl::Pan => "PAN",
            ChannelControl::LowPassFilter => "LPF",
            ChannelControl::HighPassFilter => "HPF",
            ChannelControl::EqLow => "LOW",
            ChannelControl::EqLowKill => "L×",
            ChannelControl::EqMid => "MID",
            ChannelControl::EqMidKill => "M×",
            ChannelControl::EqHigh => "HIGH",
            ChannelControl::EqHighKill => "H×",
            ChannelControl::Mute => "MUTE",
            ChannelControl::Solo => "SOLO",
            ChannelControl::Pfl => "PFL",
        }
    }

    /// Is this a continuous control (knob/fader) vs toggle (button)
    pub fn is_continuous(&self) -> bool {
        matches!(
            self,
            ChannelControl::Bpm
                | ChannelControl::Fader
                | ChannelControl::Pan
                | ChannelControl::LowPassFilter
                | ChannelControl::HighPassFilter
                | ChannelControl::EqLow
                | ChannelControl::EqMid
                | ChannelControl::EqHigh
        )
    }

    /// Get the index in the vertical layout (top to bottom)
    /// PlayPause and Bpm are only available for deck channels (A/B)
    /// Kill switches share rows with their EQ controls
    pub fn row_index(&self) -> usize {
        match self {
            ChannelControl::PlayPause => 0,
            ChannelControl::Bpm => 1,
            ChannelControl::EqHigh | ChannelControl::EqHighKill => 2,
            ChannelControl::EqMid | ChannelControl::EqMidKill => 3,
            ChannelControl::EqLow | ChannelControl::EqLowKill => 4,
            ChannelControl::HighPassFilter => 5,
            ChannelControl::LowPassFilter => 6,
            ChannelControl::Pan => 7,
            ChannelControl::Fader => 8,
            ChannelControl::Mute => 9,
            ChannelControl::Solo => 10,
            ChannelControl::Pfl => 11,
        }
    }

    pub fn from_row_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(ChannelControl::PlayPause),
            1 => Some(ChannelControl::Bpm),
            2 => Some(ChannelControl::EqHigh),  // Default to slider, h/l navigates to kill
            3 => Some(ChannelControl::EqMid),
            4 => Some(ChannelControl::EqLow),
            5 => Some(ChannelControl::HighPassFilter),
            6 => Some(ChannelControl::LowPassFilter),
            7 => Some(ChannelControl::Pan),
            8 => Some(ChannelControl::Fader),
            9 => Some(ChannelControl::Mute),
            10 => Some(ChannelControl::Solo),
            11 => Some(ChannelControl::Pfl),
            _ => None,
        }
    }
    
    /// Get row index for non-deck channels (no PlayPause/Bpm)
    pub fn row_index_no_deck(&self) -> usize {
        match self {
            ChannelControl::PlayPause => 0, // Not used, but fallback
            ChannelControl::Bpm => 0,       // Not used, but fallback
            ChannelControl::EqHigh | ChannelControl::EqHighKill => 0,
            ChannelControl::EqMid | ChannelControl::EqMidKill => 1,
            ChannelControl::EqLow | ChannelControl::EqLowKill => 2,
            ChannelControl::HighPassFilter => 3,
            ChannelControl::LowPassFilter => 4,
            ChannelControl::Pan => 5,
            ChannelControl::Fader => 6,
            ChannelControl::Mute => 7,
            ChannelControl::Solo => 8,
            ChannelControl::Pfl => 9,
        }
    }
    
    pub fn from_row_index_no_deck(index: usize) -> Option<Self> {
        match index {
            0 => Some(ChannelControl::EqHigh),
            1 => Some(ChannelControl::EqMid),
            2 => Some(ChannelControl::EqLow),
            3 => Some(ChannelControl::HighPassFilter),
            4 => Some(ChannelControl::LowPassFilter),
            5 => Some(ChannelControl::Pan),
            6 => Some(ChannelControl::Fader),
            7 => Some(ChannelControl::Mute),
            8 => Some(ChannelControl::Solo),
            9 => Some(ChannelControl::Pfl),
            _ => None,
        }
    }
    
    /// Get the paired kill switch for an EQ control
    pub fn eq_kill_pair(&self) -> Option<Self> {
        match self {
            ChannelControl::EqHigh => Some(ChannelControl::EqHighKill),
            ChannelControl::EqMid => Some(ChannelControl::EqMidKill),
            ChannelControl::EqLow => Some(ChannelControl::EqLowKill),
            ChannelControl::EqHighKill => Some(ChannelControl::EqHigh),
            ChannelControl::EqMidKill => Some(ChannelControl::EqMid),
            ChannelControl::EqLowKill => Some(ChannelControl::EqLow),
            _ => None,
        }
    }
    
    /// Is this an EQ kill switch
    pub fn is_eq_kill(&self) -> bool {
        matches!(self, ChannelControl::EqHighKill | ChannelControl::EqMidKill | ChannelControl::EqLowKill)
    }
}

/// Master channel with additional controls
#[derive(Debug, Clone)]
pub struct MasterChannel {
    /// Master fader level
    pub fader: f32,
    /// Master mute
    pub muted: bool,
    /// Dim level (reduce by fixed amount for monitoring)
    pub dim: bool,
    /// Mono fold-down for checking mono compatibility
    pub mono: bool,
    /// Left peak level
    pub peak_left: f32,
    /// Right peak level
    pub peak_right: f32,
}

impl MasterChannel {
    pub fn new() -> Self {
        Self {
            fader: 0.75,
            muted: false,
            dim: false,
            mono: false,
            peak_left: 0.0,
            peak_right: 0.0,
        }
    }

    pub fn fader_db(&self) -> f32 {
        if self.fader <= 0.0 {
            f32::NEG_INFINITY
        } else {
            20.0 * (self.fader / 0.75).log10()
        }
    }
}

impl Default for MasterChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Which global control is selected (DJ section, master)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalControl {
    Crossfader,
    CrossfaderCurve,
    CueMix,
    HeadphoneVolume,
    BoothVolume,
    MasterFader,
    MasterMute,
    MasterDim,
    MasterMono,
}

impl GlobalControl {
    pub fn label(&self) -> &'static str {
        match self {
            GlobalControl::Crossfader => "X-FADER",
            GlobalControl::CrossfaderCurve => "CURVE",
            GlobalControl::CueMix => "CUE MIX",
            GlobalControl::HeadphoneVolume => "PHONES",
            GlobalControl::BoothVolume => "BOOTH",
            GlobalControl::MasterFader => "MASTER",
            GlobalControl::MasterMute => "MUTE",
            GlobalControl::MasterDim => "DIM",
            GlobalControl::MasterMono => "MONO",
        }
    }

    pub fn is_continuous(&self) -> bool {
        matches!(
            self,
            GlobalControl::Crossfader
                | GlobalControl::CueMix
                | GlobalControl::HeadphoneVolume
                | GlobalControl::BoothVolume
                | GlobalControl::MasterFader
        )
    }
}

/// Selection focus - either on a channel or on global controls
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionFocus {
    /// Focus on a channel strip
    Channel(usize),
    /// Focus on DJ/master section
    Global,
}

/// Full mixer state
#[derive(Debug, Clone)]
pub struct MixerState {
    /// Individual channels (up to 2 for this mixer)
    pub channels: Vec<MixerChannel>,
    /// DJ section (crossfader, cue mix, etc.)
    pub dj: DjSection,
    /// Master output
    pub master: MasterChannel,
    /// Selection focus
    pub focus: SelectionFocus,
    /// Currently selected channel index (when focus is Channel)
    pub selected_channel: usize,
    /// Currently selected control within channel
    pub selected_control: ChannelControl,
    /// Currently selected global control (when focus is Global)
    pub selected_global: GlobalControl,
    /// Is any solo active (for solo-in-place logic)
    pub solo_active: bool,
    /// Source selection mode active
    pub source_select_mode: bool,
    /// Currently selected source in dropdown
    pub source_select_index: usize,
}

impl MixerState {
    pub fn new(num_channels: usize) -> Self {
        let channels = (0..num_channels)
            .map(|i| MixerChannel::new(format!("CH {}", i + 1), i))
            .collect();

        Self {
            channels,
            dj: DjSection::new(),
            master: MasterChannel::new(),
            focus: SelectionFocus::Channel(0),
            selected_channel: 0,
            selected_control: ChannelControl::Fader,
            selected_global: GlobalControl::Crossfader,
            solo_active: false,
            source_select_mode: false,
            source_select_index: 0,
        }
    }

    pub fn selected_channel(&self) -> Option<&MixerChannel> {
        self.channels.get(self.selected_channel)
    }

    pub fn selected_channel_mut(&mut self) -> Option<&mut MixerChannel> {
        self.channels.get_mut(self.selected_channel)
    }
    
    /// Check if a channel index is a deck channel (has PlayPause/BPM controls)
    pub fn is_deck_channel(&self, ch_idx: usize) -> bool {
        ch_idx == self.dj.deck_a_channel || ch_idx == self.dj.deck_b_channel
    }

    /// Move selection left
    pub fn select_prev_channel(&mut self) {
        if self.selected_channel > 0 {
            self.selected_channel -= 1;
        }
    }

    /// Move selection right
    pub fn select_next_channel(&mut self) {
        if self.selected_channel < self.channels.len().saturating_sub(1) {
            self.selected_channel += 1;
        }
    }

    /// Move selection up (round-robin)
    pub fn select_prev_control(&mut self) {
        let is_deck = self.is_deck_channel(self.selected_channel);
        let current_row = if is_deck {
            self.selected_control.row_index()
        } else {
            self.selected_control.row_index_no_deck()
        };
        
        let max_row = if is_deck { 11 } else { 9 };
        let new_row = if current_row == 0 { max_row } else { current_row - 1 };
        
        if is_deck {
            if let Some(ctrl) = ChannelControl::from_row_index(new_row) {
                self.selected_control = ctrl;
            }
        } else if let Some(ctrl) = ChannelControl::from_row_index_no_deck(new_row) {
            self.selected_control = ctrl;
        }
    }

    /// Move selection down (round-robin)
    pub fn select_next_control(&mut self) {
        let is_deck = self.is_deck_channel(self.selected_channel);
        let current_row = if is_deck {
            self.selected_control.row_index()
        } else {
            self.selected_control.row_index_no_deck()
        };
        
        let max_row = if is_deck { 11 } else { 9 };
        let new_row = if current_row >= max_row { 0 } else { current_row + 1 };
        
        if is_deck {
            if let Some(ctrl) = ChannelControl::from_row_index(new_row) {
                self.selected_control = ctrl;
            }
        } else if let Some(ctrl) = ChannelControl::from_row_index_no_deck(new_row) {
            self.selected_control = ctrl;
        }
    }

    /// Adjust the currently selected control
    pub fn adjust_selected(&mut self, delta: f32) {
        match self.focus {
            SelectionFocus::Channel(_) => {
                if let Some(channel) = self.channels.get_mut(self.selected_channel) {
                    match self.selected_control {
                        ChannelControl::Fader => {
                            channel.fader = (channel.fader + delta).clamp(0.0, 1.0);
                        }
                        ChannelControl::Bpm => {
                            // BPM adjusts playback speed: 0.5x to 2.0x
                            // Smaller delta for finer control
                            channel.playback_speed = (channel.playback_speed + delta * 0.5).clamp(0.5, 2.0);
                        }
                        ChannelControl::Pan => {
                            channel.pan = (channel.pan + delta).clamp(-1.0, 1.0);
                        }
                        ChannelControl::LowPassFilter => {
                            // Logarithmic adjustment for frequency
                            // Faster adjustment (4x) and wider range
                            let log_freq = channel.lpf_freq.log10();
                            let new_log = (log_freq + delta * 4.0).clamp(2.0, 4.3); // 100-20000 Hz
                            channel.lpf_freq = 10f32.powf(new_log);
                        }
                        ChannelControl::HighPassFilter => {
                            // Faster adjustment (4x) and wider range
                            let log_freq = channel.hpf_freq.log10();
                            let new_log = (log_freq + delta * 4.0).clamp(1.3, 3.7); // 20-5000 Hz
                            channel.hpf_freq = 10f32.powf(new_log);
                        }
                        ChannelControl::EqLow => {
                            // ±24dB range with faster adjustment (6x)
                            channel.eq_low = (channel.eq_low + delta * 6.0).clamp(-24.0, 24.0);
                        }
                        ChannelControl::EqMid => {
                            channel.eq_mid = (channel.eq_mid + delta * 6.0).clamp(-24.0, 24.0);
                        }
                        ChannelControl::EqHigh => {
                            channel.eq_high = (channel.eq_high + delta * 6.0).clamp(-24.0, 24.0);
                        }
                        // Buttons/toggles don't need continuous adjustment
                        ChannelControl::PlayPause 
                        | ChannelControl::Mute 
                        | ChannelControl::Solo 
                        | ChannelControl::Pfl 
                        | ChannelControl::EqLowKill 
                        | ChannelControl::EqMidKill 
                        | ChannelControl::EqHighKill => {}
                    }
                }
            }
            SelectionFocus::Global => {
                match self.selected_global {
                    GlobalControl::Crossfader => {
                        self.dj.crossfader = (self.dj.crossfader + delta).clamp(-1.0, 1.0);
                    }
                    GlobalControl::CueMix => {
                        self.dj.cue_mix = (self.dj.cue_mix + delta).clamp(0.0, 1.0);
                    }
                    GlobalControl::HeadphoneVolume => {
                        self.dj.headphone_volume = (self.dj.headphone_volume + delta).clamp(0.0, 1.0);
                    }
                    GlobalControl::BoothVolume => {
                        self.dj.booth_volume = (self.dj.booth_volume + delta).clamp(0.0, 1.0);
                    }
                    GlobalControl::MasterFader => {
                        self.master.fader = (self.master.fader + delta).clamp(0.0, 1.0);
                    }
                    // Buttons/toggles don't need continuous adjustment
                    GlobalControl::MasterMute | GlobalControl::MasterDim | GlobalControl::MasterMono | GlobalControl::CrossfaderCurve => {}
                }
            }
        }
    }

    /// Toggle the currently selected control (for buttons)
    pub fn toggle_selected(&mut self) {
        if let Some(channel) = self.channels.get_mut(self.selected_channel) {
            match self.selected_control {
                ChannelControl::PlayPause => channel.playing = !channel.playing,
                ChannelControl::Mute => channel.muted = !channel.muted,
                ChannelControl::Solo => {
                    channel.solo = !channel.solo;
                    self.update_solo_state();
                }
                ChannelControl::Pfl => channel.pfl = !channel.pfl,
                ChannelControl::EqLowKill => channel.eq_low_kill = !channel.eq_low_kill,
                ChannelControl::EqMidKill => channel.eq_mid_kill = !channel.eq_mid_kill,
                ChannelControl::EqHighKill => channel.eq_high_kill = !channel.eq_high_kill,
                _ => {}
            }
        }
    }

    fn update_solo_state(&mut self) {
        self.solo_active = self.channels.iter().any(|c| c.solo);
    }

    /// Cycle focus: Deck A → Deck B → Master → Crossfader → Deck A
    pub fn toggle_focus(&mut self) {
        match self.focus {
            SelectionFocus::Channel(ch) => {
                if ch == self.dj.deck_a_channel {
                    // A → B
                    self.focus = SelectionFocus::Channel(self.dj.deck_b_channel);
                    self.selected_channel = self.dj.deck_b_channel;
                } else {
                    // B → Master
                    self.focus = SelectionFocus::Global;
                    self.selected_global = GlobalControl::MasterFader;
                }
            }
            SelectionFocus::Global => {
                match self.selected_global {
                    GlobalControl::MasterFader | GlobalControl::MasterMute | 
                    GlobalControl::MasterDim | GlobalControl::MasterMono => {
                        // Master → Crossfader
                        self.selected_global = GlobalControl::Crossfader;
                    }
                    _ => {
                        // Crossfader/DJ controls → Deck A
                        self.focus = SelectionFocus::Channel(self.dj.deck_a_channel);
                        self.selected_channel = self.dj.deck_a_channel;
                    }
                }
            }
        }
    }

    /// Move to previous global control
    pub fn select_prev_global(&mut self) {
        self.selected_global = match self.selected_global {
            GlobalControl::Crossfader => GlobalControl::MasterMono,
            GlobalControl::CrossfaderCurve => GlobalControl::Crossfader,
            GlobalControl::CueMix => GlobalControl::CrossfaderCurve,
            GlobalControl::HeadphoneVolume => GlobalControl::CueMix,
            GlobalControl::BoothVolume => GlobalControl::HeadphoneVolume,
            GlobalControl::MasterFader => GlobalControl::BoothVolume,
            GlobalControl::MasterMute => GlobalControl::MasterFader,
            GlobalControl::MasterDim => GlobalControl::MasterMute,
            GlobalControl::MasterMono => GlobalControl::MasterDim,
        };
    }

    /// Move to next global control
    pub fn select_next_global(&mut self) {
        self.selected_global = match self.selected_global {
            GlobalControl::Crossfader => GlobalControl::CrossfaderCurve,
            GlobalControl::CrossfaderCurve => GlobalControl::CueMix,
            GlobalControl::CueMix => GlobalControl::HeadphoneVolume,
            GlobalControl::HeadphoneVolume => GlobalControl::BoothVolume,
            GlobalControl::BoothVolume => GlobalControl::MasterFader,
            GlobalControl::MasterFader => GlobalControl::MasterMute,
            GlobalControl::MasterMute => GlobalControl::MasterDim,
            GlobalControl::MasterDim => GlobalControl::MasterMono,
            GlobalControl::MasterMono => GlobalControl::Crossfader,
        };
    }

    /// Adjust the currently selected global control
    pub fn adjust_global(&mut self, delta: f32) {
        match self.selected_global {
            GlobalControl::Crossfader => {
                self.dj.crossfader = (self.dj.crossfader + delta).clamp(-1.0, 1.0);
            }
            GlobalControl::CueMix => {
                self.dj.cue_mix = (self.dj.cue_mix + delta).clamp(0.0, 1.0);
            }
            GlobalControl::HeadphoneVolume => {
                self.dj.headphone_volume = (self.dj.headphone_volume + delta).clamp(0.0, 1.0);
            }
            GlobalControl::BoothVolume => {
                self.dj.booth_volume = (self.dj.booth_volume + delta).clamp(0.0, 1.0);
            }
            GlobalControl::MasterFader => {
                self.master.fader = (self.master.fader + delta).clamp(0.0, 1.0);
            }
            GlobalControl::CrossfaderCurve => {
                self.dj.crossfader_curve = self.dj.crossfader_curve.next();
            }
            GlobalControl::MasterMute => {
                self.master.muted = !self.master.muted;
            }
            GlobalControl::MasterDim => {
                self.master.dim = !self.master.dim;
            }
            GlobalControl::MasterMono => {
                self.master.mono = !self.master.mono;
            }
        }
    }

    /// Update metering levels (simulated for now)
    pub fn update_meters(&mut self) {
        for channel in &mut self.channels {
            // Decay peak
            channel.peak_level *= 0.95;
            channel.rms_level *= 0.9;

            // Simulate some activity if not muted
            if !channel.muted && channel.fader > 0.0 {
                let activity = (rand_simple() * 0.3 + 0.2) * channel.fader;
                channel.rms_level = channel.rms_level.max(activity);
                if rand_simple() > 0.9 {
                    channel.peak_level = channel.rms_level * 1.2;
                }
            }
        }

        // Master meters
        self.master.peak_left *= 0.95;
        self.master.peak_right *= 0.95;

        if !self.master.muted {
            let sum_l: f32 = self
                .channels
                .iter()
                .filter(|c| !c.muted && (!self.solo_active || c.solo))
                .map(|c| c.rms_level * ((1.0 - c.pan) / 2.0 + 0.5))
                .sum();
            let sum_r: f32 = self
                .channels
                .iter()
                .filter(|c| !c.muted && (!self.solo_active || c.solo))
                .map(|c| c.rms_level * ((1.0 + c.pan) / 2.0 + 0.5))
                .sum();

            self.master.peak_left = (sum_l * self.master.fader).min(1.0);
            self.master.peak_right = (sum_r * self.master.fader).min(1.0);
        }
    }
}

impl Default for MixerState {
    fn default() -> Self {
        Self::new(2) // Default to 2 channels
    }
}

/// Simple pseudo-random for meter simulation
fn rand_simple() -> f32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    ((nanos % 1000) as f32) / 1000.0
}
