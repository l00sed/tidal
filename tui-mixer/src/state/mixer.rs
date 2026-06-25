//! Mixer channel state and controls

use serde::{Deserialize, Serialize};

/// DJ section controls
#[derive(Debug, Clone)]
pub struct DjSection {
    /// Crossfader position (-1.0 = full A, 0.0 = center, 1.0 = full B)
    pub crossfader: f32,
    /// Headphone volume
    pub headphone_volume: f32,
    /// Which channel is assigned to deck A (usually 0)
    pub deck_a_channel: usize,
    /// Which channel is assigned to deck B (usually 1)
    pub deck_b_channel: usize,
}

impl DjSection {
    pub fn new() -> Self {
        Self {
            crossfader: 0.0, // Center
            headphone_volume: 0.75,
            deck_a_channel: 0,
            deck_b_channel: 1,
        }
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
    /// Left channel peak level (0.0 to 1.0)
    #[serde(default)]
    pub peak_left: f32,
    /// Right channel peak level (0.0 to 1.0)
    #[serde(default)]
    pub peak_right: f32,
    /// Left channel RMS level (0.0 to 1.0)
    #[serde(default)]
    pub rms_left: f32,
    /// Right channel RMS level (0.0 to 1.0)
    #[serde(default)]
    pub rms_right: f32,
    /// Is the source currently playing audio
    #[serde(default)]
    pub playing: bool,
    /// Playback speed multiplier (1.0 = normal, 0.0 = stopped, 2.0 = 2x speed)
    #[serde(default = "default_playback_speed")]
    pub playback_speed: f32,
    /// Detected BPM of the currently loaded track (None = not yet analyzed)
    #[serde(default)]
    pub bpm: Option<f32>,
}

fn default_playback_speed() -> f32 {
    1.0
}

impl MixerChannel {
    pub fn new(name: impl Into<String>, index: usize) -> Self {
        Self {
            name: name.into(),
            index,
            fader: 0.5, // Unity gain at center
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
            peak_left: 0.0,
            peak_right: 0.0,
            rms_left: 0.0,
            rms_right: 0.0,
            playing: false,
            playback_speed: 1.0,
            bpm: None,
        }
    }

    /// Convert fader position to dB
    pub fn fader_db(&self) -> f32 {
        if self.fader <= 0.0 {
            f32::NEG_INFINITY
        } else {
            // Logarithmic scale: 0.5 = 0dB (center), 1.0 = +6dB, 0.0 = -inf
            20.0 * (self.fader / 0.5).log10()
        }
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
    // CUE-specific controls (Deck C only)
    CueSendToA,
    CueSendToB,
    CueOutputSelect,
}

impl ChannelControl {
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
            ChannelControl::CueSendToA => "-> A",
            ChannelControl::CueSendToB => "-> B",
            ChannelControl::CueOutputSelect => "OUTPUT",
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
    /// CueControl items are rows 12-14 (Deck C only)
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
            ChannelControl::CueSendToA => 11,
            ChannelControl::CueSendToB => 12,
            ChannelControl::CueOutputSelect => 13,
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
            11 => Some(ChannelControl::CueSendToA),
            12 => Some(ChannelControl::CueSendToB),
            13 => Some(ChannelControl::CueOutputSelect),
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
            // CUE controls don't apply to non-deck channels
            ChannelControl::CueSendToA => 9,
            ChannelControl::CueSendToB => 10,
            ChannelControl::CueOutputSelect => 11,
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
}

/// Master channel with additional controls
#[derive(Debug, Clone)]
pub struct MasterChannel {
    /// Master fader level
    pub fader: f32,
    /// Master mute
    pub muted: bool,
    /// Left peak level
    pub peak_left: f32,
    /// Right peak level
    pub peak_right: f32,
    /// Left RMS level (for meter fill)
    pub rms_left: f32,
    /// Right RMS level (for meter fill)
    pub rms_right: f32,
}

impl MasterChannel {
    pub fn new() -> Self {
        Self {
            fader: 0.5,
            muted: false,
            peak_left: 0.0,
            peak_right: 0.0,
            rms_left: 0.0,
            rms_right: 0.0,
        }
    }

    pub fn fader_db(&self) -> f32 {
        if self.fader <= 0.0 {
            f32::NEG_INFINITY
        } else {
            20.0 * (self.fader / 0.5).log10()
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
    HeadphoneVolume,
    MasterFader,
    MasterMute,
    MasterOutputSelect,
}

impl GlobalControl {
    pub fn label(&self) -> &'static str {
        match self {
            GlobalControl::Crossfader => "X-FADER",
            GlobalControl::HeadphoneVolume => "PHONES",
            GlobalControl::MasterFader => "MASTER",
            GlobalControl::MasterMute => "MUTE",
            GlobalControl::MasterOutputSelect => "OUTPUT",
        }
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
    /// CUE channel (headphone preview deck)
    pub cue_channel: MixerChannel,
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
    /// Terminal height for computing fader step size
    pub terminal_height: u16,
}

/// Target deck for sending CUE
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendTarget {
    A,
    B,
}

impl MixerState {
    pub fn new(num_channels: usize) -> Self {
        let channels = (0..num_channels)
            .map(|i| MixerChannel::new(format!("CH {}", i + 1), i))
            .collect();

        let mut cue_channel = MixerChannel::new("CUE", 2);
        cue_channel.pfl = true; // CUE channel is always in PFL mode

        Self {
            channels,
            cue_channel,
            dj: DjSection::new(),
            master: MasterChannel::new(),
            focus: SelectionFocus::Channel(0),
            selected_channel: 0,
            selected_control: ChannelControl::Fader,
            selected_global: GlobalControl::Crossfader,
            solo_active: false,
            terminal_height: 24,
        }
    }

    pub fn selected_channel(&self) -> Option<&MixerChannel> {
        if self.selected_channel == 2 {
            Some(&self.cue_channel)
        } else {
            self.channels.get(self.selected_channel)
        }
    }

    pub fn selected_channel_mut(&mut self) -> Option<&mut MixerChannel> {
        if self.selected_channel == 2 {
            Some(&mut self.cue_channel)
        } else {
            self.channels.get_mut(self.selected_channel)
        }
    }
    
    /// Check if a channel index is a deck channel (has PlayPause/BPM controls)
    pub fn is_deck_channel(&self, ch_idx: usize) -> bool {
        ch_idx == self.dj.deck_a_channel || ch_idx == self.dj.deck_b_channel || ch_idx == 2 // Deck C
    }

    /// Send CUE channel to Deck A or B (transfers all settings, clears CUE)
    pub fn send_cue_to_deck(&mut self, target: SendTarget) {
        let target_idx = match target {
            SendTarget::A => self.dj.deck_a_channel,
            SendTarget::B => self.dj.deck_b_channel,
        };

        if let Some(target_channel) = self.channels.get_mut(target_idx) {
            // Transfer all settings from CUE to target
            target_channel.fader = self.cue_channel.fader;
            target_channel.muted = self.cue_channel.muted;
            target_channel.solo = self.cue_channel.solo;
            target_channel.pan = self.cue_channel.pan;
            target_channel.lpf_freq = self.cue_channel.lpf_freq;
            target_channel.hpf_freq = self.cue_channel.hpf_freq;
            target_channel.eq_low = self.cue_channel.eq_low;
            target_channel.eq_mid = self.cue_channel.eq_mid;
            target_channel.eq_high = self.cue_channel.eq_high;
            target_channel.eq_low_kill = self.cue_channel.eq_low_kill;
            target_channel.eq_mid_kill = self.cue_channel.eq_mid_kill;
            target_channel.eq_high_kill = self.cue_channel.eq_high_kill;
            target_channel.source_id = self.cue_channel.source_id.clone();
            target_channel.connected = self.cue_channel.connected;
            target_channel.playing = self.cue_channel.playing;
            target_channel.playback_speed = self.cue_channel.playback_speed;
        }

        // Clear CUE channel
        self.cue_channel = MixerChannel::new("CUE", 2);
        self.cue_channel.pfl = true;
    }

    /// Move selection up (round-robin)
    /// `is_cue_pane` is true when the Deck C pane is active
    pub fn select_prev_control(&mut self, is_cue_pane: bool) {
        let is_deck = self.is_deck_channel(self.selected_channel);
        let current_row = if is_deck {
            self.selected_control.row_index()
        } else {
            self.selected_control.row_index_no_deck()
        };
        
        // Max row indices: CUE=13 (CueOutputSelect), Deck A/B=10 (Solo), Non-deck=8 (Solo)
        let max_row = if is_cue_pane { 13 } else if is_deck { 10 } else { 8 };
        let new_row = if current_row == 0 { max_row } else { current_row - 1 };
        
        if is_deck || is_cue_pane {
            if let Some(ctrl) = ChannelControl::from_row_index(new_row) {
                self.selected_control = ctrl;
            }
        } else if let Some(ctrl) = ChannelControl::from_row_index_no_deck(new_row) {
            self.selected_control = ctrl;
        }
    }

    /// Move selection down (round-robin)
    /// `is_cue_pane` is true when the Deck C pane is active
    pub fn select_next_control(&mut self, is_cue_pane: bool) {
        let is_deck = self.is_deck_channel(self.selected_channel);
        let current_row = if is_deck {
            self.selected_control.row_index()
        } else {
            self.selected_control.row_index_no_deck()
        };
        
        // Max row indices: CUE=13 (CueOutputSelect), Deck A/B=10 (Solo), Non-deck=8 (Solo)
        let max_row = if is_cue_pane { 13 } else if is_deck { 10 } else { 8 };
        let new_row = if current_row >= max_row { 0 } else { current_row + 1 };
        
        if is_deck || is_cue_pane {
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
            SelectionFocus::Channel(ch_idx) => {
                let channel = if ch_idx == 2 {
                    Some(&mut self.cue_channel)
                } else {
                    self.channels.get_mut(ch_idx)
                };
                
                if let Some(channel) = channel {
                    match self.selected_control {
                        ChannelControl::Fader => {
                            // One row per keypress: fader track = terminal - 28 rows
                            let track_h = self.terminal_height.saturating_sub(28).max(1) as f32;
                            let step = 1.0 / track_h;
                            channel.fader = (channel.fader + delta.signum() * step).clamp(0.0, 1.0);
                        }
                        ChannelControl::Bpm => {
                            channel.playback_speed = (channel.playback_speed + delta * 0.5).clamp(0.5, 2.0);
                        }
                        ChannelControl::Pan => {
                            channel.pan = (channel.pan + delta).clamp(-1.0, 1.0);
                        }
                        ChannelControl::LowPassFilter => {
                            let log_freq = channel.lpf_freq.log10();
                            let new_log = (log_freq + delta * 4.0).clamp(2.0, 4.3);
                            channel.lpf_freq = 10f32.powf(new_log);
                        }
                        ChannelControl::HighPassFilter => {
                            let log_freq = channel.hpf_freq.log10();
                            let new_log = (log_freq + delta * 4.0).clamp(1.3, 3.7);
                            channel.hpf_freq = 10f32.powf(new_log);
                        }
                        ChannelControl::EqLow => {
                            channel.eq_low = (channel.eq_low + delta * 6.0).clamp(-24.0, 24.0);
                        }
                        ChannelControl::EqMid => {
                            channel.eq_mid = (channel.eq_mid + delta * 6.0).clamp(-24.0, 24.0);
                        }
                        ChannelControl::EqHigh => {
                            channel.eq_high = (channel.eq_high + delta * 6.0).clamp(-24.0, 24.0);
                        }
                        _ => {} // Buttons, kills, CUE controls - no continuous adjustment
                    }
                }
            }
            SelectionFocus::Global => {
                match self.selected_global {
                    GlobalControl::Crossfader => {
                        self.dj.crossfader = (self.dj.crossfader + delta).clamp(-1.0, 1.0);
                    }
                    GlobalControl::HeadphoneVolume => {
                        self.dj.headphone_volume = (self.dj.headphone_volume + delta).clamp(0.0, 1.0);
                    }
                    GlobalControl::MasterFader => {
                        let track_h = self.terminal_height.saturating_sub(28).max(1) as f32;
                        let step = 1.0 / track_h;
                        self.master.fader = (self.master.fader + delta.signum() * step).clamp(0.0, 1.0);
                    }
                    // Output selection controls are handled by UI, not continuous
                    GlobalControl::MasterOutputSelect => {}
                    // Buttons/toggles don't need continuous adjustment
                    GlobalControl::MasterMute => {}
                }
            }
        }
    }

    /// Toggle the currently selected control (for buttons)
    pub fn toggle_selected(&mut self) {
        // Use cue_channel for channel 2 (Deck C)
        let channel = if self.selected_channel == 2 {
            Some(&mut self.cue_channel)
        } else {
            self.channels.get_mut(self.selected_channel)
        };
        
        if let Some(channel) = channel {
            match self.selected_control {
                ChannelControl::PlayPause => channel.playing = !channel.playing,
                ChannelControl::Mute => channel.muted = !channel.muted,
                ChannelControl::Solo => {
                    channel.solo = !channel.solo;
                    self.update_solo_state();
                }
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

    /// Update metering levels — reactive to fader, EQ, filters, pan, crossfader, and master gain.
    /// When `real_master` is provided (peak_l, peak_r, rms_l, rms_r), master meters use
    /// the actual audio levels. When `real_channels` provides per-channel levels, those
    /// channels use real metering instead of simulation.
    pub fn update_meters(
        &mut self,
        real_master: Option<(f32, f32, f32, f32)>,
        real_channels: &[(usize, f32, f32, f32, f32)], // (channel_idx, peak_l, peak_r, rms_l, rms_r)
    ) {
        // Precompute crossfader gains (same formula as App::calculate_crossfader_gains)
        let xf = self.dj.crossfader;
        let xf_gain_a = if xf <= 0.0 { 1.0 } else { 1.0 - xf };
        let xf_gain_b = if xf >= 0.0 { 1.0 } else { 1.0 + xf };
        let xf_gain_a = xf_gain_a.clamp(0.0, 1.0);
        let xf_gain_b = xf_gain_b.clamp(0.0, 1.0);

        for (i, channel) in self.channels.iter_mut().enumerate() {
            // Decay peaks (slow hold) and RMS (faster fall)
            channel.peak_left *= 0.92;
            channel.peak_right *= 0.92;
            channel.rms_left *= 0.85;
            channel.rms_right *= 0.85;
            channel.peak_level *= 0.92;
            channel.rms_level *= 0.85;

            // Check if we have real audio levels for this channel
            if let Some(&(_, peak_l, peak_r, rms_l, rms_r)) = real_channels.iter().find(|(idx, _, _, _, _)| *idx == i) {
                // Use real levels from MPV astats filter.
                // Peaks: keep highest (decay already applied above, max with new).
                channel.peak_left = channel.peak_left.max(peak_l);
                channel.peak_right = channel.peak_right.max(peak_r);
                channel.peak_level = channel.peak_left.max(channel.peak_right);
                // RMS tracks smoothly: fast attack, slow release
                let rms_attack = 0.35;
                let rms_release = 0.06;
                channel.rms_left += if rms_l > channel.rms_left {
                    (rms_l - channel.rms_left) * rms_attack
                } else {
                    (rms_l - channel.rms_left) * rms_release
                };
                channel.rms_right += if rms_r > channel.rms_right {
                    (rms_r - channel.rms_right) * rms_attack
                } else {
                    (rms_r - channel.rms_right) * rms_release
                };
                channel.rms_level = (channel.rms_left + channel.rms_right) / 2.0;
                continue;
            }

            // Effective mute: muted OR (solo active and not soloed) OR not playing
            let effective_muted =
                channel.muted || (self.solo_active && !channel.solo) || !channel.playing;

            if effective_muted || channel.fader <= 0.0 {
                continue;
            }

            // --- Compute effective gain for this channel ---
            let fader = channel.fader;

            // EQ gain multiplier (dB to linear, kill = -inf)
            let eq_mult = if channel.eq_low_kill && channel.eq_mid_kill && channel.eq_high_kill {
                0.0
            } else {
                let low = if channel.eq_low_kill { -60.0 } else { channel.eq_low };
                let mid = if channel.eq_mid_kill { -60.0 } else { channel.eq_mid };
                let high = if channel.eq_high_kill { -60.0 } else { channel.eq_high };
                let avg_db = (low + mid + high) / 3.0;
                10f32.powf(avg_db / 20.0)
            };

            // Filter attenuation (simplified — full cutoff = near silence)
            let lpf_factor = if channel.lpf_freq < 200.0 { channel.lpf_freq / 200.0 } else { 1.0 };
            let hpf_factor = if channel.hpf_freq > 5000.0 { 1.0 - (channel.hpf_freq - 5000.0) / 15000.0 } else { 1.0 };
            let filter_mult = lpf_factor * hpf_factor.max(0.05);

            // Crossfader gain
            let xf_gain = if i == self.dj.deck_a_channel {
                xf_gain_a
            } else if i == self.dj.deck_b_channel {
                xf_gain_b
            } else {
                1.0
            };

            // Total effective gain (pre-master)
            let gain = (fader * eq_mult * filter_mult * xf_gain).clamp(0.0, 1.0);

            if gain <= 0.001 {
                continue;
            }

            // --- Pan weighting ---
            // pan = -1.0 → all L, pan = 1.0 → all R, pan = 0.0 → equal
            let pan_l = ((1.0 - channel.pan) * 0.5).max(0.0);
            let pan_r = ((1.0 + channel.pan) * 0.5).max(0.0);

            // Simulated audio activity — responds to fader, EQ, filter, crossfader
            let t = rand_simple();
            let beat = (t * std::f32::consts::TAU).sin().abs(); // smooth rhythmic pulse
            let noise = rand_simple() * 0.2;
            let base = gain * (0.6 + beat * 0.25);
            let activity_l = (base + noise) * pan_l;
            let activity_r = (base + noise) * pan_r;

            // Track RMS toward the current activity level (smooth envelope following)
            let rms_attack = 0.35;
            let rms_release = 0.06;
            channel.rms_left += if activity_l > channel.rms_left {
                (activity_l - channel.rms_left) * rms_attack
            } else {
                (activity_l - channel.rms_left) * rms_release
            };
            channel.rms_right += if activity_r > channel.rms_right {
                (activity_r - channel.rms_right) * rms_attack
            } else {
                (activity_r - channel.rms_right) * rms_release
            };
            channel.rms_level = (channel.rms_left + channel.rms_right) / 2.0;

            // Update peaks — more frequent transients above RMS
            if rand_simple() > 0.7 {
                let peak_l = (activity_l * 1.4).min(1.0);
                let peak_r = (activity_r * 1.4).min(1.0);
                channel.peak_left = channel.peak_left.max(peak_l);
                channel.peak_right = channel.peak_right.max(peak_r);
                channel.peak_level = channel.peak_left.max(channel.peak_right);
            }
        }

        // --- Master meters ---
        if let Some((real_peak_l, real_peak_r, real_rms_l, real_rms_r)) = real_master {
            // Use real audio levels from capture callback.
            // Peaks are accumulated via atomic compare-exchange in the audio callback
            // and decayed in read_meters(), so we just read them directly.
            self.master.peak_left = real_peak_l;
            self.master.peak_right = real_peak_r;
            // RMS tracks smoothly: fast attack, slow release
            let rms_attack = 0.4;
            let rms_release = 0.08;
            self.master.rms_left += if real_rms_l > self.master.rms_left {
                (real_rms_l - self.master.rms_left) * rms_attack
            } else {
                (real_rms_l - self.master.rms_left) * rms_release
            };
            self.master.rms_right += if real_rms_r > self.master.rms_right {
                (real_rms_r - self.master.rms_right) * rms_attack
            } else {
                (real_rms_r - self.master.rms_right) * rms_release
            };
        } else {
            // Simulated master from channel sum
            self.master.peak_left *= 0.92;
            self.master.peak_right *= 0.92;
            self.master.rms_left *= 0.85;
            self.master.rms_right *= 0.85;

            if !self.master.muted {
                let sum_l: f32 = self
                    .channels
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !c.muted && (!self.solo_active || c.solo))
                    .map(|(i, c)| {
                        let xf_g = if i == self.dj.deck_a_channel {
                            xf_gain_a
                        } else if i == self.dj.deck_b_channel {
                            xf_gain_b
                        } else {
                            1.0
                        };
                        c.rms_left * ((1.0 - c.pan) * 0.5 + 0.25) * xf_g
                    })
                    .sum();
                let sum_r: f32 = self
                    .channels
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !c.muted && (!self.solo_active || c.solo))
                    .map(|(i, c)| {
                        let xf_g = if i == self.dj.deck_a_channel {
                            xf_gain_a
                        } else if i == self.dj.deck_b_channel {
                            xf_gain_b
                        } else {
                            1.0
                        };
                        c.rms_right * ((1.0 + c.pan) * 0.5 + 0.25) * xf_g
                    })
                    .sum();

                let master_l = (sum_l * self.master.fader).min(1.0);
                let master_r = (sum_r * self.master.fader).min(1.0);

                self.master.rms_left = self.master.rms_left.max(master_l);
                self.master.rms_right = self.master.rms_right.max(master_r);
                self.master.peak_left = self.master.peak_left.max((master_l * 1.15).min(1.0));
                self.master.peak_right = self.master.peak_right.max((master_r * 1.15).min(1.0));
            }
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
