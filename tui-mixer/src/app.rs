//! Application state and event handling

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{AudioCapture, AudioSource, AudioSourceManager, AudioOutput, BpmAnalyzer, MpvClient, RackPlayer, SampleEngine, SuperColliderClient};
use crate::state::{ChannelControl, GlobalControl, MixerState, PadControl, RackState, SamplePadGrid, SendTarget, SelectionFocus};

/// Which deck is being configured
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deck {
    A,
    B,
    C,
}

/// Application mode - 3-level navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Mode 1: Navigate between panes (Deck A, DJ Center, Deck B, Master)
    PaneSelect,
    /// Mode 2: Navigate controls within selected pane
    ControlSelect,
    /// Mode 3: Edit selected control value
    Edit,
    /// Help overlay
    Help,
    /// Pad keys active, trigger samples
    SamplePads,
    /// Configure pads (assign samples)
    SamplePadConfig,
    /// Source picker popup for deck A or B
    SourcePicker(Deck),
    /// Sample picker popup for a pad
    SamplePicker(usize),  // pad index
}

/// Source picker tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerInputMode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePickerTab {
    MpvSockets,
    AudioFiles,
    SuperCollider,
}

/// Source picker state
#[derive(Debug, Clone)]
pub struct SourcePickerState {
    pub tab: SourcePickerTab,
    pub input_mode: PickerInputMode,
    pub query: String,
    pub items: Vec<SourcePickerItem>,
    pub filtered: Vec<usize>,  // Indices into items
    pub selected: usize,       // Index into filtered
    pub scroll_offset: usize,
    pub current_dir: PathBuf,  // Current directory being browsed
    pub root_dir: PathBuf,     // Root samples directory (can't go above this)
    pub visible_height: usize, // Number of visible items (set by UI)
}

#[derive(Debug, Clone)]
pub struct SourcePickerItem {
    pub name: String,
    pub path: PathBuf,
    pub is_socket: bool,
    pub is_udp: bool,
    pub is_dir: bool,
}

impl SourcePickerState {
    pub fn new() -> Self {
        Self {
            tab: SourcePickerTab::AudioFiles,
            input_mode: PickerInputMode::Insert,
            query: String::new(),
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            current_dir: PathBuf::new(),
            root_dir: PathBuf::new(),
            visible_height: 12,
        }
    }

    pub fn set_root(&mut self, root: PathBuf) {
        self.root_dir = root.clone();
        self.current_dir = root;
    }

    pub fn filter(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.filtered = self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if query_lower.is_empty() {
                    true
                } else {
                    item.name.to_lowercase().contains(&query_lower)
                }
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn selected_item(&self) -> Option<&SourcePickerItem> {
        self.filtered.get(self.selected).and_then(|&i| self.items.get(i))
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            // Scroll up if cursor goes above visible area
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            // Scroll down if cursor goes below visible area
            if self.selected >= self.scroll_offset + self.visible_height {
                self.scroll_offset = self.selected.saturating_sub(self.visible_height - 1);
            }
        }
    }

    /// Ensure scroll offset keeps selected item visible
    pub fn clamp_scroll(&mut self) {
        if self.visible_height == 0 {
            return;
        }
        let max_scroll = self.filtered.len().saturating_sub(self.visible_height);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Ensure selected is visible
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + self.visible_height {
            self.scroll_offset = self.selected.saturating_sub(self.visible_height - 1);
        }
    }

    /// Check if we can go up a directory (not at root)
    pub fn can_go_up(&self) -> bool {
        self.current_dir != self.root_dir && self.current_dir.starts_with(&self.root_dir)
    }

    /// Get display path relative to root
    pub fn relative_path(&self) -> String {
        self.current_dir.strip_prefix(&self.root_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// Which pane is selected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedPane {
    DeckA,
    DjCenter,
    Loops,
    Xfader,
    DeckB,
    DeckC,
    Master,
}

impl SelectedPane {
    pub fn next(self) -> Self {
        match self {
            SelectedPane::DeckA => SelectedPane::DjCenter,
            SelectedPane::DjCenter => SelectedPane::Loops,
            SelectedPane::Loops => SelectedPane::Xfader,
            SelectedPane::Xfader => SelectedPane::DeckB,
            SelectedPane::DeckB => SelectedPane::DeckC,
            SelectedPane::DeckC => SelectedPane::Master,
            SelectedPane::Master => SelectedPane::DeckA,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            SelectedPane::DeckA => SelectedPane::Master,
            SelectedPane::DjCenter => SelectedPane::DeckA,
            SelectedPane::Loops => SelectedPane::DjCenter,
            SelectedPane::Xfader => SelectedPane::Loops,
            SelectedPane::DeckB => SelectedPane::Xfader,
            SelectedPane::DeckC => SelectedPane::DeckB,
            SelectedPane::Master => SelectedPane::DeckC,
        }
    }
}

/// Main application state
pub struct App {
    pub mixer: MixerState,
    pub sample_pads: SamplePadGrid,
    pub audio_manager: AudioSourceManager,
    pub mode: AppMode,
    pub selected_pane: SelectedPane,
    pub should_quit: bool,
    pub last_tick: Instant,
    pub tick_rate: Duration,
    // Mouse dragging state
    drag_start_y: Option<u16>,
    drag_start_x: Option<u16>,
    drag_start_value: Option<f32>,
    // Channel strip areas for mouse hit testing
    channel_areas: Vec<ChannelArea>,
    // Sample pad areas for mouse hit testing
    pad_areas: Vec<(usize, u16, u16, u16, u16)>, // (pad_idx, x, y, w, h)
    // Source picker
    pub source_picker: SourcePickerState,
    pub music_dir: PathBuf,
    pub samples_dir: PathBuf,
    // Currently selected pad in DJ center (when navigating pads)
    pub selected_pad_idx: Option<usize>,
    // MPV clients for each deck
    mpv_deck_a: Option<MpvClient>,
    mpv_deck_b: Option<MpvClient>,
    mpv_deck_c: Option<MpvClient>,
    // SuperCollider clients for each deck
    sc_deck_a: Option<SuperColliderClient>,
    sc_deck_b: Option<SuperColliderClient>,
    sc_deck_c: Option<SuperColliderClient>,
    // System audio capture for master metering (via flexaudio)
    audio_capture: Option<AudioCapture>,
    // Sample playback engine (cached samples for instant playback)
    sample_engine: Option<SampleEngine>,
    // Rack state and audio player
    pub rack_state: RackState,
    rack_player: Option<RackPlayer>,
    // Frame counter for animations (blinking indicators, count-in)
    pub frame_counter: u8,
    // Elapsed time in ms since program start (for rack recording timestamps)
    pub elapsed_ms: u64,
    // Scroll offset for rack rows in DJ center
    pub rack_scroll_offset: usize,
    // Terminal height for calculating visible rack rows
    pub terminal_height: u16,
    // Audio output devices
    pub master_output: AudioOutput,
    pub cue_output: AudioOutput,
    // Selected output device indices (for UI navigation)
    pub selected_master_output_idx: usize,
    pub selected_cue_output_idx: usize,
    // Output device picker mode
    pub output_picker_active: bool,
    pub output_picker_target: OutputPickerTarget,
    // Debug log messages (circular buffer, last 100 messages)
    pub debug_log: Vec<String>,
}

/// Which output device picker is active
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPickerTarget {
    Master,
    Cue,
}

/// Tracks the screen areas for each channel for mouse interaction
#[derive(Debug, Clone, Default)]
pub struct ChannelArea {
    pub bounds: (u16, u16, u16, u16), // x, y, width, height
    pub control_rows: Vec<(ChannelControl, u16, u16)>, // control, y_start, y_end
}

impl App {
    pub fn new(num_channels: usize) -> Self {
        let mut mixer = MixerState::new(num_channels);

        // Set up channel names
        for (i, channel) in mixer.channels.iter_mut().enumerate() {
            channel.name = format!("INPUT {}", i + 1);
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Default samples directory: SuperCollider Dirt-Samples
        let default_samples_dir = std::env::var("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support/SuperCollider/downloaded-quarks/Dirt-Samples"))
            .unwrap_or_else(|_| cwd.clone());

        // Initialize sample engine for instant playback
        let sample_engine = SampleEngine::new().ok();

        // Initialize rack player from sample engine's stream handle
        let rack_player = sample_engine.as_ref()
            .map(|e| RackPlayer::new(e.mixer().clone()));

        Self {
            mixer,
            sample_pads: SamplePadGrid::new(),
            audio_manager: AudioSourceManager::new(),
            mode: AppMode::PaneSelect,
            selected_pane: SelectedPane::DeckA,
            should_quit: false,
            last_tick: Instant::now(),
            tick_rate: Duration::from_millis(50), // 20 FPS for meter updates
            drag_start_y: None,
            drag_start_x: None,
            drag_start_value: None,
            channel_areas: Vec::new(),
            pad_areas: Vec::new(),
            source_picker: SourcePickerState::new(),
            music_dir: cwd,
            samples_dir: default_samples_dir,
            selected_pad_idx: None,
            mpv_deck_a: None,
            mpv_deck_b: None,
            mpv_deck_c: None,
            sc_deck_a: None,
            sc_deck_b: None,
            sc_deck_c: None,
            audio_capture: AudioCapture::new().ok(),
            sample_engine,
            rack_state: RackState::new(),
            rack_player,
            frame_counter: 0,
            elapsed_ms: 0,
            rack_scroll_offset: 0,
            terminal_height: 24,
            master_output: AudioOutput::new(),
            cue_output: AudioOutput::new(),
            selected_master_output_idx: 0,
            selected_cue_output_idx: 0,
            output_picker_active: false,
            output_picker_target: OutputPickerTarget::Master,
            debug_log: Vec::new(),
        }
    }

    pub fn set_music_dir(&mut self, dir: PathBuf) {
        self.music_dir = dir;
    }

    pub fn set_samples_dir(&mut self, dir: PathBuf) {
        self.samples_dir = dir;
    }

    /// Configure audio sources from socket paths
    pub fn configure_sources(&mut self, sources: Vec<(String, String)>) {
        for (name, socket_path) in sources {
            let source = AudioSource::new(name, socket_path);
            self.audio_manager.add_source(source);
        }

        // Match channel names to source names
        for (i, source) in self.audio_manager.sources().iter().enumerate() {
            if let Some(channel) = self.mixer.channels.get_mut(i) {
                channel.name = source.name().to_string();
            }
        }
    }

    /// Main tick - update meters, etc.
    pub fn tick(&mut self) {
        let real_master = self
            .audio_capture
            .as_ref()
            .map(|cap| {
                let m = cap.read_meters();
                (m.peak_left, m.peak_right, m.rms_left, m.rms_right)
            });

        // Poll per-deck audio levels from MPV astats filters
        let mut real_channels = Vec::new();
        for (i, client_opt) in [
            self.mpv_deck_a.as_mut(),
            self.mpv_deck_b.as_mut(),
            self.mpv_deck_c.as_mut(),
        ]
        .iter_mut()
        .enumerate()
        {
            if let Some(client) = client_opt {
                let (peak_l, peak_r, rms_l, rms_r) = client.get_audio_levels();
                if peak_l > 0.0 || peak_r > 0.0 {
                    real_channels.push((i, peak_l, peak_r, rms_l, rms_r));
                }
            }
        }

        self.mixer.update_meters(real_master, &real_channels);
        self.sample_pads.update();
    }

    /// Check if we're in edit mode
    pub fn is_editing(&self) -> bool {
        self.mode == AppMode::Edit
    }

    /// Handle keyboard input
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Global shortcuts available from any mode
        match key.code {
            // Quit from anywhere
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            // Source picker for Deck A from anywhere
            KeyCode::Char('A') => {
                self.open_source_picker(Deck::A);
                return;
            }
            // Source picker for Deck B from anywhere
            KeyCode::Char('B') => {
                self.open_source_picker(Deck::B);
                return;
            }
            // Source picker for Deck C (CUE) from anywhere
            KeyCode::Char('C') => {
                self.open_source_picker(Deck::C);
                return;
            }
            _ => {}
        }

        // Handle recording commit (global - works from any mode)
        if self.is_rack_recording() && key.code == KeyCode::Char(' ') {
            self.log_debug("Space pressed during recording - committing");
            self.commit_rack_recording();
            return;
        }

        // Handle output picker navigation if active
        if self.output_picker_active {
            match key.code {
                KeyCode::Esc => {
                    self.output_picker_active = false;
                    return;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    let max_idx = match self.output_picker_target {
                        OutputPickerTarget::Master => self.master_output.devices().len(),
                        OutputPickerTarget::Cue => self.cue_output.devices().len(),
                    };
                    if max_idx > 0 {
                        match self.output_picker_target {
                            OutputPickerTarget::Master => {
                                self.selected_master_output_idx = (self.selected_master_output_idx + 1).min(max_idx - 1);
                            }
                            OutputPickerTarget::Cue => {
                                self.selected_cue_output_idx = (self.selected_cue_output_idx + 1).min(max_idx - 1);
                            }
                        }
                    }
                    return;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    match self.output_picker_target {
                        OutputPickerTarget::Master => {
                            self.selected_master_output_idx = self.selected_master_output_idx.saturating_sub(1);
                        }
                        OutputPickerTarget::Cue => {
                            self.selected_cue_output_idx = self.selected_cue_output_idx.saturating_sub(1);
                        }
                    }
                    return;
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.select_output_device();
                    return;
                }
                _ => {}
            }
            return;
        }

        // Mode-specific handling
        self.log_debug(format!("Key {:?} in mode {:?}", key.code, self.mode));
        match self.mode {
            AppMode::Help => self.handle_help_key(key),
            AppMode::PaneSelect => self.handle_pane_select_key(key),
            AppMode::ControlSelect => self.handle_control_select_key(key),
            AppMode::Edit => self.handle_edit_key(key),
            AppMode::SamplePads => self.handle_pad_key(key),
            AppMode::SamplePadConfig => self.handle_pad_config_key(key),
            AppMode::SourcePicker(_) => self.handle_source_picker_key(key),
            AppMode::SamplePicker(_) => self.handle_sample_picker_key(key),
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.mode = AppMode::PaneSelect;
            }
            _ => {}
        }
    }

    /// Mode 1: Pane Select - navigate between panes with Tab/hjkl
    fn handle_pane_select_key(&mut self, key: KeyEvent) {
        match key.code {
            // Esc does nothing in PaneSelect (already at top level)
            KeyCode::Esc => {}

            // Help
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }

            // Toggle sample pad mode
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.mode = AppMode::SamplePads;
                self.sample_pads.active = true;
            }

            // Tab: next pane (round-robin)
            KeyCode::Tab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.next();
                self.sync_pane_to_mixer();
            }

            // Shift+Tab: previous pane (round-robin)
            KeyCode::BackTab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.prev();
                self.sync_pane_to_mixer();
            }

            // h/l: horizontal navigation across mixer layout
            // DeckA ↔ Xfader ↔ DeckB ↔ CUE ↔ Master
            KeyCode::Char('l') | KeyCode::Right => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DeckA => SelectedPane::Xfader,
                    SelectedPane::Xfader => SelectedPane::DeckB,
                    SelectedPane::DeckB => SelectedPane::DeckC,
                    SelectedPane::DeckC => SelectedPane::DeckA,
                    SelectedPane::Master => SelectedPane::DeckA,
                    SelectedPane::DjCenter | SelectedPane::Loops => SelectedPane::DeckB,
                };
                self.sync_pane_to_mixer();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DeckA => SelectedPane::DeckC,
                    SelectedPane::Xfader => SelectedPane::DeckA,
                    SelectedPane::DeckB => SelectedPane::Xfader,
                    SelectedPane::DeckC => SelectedPane::DeckB,
                    SelectedPane::Master => SelectedPane::DeckB,
                    SelectedPane::DjCenter | SelectedPane::Loops => SelectedPane::DeckA,
                };
                self.sync_pane_to_mixer();
            }

            // j/k: vertical pane navigation
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DeckA | SelectedPane::DjCenter => SelectedPane::Loops,
                    SelectedPane::Loops => SelectedPane::Xfader,
                    SelectedPane::Xfader => SelectedPane::DeckB,
                    SelectedPane::DeckB => SelectedPane::DeckC,
                    SelectedPane::DeckC => SelectedPane::Master,
                    SelectedPane::Master => SelectedPane::DeckA,
                };
                self.sync_pane_to_mixer();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DeckA => SelectedPane::Master,
                    SelectedPane::DjCenter => SelectedPane::DeckA,
                    SelectedPane::Loops => SelectedPane::DjCenter,
                    SelectedPane::Xfader => SelectedPane::Loops,
                    SelectedPane::DeckB => SelectedPane::Xfader,
                    SelectedPane::DeckC => SelectedPane::DeckB,
                    SelectedPane::Master => SelectedPane::DeckC,
                };
                self.sync_pane_to_mixer();
            }

            // Enter: activate control select mode for this pane
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.selected_pane == SelectedPane::Xfader {
                    // Xfader has one control - go straight to edit
                    self.mixer.focus = SelectionFocus::Global;
                    self.mixer.selected_global = GlobalControl::Crossfader;
                    self.mode = AppMode::Edit;
                } else {
                    self.mode = AppMode::ControlSelect;
                    if self.selected_pane == SelectedPane::DjCenter {
                        self.selected_pad_idx = Some(0);
                    }
                    self.sync_pane_to_mixer();
                }
            }

            // c: open pad config if in DJ center with a pad selected
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if self.selected_pane == SelectedPane::DjCenter {
                    if self.selected_pad_idx.is_none() {
                        self.selected_pad_idx = Some(0);
                    }
                    self.mode = AppMode::SamplePadConfig;
                    self.sample_pads.config_mode = true;
                    self.sample_pads.selected_pad = self.selected_pad_idx.unwrap_or(0);
                    // Always focus the first control (Sample) when opening pad config
                    self.sample_pads.selected_control = PadControl::Sample;
                }
            }

            // Quick toggle shortcuts (work in PaneSelect too)
            KeyCode::Char('m') => {
                if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.muted = !channel.muted;
                    }
                    self.sync_mute_to_mpv(ch_idx);
                } else if self.selected_pane == SelectedPane::Master {
                    self.mixer.master.muted = !self.mixer.master.muted;
                    self.sync_deck_volume(true);
                    self.sync_deck_volume(false);
                }
            }
            KeyCode::Char('s') => {
                if let SelectionFocus::Channel(_ch_idx) = self.mixer.focus {
                    let was_solo = self.mixer.selected_channel_mut().map(|c| c.solo).unwrap_or(false);
                    if !was_solo {
                        // Exclusive solo: clear all others, then enable this one
                        for ch in &mut self.mixer.channels {
                            ch.solo = false;
                        }
                        self.mixer.cue_channel.solo = false;
                    }
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.solo = !was_solo;
                    }
                    self.mixer.solo_active = self.mixer.channels.iter().any(|c| c.solo)
                        || self.mixer.cue_channel.solo;
                    self.sync_solo_to_all_mpv();
                }
            }

            _ => {}
        }
    }

    /// Sync selected_pane to mixer focus/channel
    fn sync_pane_to_mixer(&mut self) {
        match self.selected_pane {
            SelectedPane::DeckA => {
                self.mixer.focus = SelectionFocus::Channel(self.mixer.dj.deck_a_channel);
                self.mixer.selected_channel = self.mixer.dj.deck_a_channel;
                self.mixer.selected_control = ChannelControl::Fader;
            }
            SelectedPane::DeckB => {
                self.mixer.focus = SelectionFocus::Channel(self.mixer.dj.deck_b_channel);
                self.mixer.selected_channel = self.mixer.dj.deck_b_channel;
                self.mixer.selected_control = ChannelControl::Fader;
            }
            SelectedPane::DjCenter => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::Crossfader;
            }
            SelectedPane::Loops => {
                // Loops pane uses its own rack selection, keep global focus
                self.mixer.focus = SelectionFocus::Global;
            }
            SelectedPane::Xfader => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::Crossfader;
            }
            SelectedPane::Master => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::MasterFader;
            }
            SelectedPane::DeckC => {
                self.mixer.focus = SelectionFocus::Channel(2);
                self.mixer.selected_channel = 2;
                self.mixer.selected_control = ChannelControl::Fader;
            }
        }
    }

    /// Mode 2: Control Select - navigate controls within pane
    fn handle_control_select_key(&mut self, key: KeyEvent) {
        match key.code {
            // Esc: back to pane select
            KeyCode::Esc => {
                self.selected_pad_idx = None;
                self.mode = AppMode::PaneSelect;
            }

            // Help
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
            }

            // Toggle sample pad mode
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.mode = AppMode::SamplePads;
                self.sample_pads.active = true;
            }

            // Tab: next pane (quick switch, round-robin)
            KeyCode::Tab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.next();
                self.sync_pane_to_mixer();
            }

            // Shift+Tab: previous pane (quick switch, round-robin)
            KeyCode::BackTab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.prev();
                self.sync_pane_to_mixer();
            }

            // Navigation within pane
            KeyCode::Char('j') | KeyCode::Down => {
                self.navigate_control_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.navigate_control_up();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.navigate_control_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.navigate_control_right();
            }

            // Enter/Space: context-dependent action
            KeyCode::Enter | KeyCode::Char(' ') => {
                // Handle output picker first if active
                if self.output_picker_active {
                    self.select_output_device();
                    return;
                }

                if self.selected_pane == SelectedPane::DjCenter {
                    // Pad selected → open pad config
                    if let Some(pad_idx) = self.selected_pad_idx {
                        self.sample_pads.selected_pad = pad_idx;
                        self.mode = AppMode::SamplePadConfig;
                        self.sample_pads.config_mode = true;
                        // Always focus the first control (Sample) when opening pad config
                        self.sample_pads.selected_control = PadControl::Sample;
                        return;
                    }
                }
                if self.selected_pane == SelectedPane::Loops {
                    // Rack selected → toggle playback
                    if let Some(rack_idx) = self.rack_state.selected_rack {
                        self.toggle_rack_playback(rack_idx);
                        return;
                    }
                }
                if self.selected_pane == SelectedPane::DeckC {
                    // CUE deck controls
                    match self.mixer.selected_control {
                        ChannelControl::CueSendToA => {
                            self.mixer.send_cue_to_deck(SendTarget::A);
                            return;
                        }
                        ChannelControl::CueSendToB => {
                            self.mixer.send_cue_to_deck(SendTarget::B);
                            return;
                        }
                        ChannelControl::CueOutputSelect => {
                            self.open_output_picker(OutputPickerTarget::Cue);
                            return;
                        }
                        _ => {} // Fall through to normal deck control handling
                    }
                }
                if self.selected_pane == SelectedPane::Master {
                    // Master output selector
                    if self.mixer.selected_global == GlobalControl::MasterOutputSelect {
                        self.open_output_picker(OutputPickerTarget::Master);
                        return;
                    }
                }

                if self.is_current_control_continuous() {
                    self.mode = AppMode::Edit;
                } else {
                    self.toggle_current_control();
                }
            }

            // r: start recording on selected rack
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.selected_pane == SelectedPane::Loops {
                    if let Some(rack_idx) = self.rack_state.selected_rack {
                        self.start_rack_recording(rack_idx);
                    }
                }
            }

            // a: add new loop (from anywhere in LOOPS pane)
            KeyCode::Char('a') => {
                if self.selected_pane == SelectedPane::Loops {
                    self.rack_state.add_rack();
                }
            }

            // x: remove selected loop
            KeyCode::Char('x') => {
                if self.selected_pane == SelectedPane::Loops {
                    if let Some(rack_idx) = self.rack_state.selected_rack {
                        // Clean up rack audio buffer
                        if let Some(ref mut player) = self.rack_player {
                            player.delete_rack(rack_idx);
                        }
                        self.rack_state.remove_rack(rack_idx);
                    }
                }
            }

            // Quick toggles
            KeyCode::Char('m') => {
                if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.muted = !channel.muted;
                    }
                    self.sync_mute_to_mpv(ch_idx);
                } else if self.selected_pane == SelectedPane::Master {
                    self.mixer.master.muted = !self.mixer.master.muted;
                    self.sync_deck_volume(true);
                    self.sync_deck_volume(false);
                }
            }
            KeyCode::Char('s') => {
                if let SelectionFocus::Channel(_ch_idx) = self.mixer.focus {
                    let was_solo = self.mixer.selected_channel_mut().map(|c| c.solo).unwrap_or(false);
                    if !was_solo {
                        for ch in &mut self.mixer.channels {
                            ch.solo = false;
                        }
                        self.mixer.cue_channel.solo = false;
                    }
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.solo = !was_solo;
                    }
                    self.mixer.solo_active = self.mixer.channels.iter().any(|c| c.solo)
                        || self.mixer.cue_channel.solo;
                    // Sync all channels to apply solo logic
                    self.sync_solo_to_all_mpv();
                }
            }

            // Reset to default
            KeyCode::Char('0') => {
                self.reset_current_control();
            }

            // c: open pad config if pad selected, else center pan/crossfader
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if self.selected_pane == SelectedPane::DjCenter {
                    if let Some(pad_idx) = self.selected_pad_idx {
                        self.sample_pads.selected_pad = pad_idx;
                        self.mode = AppMode::SamplePadConfig;
                        self.sample_pads.config_mode = true;
                        // Always focus the first control (Sample) when opening pad config
                        self.sample_pads.selected_control = PadControl::Sample;
                        return;
                    }
                }
                match self.mixer.focus {
                    SelectionFocus::Channel(_) => {
                        if self.mixer.selected_control == ChannelControl::Pan {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.pan = 0.0;
                            }
                        }
                    }
                    SelectionFocus::Global => {
                        if self.mixer.selected_global == GlobalControl::Crossfader {
                            self.mixer.dj.crossfader = 0.0;
                        }
                    }
                }
                self.sync_current_control_to_mpv();
            }

            _ => {}
        }
    }

    /// Navigate to next control down within current pane
    fn navigate_control_down(&mut self) {
        match self.selected_pane {
            SelectedPane::DeckA | SelectedPane::DeckB | SelectedPane::DeckC => {
                self.mixer.select_next_control(self.selected_pane == SelectedPane::DeckC);
            }
            SelectedPane::DjCenter => {
                if let Some(pad_idx) = self.selected_pad_idx {
                    let row = pad_idx / 4;
                    let col = pad_idx % 4;
                    // Round-robin: wrap from bottom row to top row
                    let new_row = if row == 3 { 0 } else { row + 1 };
                    self.selected_pad_idx = Some(new_row * 4 + col);
                } else if self.mixer.selected_global == GlobalControl::Crossfader {
                    // Crossfader → top controls
                    self.mixer.selected_global = GlobalControl::HeadphoneVolume;
                } else {
                    // Top controls → pad grid
                    self.selected_pad_idx = Some(0);
                }
            }
            SelectedPane::Loops => {
                if self.rack_state.selected_rack.is_some() {
                    // In rack area → move down
                    self.rack_state.select_down();
                    if let Some(idx) = self.rack_state.selected_rack {
                        // Scroll down if selected rack is past visible area
                        let max_visible = self.loops_max_visible();
                        if idx >= self.rack_scroll_offset + max_visible {
                            self.rack_scroll_offset = idx + 1 - max_visible;
                        }
                    }
                } else {
                    // No selection → enter at first rack
                    if !self.rack_state.racks.is_empty() {
                        self.rack_state.selected_rack = Some(0);
                    }
                }
            }
            SelectedPane::Xfader => {} // Single control, nothing to navigate
            SelectedPane::Master => {
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::MasterFader => GlobalControl::MasterMute,
                    GlobalControl::MasterMute => GlobalControl::MasterOutputSelect,
                    GlobalControl::MasterOutputSelect => GlobalControl::MasterFader,
                    other => other,
                };
            }
        }
    }

    /// Navigate to previous control up within current pane (round-robin)
    fn navigate_control_up(&mut self) {
        match self.selected_pane {
            SelectedPane::DeckA | SelectedPane::DeckB | SelectedPane::DeckC => {
                self.mixer.select_prev_control(self.selected_pane == SelectedPane::DeckC);
            }
            SelectedPane::DjCenter => {
                if let Some(pad_idx) = self.selected_pad_idx {
                    let row = pad_idx / 4;
                    let col = pad_idx % 4;
                    // Round-robin: wrap from top row to bottom row
                    let new_row = if row == 0 { 3 } else { row - 1 };
                    self.selected_pad_idx = Some(new_row * 4 + col);
                } else if self.mixer.selected_global == GlobalControl::Crossfader {
                    // Crossfader → bottom pad row
                    self.selected_pad_idx = Some(12);
                } else {
                    // Top controls → crossfader
                    self.mixer.selected_global = GlobalControl::Crossfader;
                }
            }
            SelectedPane::Loops => {
                if self.rack_state.selected_rack.is_some() {
                    // In rack area → move up
                    self.rack_state.select_up();
                    if let Some(idx) = self.rack_state.selected_rack {
                        // Scroll up if selected rack is above visible area
                        if idx < self.rack_scroll_offset {
                            self.rack_scroll_offset = idx;
                        }
                    }
                } else {
                    // No selection → select last rack
                    if !self.rack_state.racks.is_empty() {
                        let last = self.rack_state.racks.len() - 1;
                        self.rack_state.selected_rack = Some(last);
                        let max_visible = self.loops_max_visible();
                        if last >= self.rack_scroll_offset + max_visible {
                            self.rack_scroll_offset = last + 1 - max_visible;
                        }
                    }
                }
            }
            SelectedPane::Xfader => {} // Single control, nothing to navigate
            SelectedPane::Master => {
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::MasterFader => GlobalControl::MasterOutputSelect,
                    GlobalControl::MasterMute => GlobalControl::MasterFader,
                    GlobalControl::MasterOutputSelect => GlobalControl::MasterMute,
                    other => other,
                };
            }
        }
    }

    /// Navigate left within DJ center (CUE/PH/BT are horizontal, pads too)
    fn navigate_control_left(&mut self) {
        if self.selected_pane == SelectedPane::DjCenter {
            if let Some(pad_idx) = self.selected_pad_idx {
                let row = pad_idx / 4;
                let col = pad_idx % 4;
                // Round-robin: wrap from col 0 to col 3 on same row
                let new_col = if col == 0 { 3 } else { col - 1 };
                self.selected_pad_idx = Some(row * 4 + new_col);
            } else {
                // Round-robin for DJ center top controls
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::HeadphoneVolume => GlobalControl::HeadphoneVolume,
                    other => other,
                };
            }
        } else if self.selected_pane == SelectedPane::DeckA || self.selected_pane == SelectedPane::DeckB || self.selected_pane == SelectedPane::DeckC {
            match self.mixer.selected_control {
                ChannelControl::Mute => {
                    self.mixer.selected_control = ChannelControl::Solo;
                }
                ChannelControl::Solo => {
                    self.mixer.selected_control = ChannelControl::Mute;
                }
                _ => {
                    if let Some(paired) = self.mixer.selected_control.eq_kill_pair() {
                        self.mixer.selected_control = paired;
                    }
                }
            }
        } else if self.selected_pane == SelectedPane::Master {
            match self.mixer.selected_global {
                GlobalControl::MasterMute => {
                    self.mixer.selected_global = GlobalControl::MasterOutputSelect;
                }
                GlobalControl::MasterOutputSelect => {
                    self.mixer.selected_global = GlobalControl::MasterMute;
                }
                _ => {}
            }
        }
    }

    /// Navigate right within DJ center
    fn navigate_control_right(&mut self) {
        if self.selected_pane == SelectedPane::DjCenter {
            if let Some(pad_idx) = self.selected_pad_idx {
                let row = pad_idx / 4;
                let col = pad_idx % 4;
                let new_col = if col == 3 { 0 } else { col + 1 };
                self.selected_pad_idx = Some(row * 4 + new_col);
            } else {
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::HeadphoneVolume => GlobalControl::HeadphoneVolume,
                    other => other,
                };
            }
        } else if self.selected_pane == SelectedPane::DeckA || self.selected_pane == SelectedPane::DeckB || self.selected_pane == SelectedPane::DeckC {
            match self.mixer.selected_control {
                ChannelControl::Mute => {
                    self.mixer.selected_control = ChannelControl::Solo;
                }
                ChannelControl::Solo => {
                    self.mixer.selected_control = ChannelControl::Mute;
                }
                _ => {
                    if let Some(paired) = self.mixer.selected_control.eq_kill_pair() {
                        self.mixer.selected_control = paired;
                    }
                }
            }
        } else if self.selected_pane == SelectedPane::Master {
            match self.mixer.selected_global {
                GlobalControl::MasterMute => {
                    self.mixer.selected_global = GlobalControl::MasterOutputSelect;
                }
                GlobalControl::MasterOutputSelect => {
                    self.mixer.selected_global = GlobalControl::MasterMute;
                }
                _ => {}
            }
        }
    }

    /// Check if current control is continuous (vs toggle)
    fn is_current_control_continuous(&self) -> bool {
        match self.mixer.focus {
            SelectionFocus::Channel(_) => self.mixer.selected_control.is_continuous(),
            SelectionFocus::Global => {
                matches!(self.mixer.selected_global,
                    GlobalControl::Crossfader |
                    GlobalControl::HeadphoneVolume |
                    GlobalControl::MasterFader)
            }
        }
    }

    /// Toggle current control (for buttons)
    fn toggle_current_control(&mut self) {
        match self.mixer.focus {
            SelectionFocus::Channel(ch_idx) => {
                let control = self.mixer.selected_control;
                self.mixer.toggle_selected();

                // Sync to MPV after toggle
                match control {
                    ChannelControl::PlayPause => {
                        self.sync_playpause_to_mpv(ch_idx);
                    }
                    ChannelControl::Mute => {
                        self.sync_mute_to_mpv(ch_idx);
                    }
                    ChannelControl::EqLowKill | ChannelControl::EqMidKill | ChannelControl::EqHighKill => {
                        self.sync_eq_to_mpv(ch_idx);
                    }
                    ChannelControl::Solo => {
                        self.mixer.solo_active = self.mixer.channels.iter().any(|c| c.solo)
                            || self.mixer.cue_channel.solo;
                        self.sync_solo_to_all_mpv();
                    }
                    _ => {}
                }
            }
            SelectionFocus::Global => {
                match self.mixer.selected_global {
                    GlobalControl::MasterMute => {
                        self.mixer.master.muted = !self.mixer.master.muted;
                        self.sync_deck_volume(true);
                        self.sync_deck_volume(false);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Mode 3: Edit - hjkl adjusts values, Esc returns to ControlSelect
    fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit edit mode -> back to control select (or pane select for Xfader)
            KeyCode::Esc | KeyCode::Enter => {
                if self.selected_pane == SelectedPane::Xfader {
                    // Xfader skipped ControlSelect on entry, skip it on exit too
                    self.mode = AppMode::PaneSelect;
                } else {
                    self.mode = AppMode::ControlSelect;
                }
            }

            // Tab: next pane (quick switch while editing, round-robin)
            KeyCode::Tab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.next();
                self.sync_pane_to_mixer();
            }

            // Shift+Tab: previous pane (quick switch while editing, round-robin)
            KeyCode::BackTab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.prev();
                self.sync_pane_to_mixer();
            }

            // Adjust values with hjkl
            KeyCode::Char('h') | KeyCode::Left => {
                self.mixer.adjust_selected(-0.05);
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.mixer.adjust_selected(0.05);
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mixer.adjust_selected(0.05);
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.mixer.adjust_selected(-0.05);
                self.sync_current_control_to_mpv();
            }

            // Coarse adjustment with Shift
            KeyCode::Char('H') => {
                self.mixer.adjust_selected(-0.2);
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('L') => {
                self.mixer.adjust_selected(0.2);
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('K') => {
                self.mixer.adjust_selected(0.2);
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('J') => {
                self.mixer.adjust_selected(-0.2);
                self.sync_current_control_to_mpv();
            }

            // Reset
            KeyCode::Char('0') => {
                self.reset_current_control();
                self.sync_current_control_to_mpv();
            }

            // Center (for pan/crossfader) or reset to default
            KeyCode::Char('c') | KeyCode::Char('C') => {
                match self.mixer.focus {
                    SelectionFocus::Channel(_) => {
                        match self.mixer.selected_control {
                            ChannelControl::Pan => {
                                if let Some(channel) = self.mixer.selected_channel_mut() {
                                    channel.pan = 0.0;
                                }
                            }
                            ChannelControl::Bpm => {
                                if let Some(channel) = self.mixer.selected_channel_mut() {
                                    channel.playback_speed = 1.0;
                                }
                            }
                            ChannelControl::Fader => {
                                if let Some(channel) = self.mixer.selected_channel_mut() {
                                    channel.fader = 0.5;
                                }
                            }
                            _ => {}
                        }
                    }
                    SelectionFocus::Global => {
                        match self.mixer.selected_global {
                            GlobalControl::Crossfader => {
                                self.mixer.dj.crossfader = 0.0;
                            }
                            GlobalControl::MasterFader => {
                                self.mixer.master.fader = 0.5;
                            }
                            _ => {}
                        }
                    }
                }
                self.sync_current_control_to_mpv();
            }

            _ => {}
        }
    }

    fn reset_current_control(&mut self) {
        match self.mixer.focus {
            SelectionFocus::Channel(_) => {
                let control = self.mixer.selected_control;
                if let Some(channel) = self.mixer.selected_channel_mut() {
                    match control {
                        ChannelControl::Fader => channel.fader = 0.5,
                        ChannelControl::Pan => channel.pan = 0.0,
                        ChannelControl::LowPassFilter => channel.lpf_freq = 20000.0,
                        ChannelControl::HighPassFilter => channel.hpf_freq = 20.0,
                        ChannelControl::EqLow => channel.eq_low = 0.0,
                        ChannelControl::EqMid => channel.eq_mid = 0.0,
                        ChannelControl::EqHigh => channel.eq_high = 0.0,
                        _ => {}
                    }
                }
            }
            SelectionFocus::Global => {
                match self.mixer.selected_global {
                    GlobalControl::HeadphoneVolume => self.mixer.dj.headphone_volume = 1.0,
                    GlobalControl::MasterFader => self.mixer.master.fader = 0.5,
                    _ => {}
                }
            }
        }
    }

    /// Handle keys when sample pad mode is active
    fn handle_pad_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit pad mode / cancel recording
            KeyCode::Esc => {
                if self.is_rack_recording() {
                    self.rack_state.mode = crate::state::RackMode::Idle;
                }
                self.mode = AppMode::PaneSelect;
                self.sample_pads.active = false;
            }

            // Toggle pad mode off
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.mode = AppMode::PaneSelect;
                self.sample_pads.active = false;
            }

            // Enter config mode
            KeyCode::Char('c') | KeyCode::Char('C') => {
                self.mode = AppMode::SamplePadConfig;
                self.sample_pads.config_mode = true;
                // Always focus the first control (Sample) when opening pad config
                self.sample_pads.selected_control = PadControl::Sample;
            }

            // Stop all pads / commit recording
            KeyCode::Char(' ') => {
                self.log_debug(format!("Space pressed, is_rack_recording: {}", self.is_rack_recording()));
                if self.is_rack_recording() {
                    self.commit_rack_recording();
                } else {
                    self.stop_all_samples();
                }
            }

            // Pad trigger keys: 4567 / RTYU / FGHJ / VBNM
            KeyCode::Char(c) => {
                if let Some(pad_idx) = self.sample_pads.trigger_by_key(c) {
                    self.log_debug(format!("Playing pad {}, recording: {}", pad_idx, self.is_rack_recording()));
                    self.play_sample(pad_idx);
                    // Record trigger if recording
                    if self.is_rack_recording() {
                        self.record_pad_trigger(pad_idx);
                    }
                }
            }

            _ => {}
        }
    }

    /// Play a sample using cached audio engine (instant playback)
    fn play_sample(&mut self, pad_idx: usize) {
        if let Some(pad) = self.sample_pads.pads.get(pad_idx) {
            if let Some(sample_path) = &pad.sample_path {
                if sample_path.exists() {
                    let config = pad.config.clone();
                    if let Some(ref mut engine) = self.sample_engine {
                        let _ = engine.play_with_config(sample_path, Some(&config));
                    } else {
                        // Fallback to mpv if sample engine unavailable
                        let _ = std::process::Command::new("mpv")
                            .arg("--no-video")
                            .arg("--really-quiet")
                            .arg("--no-terminal")
                            .arg(sample_path)
                            .spawn();
                    }
                }
            }
        }
    }

    /// Stop all playing samples
    fn stop_all_samples(&mut self) {
        self.sample_pads.stop_all();
        if let Some(ref mut engine) = self.sample_engine {
            engine.stop_all();
        }
    }

    /// Toggle playback of a rack
    fn toggle_rack_playback(&mut self, rack_idx: usize) {
        if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
            if rack.playing {
                rack.playing = false;
                if let Some(ref mut player) = self.rack_player {
                    player.stop_rack(rack_idx);
                }
            } else {
                rack.playing = true;
                if let Some(ref mut player) = self.rack_player {
                    let _ = player.play_loop(rack_idx);
                }
            }
        }
    }

    /// Start recording on a rack (begins count-in)
    fn start_rack_recording(&mut self, rack_idx: usize) {
        // Stop current playback
        if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
            rack.playing = false;
        }
        // Stop any playing racks
        if let Some(ref mut player) = self.rack_player {
            player.stop_rack(rack_idx);
        }
        // Begin count-in
        self.rack_state.mode = crate::state::RackMode::CountIn { step: 0, frame: 0 };
        self.rack_state.recording_start_ms = self.elapsed_ms;
    }

    /// Commit a rack recording and start playback
    fn commit_rack_recording(&mut self) {
        if let Some(rack_idx) = self.rack_state.selected_rack {
            self.rack_state.mode = crate::state::RackMode::Idle;
            
            // Stop recording and get the audio buffer
            if let Some(ref mut engine) = self.sample_engine {
                if let Some(recorded_audio) = engine.stop_recording() {
                    self.log_debug(format!("Recorded {} samples", recorded_audio.len()));
                    
                    // Store the recorded audio in the rack player
                    if let Some(ref mut player) = self.rack_player {
                        player.set_loop_buffer(rack_idx, recorded_audio, 44100, 2);
                        
                        // Start playback
                        if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
                            rack.playing = true;
                        }
                        match player.play_loop(rack_idx) {
                            Ok(_) => self.log_debug(format!("Started loop playback for rack {}", rack_idx)),
                            Err(e) => self.log_debug(format!("Failed to play loop: {}", e)),
                        }
                    }
                } else {
                    self.log_debug("No recorded audio captured!");
                }
            }
        }
    }

    /// Update rack state (count-in animation, frame counter)
    pub fn update_racks(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);

        match self.rack_state.mode {
            crate::state::RackMode::CountIn { ref mut step, ref mut frame } => {
                *frame = self.frame_counter;
                // Count-in timing: 3, 2, 1 (20 frames each = 1s at 20fps)
                let period = 20;
                if self.frame_counter % period == 0 {
                    *step += 1;
                    if *step >= 3 {
                        // Count-in done → reset timestamp and start recording
                        self.rack_state.recording_start_ms = self.elapsed_ms;
                        self.rack_state.mode = crate::state::RackMode::Recording;
                        self.mode = AppMode::SamplePads;
                        self.sample_pads.active = true;
                        
                        // Start audio recording
                        if let Some(ref mut engine) = self.sample_engine {
                            engine.start_recording(44100, 2);  // Standard sample rate and stereo
                            self.log_debug("Started audio recording");
                        }
                    }
                }
            }
            crate::state::RackMode::Recording => {
                // Record pad triggers with timestamps
                // (handled in play_sample when in recording mode)
            }
            crate::state::RackMode::Idle => {}
        }

        // Update rack playback state
        if let Some(ref mut player) = self.rack_player {
            player.cleanup();
            for (i, rack) in self.rack_state.racks.iter_mut().enumerate() {
                if rack.playing && !player.is_playing(i) {
                    rack.playing = false;
                }
            }
        }

        // Clamp scroll offset to valid range
        self.clamp_rack_scroll();
    }

    /// Clamp rack scroll offset to stay within valid bounds
    fn clamp_rack_scroll(&mut self) {
        let rack_count = self.rack_state.racks.len();
        if rack_count == 0 {
            self.rack_scroll_offset = 0;
            return;
        }
        // Max scroll is rack_count - 1 (show last rack at top)
        let max_scroll = rack_count.saturating_sub(1);
        if self.rack_scroll_offset > max_scroll {
            self.rack_scroll_offset = max_scroll;
        }
    }

    /// Calculate how many rack rows are visible in the Loops pane
    fn loops_max_visible(&self) -> usize {
        let loops_height = (self.terminal_height as f32 * 0.20) as u16;
        let loops_height = loops_height.max(3);
        // Subtract 2 for top/bottom borders
        loops_height.saturating_sub(2) as usize
    }

    /// Check if we're currently recording into a rack
    pub fn is_rack_recording(&self) -> bool {
        matches!(self.rack_state.mode, crate::state::RackMode::Recording)
    }

    /// Record a pad trigger into the current rack recording
    fn record_pad_trigger(&mut self, pad_idx: usize) {
        if let Some(rack_idx) = self.rack_state.selected_rack {
            let elapsed = self.elapsed_ms.saturating_sub(self.rack_state.recording_start_ms);
            if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
                rack.triggers.push(crate::state::RackTrigger {
                    time_ms: elapsed,
                    pad_idx,
                });
            }
        }
    }

    /// Handle keys when sample pad config mode is active (3-level nav)
    fn handle_pad_config_key(&mut self, key: KeyEvent) {
        // SPACE always previews the selected pad, at any level
        if key.code == KeyCode::Char(' ') {
            let pad_idx = self.sample_pads.selected_pad;
            self.play_sample(pad_idx);
            return;
        }

        if self.sample_pads.editing_control {
            // Level 3: editing a control value
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.sample_pads.editing_control = false;
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    self.sample_pads.adjust_selected_config(-0.05);
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.sample_pads.adjust_selected_config(0.05);
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.sample_pads.adjust_selected_config(-0.05);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.sample_pads.adjust_selected_config(0.05);
                }
                // Coarse adjustment
                KeyCode::Char('H') => {
                    self.sample_pads.adjust_selected_config(-0.2);
                }
                KeyCode::Char('L') => {
                    self.sample_pads.adjust_selected_config(0.2);
                }
                KeyCode::Char('K') => {
                    self.sample_pads.adjust_selected_config(0.2);
                }
                KeyCode::Char('J') => {
                    self.sample_pads.adjust_selected_config(-0.2);
                }
                KeyCode::Char('0') => {
                    self.sample_pads.reset_selected_config();
                }
                _ => {}
            }
        } else {
            // Level 2: navigating controls
            match key.code {
                KeyCode::Esc => {
                    // Exit config mode - return to the mode we came from
                    if self.sample_pads.active {
                        // Came from SamplePads (trigger) mode
                        self.mode = AppMode::SamplePads;
                    } else {
                        // Came from ControlSelect mode (navigating pads in DjCenter pane)
                        self.mode = AppMode::ControlSelect;
                    }
                    self.sample_pads.config_mode = false;
                    self.sample_pads.editing_control = false;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.sample_pads.config_control_down();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.sample_pads.config_control_up();
                }
                KeyCode::Enter => {
                    let control = self.sample_pads.selected_control;
                    match control {
                        PadControl::Sample => {
                            let pad_idx = self.sample_pads.selected_pad;
                            self.open_sample_picker(pad_idx);
                        }
                        PadControl::PlayMode => {
                            self.sample_pads.cycle_play_mode();
                        }
                        PadControl::FiltersHeader => {
                            // Non-interactive header, skip
                        }
                        _ if control.is_continuous() => {
                            self.sample_pads.editing_control = true;
                        }
                        _ if control.is_toggle() => {
                            self.sample_pads.toggle_selected_config();
                        }
                        _ => {}
                    }
                }
                KeyCode::Char('0') => {
                    self.sample_pads.reset_selected_config();
                }
                KeyCode::Char(c) => {
                    if let Some(idx) = self.sample_pads.pad_index_for_key(c) {
                        self.sample_pads.selected_pad = idx;
                    }
                }
                _ => {}
            }
        }
    }

    /// Handle mouse events
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match self.mode {
            AppMode::SamplePads | AppMode::SamplePadConfig => {
                self.handle_pad_mouse(mouse);
            }
            _ => {
                self.handle_mixer_mouse(mouse);
            }
        }
    }

    fn handle_mixer_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Try to select a channel/control at this position
                if let Some((channel_idx, control)) = self.hit_test(mouse.column, mouse.row) {
                    self.mixer.selected_channel = channel_idx;
                    self.mixer.selected_control = control;
                    self.mixer.focus = SelectionFocus::Channel(channel_idx);

                    // Start drag for continuous controls
                    if control.is_continuous() {
                        self.drag_start_y = Some(mouse.row);
                        self.drag_start_value = self.get_current_control_value();
                    } else {
                        // Toggle for buttons
                        self.mixer.toggle_selected();
                    }
                }
            }

            MouseEventKind::Drag(MouseButton::Left) => {
                if let (Some(start_y), Some(start_value)) = (self.drag_start_y, self.drag_start_value) {
                    let delta = (start_y as i16 - mouse.row as i16) as f32;
                    let sensitivity = 0.02;
                    let control = self.mixer.selected_control;

                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        match control {
                            ChannelControl::Fader => {
                                channel.fader = (start_value + delta * sensitivity).clamp(0.0, 1.0);
                            }
                            ChannelControl::Pan => {
                                channel.pan = (start_value + delta * sensitivity * 0.5).clamp(-1.0, 1.0);
                            }
                            ChannelControl::LowPassFilter => {
                                let log_start = start_value.log10();
                                let new_log = (log_start + delta * 0.02).clamp(1.3, 4.3);
                                channel.lpf_freq = 10f32.powf(new_log);
                            }
                            ChannelControl::HighPassFilter => {
                                let log_start = start_value.log10();
                                let new_log = (log_start + delta * 0.02).clamp(1.3, 4.3);
                                channel.hpf_freq = 10f32.powf(new_log);
                            }
                            ChannelControl::EqLow => {
                                channel.eq_low = (start_value + delta * 0.5).clamp(-15.0, 15.0);
                            }
                            ChannelControl::EqMid => {
                                channel.eq_mid = (start_value + delta * 0.5).clamp(-15.0, 15.0);
                            }
                            ChannelControl::EqHigh => {
                                channel.eq_high = (start_value + delta * 0.5).clamp(-15.0, 15.0);
                            }
                            _ => {}
                        }
                    }
                }
            }

            MouseEventKind::Up(MouseButton::Left) => {
                self.drag_start_y = None;
                self.drag_start_x = None;
                self.drag_start_value = None;
            }

            MouseEventKind::ScrollUp => {
                self.mixer.adjust_selected(0.5);
            }

            MouseEventKind::ScrollDown => {
                self.mixer.adjust_selected(-0.5);
            }

            _ => {}
        }
    }

    fn handle_pad_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Hit test against pad areas
                for &(pad_idx, x, y, w, h) in &self.pad_areas {
                    if mouse.column >= x && mouse.column < x + w
                        && mouse.row >= y && mouse.row < y + h
                    {
                        if self.mode == AppMode::SamplePadConfig {
                            self.sample_pads.selected_pad = pad_idx;
                        } else {
                            self.sample_pads.trigger_pad(pad_idx);
                            self.play_sample(pad_idx);
                        }
                        break;
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // Release gate-mode pads
                for &(pad_idx, x, y, w, h) in &self.pad_areas {
                    if mouse.column >= x && mouse.column < x + w
                        && mouse.row >= y && mouse.row < y + h
                    {
                        self.sample_pads.release_pad(pad_idx);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    /// Update channel areas for mouse hit testing
    pub fn update_channel_areas(&mut self, areas: Vec<ChannelArea>) {
        self.channel_areas = areas;
    }

    /// Hit test to find which channel/control is at a screen position
    fn hit_test(&self, x: u16, y: u16) -> Option<(usize, ChannelControl)> {
        for (idx, area) in self.channel_areas.iter().enumerate() {
            let (ax, ay, aw, ah) = area.bounds;
            if x >= ax && x < ax + aw && y >= ay && y < ay + ah {
                for &(control, y_start, y_end) in &area.control_rows {
                    if y >= y_start && y < y_end {
                        return Some((idx, control));
                    }
                }
                return Some((idx, ChannelControl::Fader));
            }
        }
        None
    }

    /// Get current value of selected control for drag operations
    fn get_current_control_value(&self) -> Option<f32> {
        self.mixer.selected_channel().map(|ch| match self.mixer.selected_control {
            ChannelControl::Fader => ch.fader,
            ChannelControl::Pan => ch.pan,
            ChannelControl::LowPassFilter => ch.lpf_freq,
            ChannelControl::HighPassFilter => ch.hpf_freq,
            ChannelControl::EqLow => ch.eq_low,
            ChannelControl::EqMid => ch.eq_mid,
            ChannelControl::EqHigh => ch.eq_high,
            _ => 0.0,
        })
    }

    pub fn show_help(&self) -> bool {
        self.mode == AppMode::Help
    }

    /// Open source picker for specified deck
    fn open_source_picker(&mut self, deck: Deck) {
        self.source_picker = SourcePickerState::new();
        self.scan_sources();
        self.mode = AppMode::SourcePicker(deck);
    }

    /// Open output device picker
    fn open_output_picker(&mut self, target: OutputPickerTarget) {
        self.output_picker_active = true;
        self.output_picker_target = target;
        // Refresh device list
        match target {
            OutputPickerTarget::Master => {
                self.master_output.refresh_devices();
                self.selected_master_output_idx = 0;
            }
            OutputPickerTarget::Cue => {
                self.cue_output.refresh_devices();
                self.selected_cue_output_idx = 0;
            }
        }
    }

    /// Select the currently highlighted output device
    fn select_output_device(&mut self) {
        let (devices, selected_idx) = match self.output_picker_target {
            OutputPickerTarget::Master => (
                self.master_output.devices().to_vec(),
                self.selected_master_output_idx,
            ),
            OutputPickerTarget::Cue => (
                self.cue_output.devices().to_vec(),
                self.selected_cue_output_idx,
            ),
        };

        if let Some(device_name) = devices.get(selected_idx) {
            // Record the selection for display purposes
            // Note: MPV uses its own CoreAudio device names, so we don't
            // route audio directly. Users should configure system audio
            // output (e.g., System Preferences > Sound) for routing.
            match self.output_picker_target {
                OutputPickerTarget::Master => {
                    self.master_output.select_main_device(device_name).ok();
                }
                OutputPickerTarget::Cue => {
                    self.cue_output.select_cue_device(device_name).ok();
                }
            }
        }

        self.output_picker_active = false;
    }

    /// Scan for MPV sockets and audio files
    fn scan_sources(&mut self) {
        self.source_picker.items.clear();

        match self.source_picker.tab {
            SourcePickerTab::MpvSockets => {
                self.scan_mpv_sockets();
            }
            SourcePickerTab::AudioFiles => {
                self.scan_audio_files();
            }
            SourcePickerTab::SuperCollider => {
                self.scan_supercollider_sources();
            }
        }

        self.source_picker.filter();
    }

    fn scan_mpv_sockets(&mut self) {
        // Common MPV socket locations
        let socket_patterns = [
            "/tmp/mpv*",
            "/tmp/mpvsocket*",
        ];

        for pattern in &socket_patterns {
            if let Ok(paths) = glob::glob(pattern) {
                for entry in paths.flatten() {
                    if entry.exists() {
                        let name = entry.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "mpv".to_string());
                        self.source_picker.items.push(SourcePickerItem {
                            name,
                            path: entry,
                            is_socket: true,
                            is_udp: false,
                            is_dir: false,
                        });
                    }
                }
            }
        }

        // Also check XDG runtime dir
        if let Ok(uid) = std::env::var("UID").or_else(|_| {
            std::process::Command::new("id")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        }) {
            let runtime_pattern = format!("/run/user/{}/mpv*", uid);
            if let Ok(paths) = glob::glob(&runtime_pattern) {
                for entry in paths.flatten() {
                    if entry.exists() {
                        let name = entry.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "mpv".to_string());
                        self.source_picker.items.push(SourcePickerItem {
                            name,
                            path: entry,
                            is_socket: true,
                            is_udp: false,
                            is_dir: false,
                        });
                    }
                }
            }
        }
    }

    fn scan_audio_files(&mut self) {
        let extensions = ["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "aiff"];

        if let Ok(entries) = std::fs::read_dir(&self.music_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        let ext_lower = ext.to_string_lossy().to_lowercase();
                        if extensions.contains(&ext_lower.as_str()) {
                            let name = path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            self.source_picker.items.push(SourcePickerItem {
                                name,
                                path,
                                is_socket: false,
                                is_udp: false,
                                is_dir: false,
                            });
                        }
                    }
                }
            }
        }

        // Sort alphabetically
        self.source_picker.items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    fn scan_supercollider_sources(&mut self) {
        self.source_picker.items.push(SourcePickerItem {
            name: "SuperCollider UDP (127.0.0.1:57110)".to_string(),
            path: PathBuf::from("udp://127.0.0.1:57110"),
            is_socket: false,
            is_udp: true,
            is_dir: false,
        });
    }

    /// Open sample picker for a pad
    fn open_sample_picker(&mut self, pad_idx: usize) {
        self.source_picker = SourcePickerState::new();
        self.source_picker.tab = SourcePickerTab::AudioFiles;
        self.source_picker.set_root(self.samples_dir.clone());
        self.scan_sample_files();
        self.mode = AppMode::SamplePicker(pad_idx);
    }

    /// Scan for sample files and folders in current directory
    fn scan_sample_files(&mut self) {
        let extensions = ["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "aiff", "aif"];
        self.source_picker.items.clear();

        // Add ".." entry if not at root
        if self.source_picker.can_go_up() {
            self.source_picker.items.push(SourcePickerItem {
                name: "..".to_string(),
                path: self.source_picker.current_dir.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| self.source_picker.root_dir.clone()),
                is_socket: false,
                is_udp: false,
                is_dir: true,
            });
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&self.source_picker.current_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Skip hidden files/folders
                if name.starts_with('.') {
                    continue;
                }

                if path.is_dir() {
                    dirs.push(SourcePickerItem {
                        name: format!("{}/", name),
                        path,
                        is_socket: false,
                        is_udp: false,
                        is_dir: true,
                    });
                } else if path.is_file() {
                    if let Some(ext) = path.extension() {
                        let ext_lower = ext.to_string_lossy().to_lowercase();
                        if extensions.contains(&ext_lower.as_str()) {
                            files.push(SourcePickerItem {
                                name,
                                path,
                                is_socket: false,
                                is_udp: false,
                                is_dir: false,
                            });
                        }
                    }
                }
            }
        }

        // Sort directories and files alphabetically
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        // Add directories first, then files
        self.source_picker.items.extend(dirs);
        self.source_picker.items.extend(files);

        self.source_picker.filter();
    }

    /// Navigate into a directory in sample picker
    fn enter_sample_directory(&mut self, path: PathBuf) {
        self.source_picker.current_dir = path;
        self.source_picker.query.clear();
        self.scan_sample_files();
    }

    /// Preview (play) the currently selected sample without assigning it
    fn preview_sample(&mut self) {
        if let Some(item) = self.source_picker.selected_item() {
            if !item.is_dir && item.path.exists() {
                if let Some(ref mut engine) = self.sample_engine {
                    // Use cached engine for instant preview
                    let _ = engine.play(&item.path);
                } else {
                    // Fallback to mpv
                    let _ = std::process::Command::new("mpv")
                        .arg("--no-video")
                        .arg("--really-quiet")
                        .arg("--no-terminal")
                        .arg(&item.path)
                        .spawn();
                }
            }
        }
    }

    /// Handle keys in sample picker mode
    fn handle_sample_picker_key(&mut self, key: KeyEvent) {
        // Set visible height for scrolling (popup height minus header rows)
        // Popup is 20 rows, minus 2 for border, 1 for path, 1 for search, 1 for hint = 15
        self.source_picker.visible_height = 15;

        match self.source_picker.input_mode {
            PickerInputMode::Normal => match key.code {
                KeyCode::Esc => {
                    self.mode = AppMode::ControlSelect;
                }
                KeyCode::Char('i') => {
                    self.source_picker.input_mode = PickerInputMode::Insert;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.source_picker.move_down();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.source_picker.move_up();
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    if self.source_picker.can_go_up() {
                        if let Some(parent) = self.source_picker.current_dir.parent() {
                            self.enter_sample_directory(parent.to_path_buf());
                        }
                    }
                }
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                    if let Some(item) = self.source_picker.selected_item().cloned() {
                        if item.is_dir {
                            self.enter_sample_directory(item.path);
                        } else if let AppMode::SamplePicker(pad_idx) = self.mode {
                            self.assign_sample_to_pad(pad_idx);
                            self.mode = AppMode::ControlSelect;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    self.preview_sample();
                }
                KeyCode::Char('g') => {
                    self.source_picker.selected = 0;
                    self.source_picker.scroll_offset = 0;
                }
                KeyCode::Char('G') => {
                    let last = self.source_picker.filtered.len().saturating_sub(1);
                    self.source_picker.selected = last;
                    self.source_picker.clamp_scroll();
                }
                _ => {}
            },
            PickerInputMode::Insert => match key.code {
                KeyCode::Esc => {
                    self.source_picker.input_mode = PickerInputMode::Normal;
                }
                KeyCode::Enter => {
                    if let Some(item) = self.source_picker.selected_item().cloned() {
                        if item.is_dir {
                            self.enter_sample_directory(item.path);
                        } else if let AppMode::SamplePicker(pad_idx) = self.mode {
                            self.assign_sample_to_pad(pad_idx);
                            self.mode = AppMode::ControlSelect;
                        }
                    }
                }
                KeyCode::Backspace => {
                    if self.source_picker.query.is_empty() {
                        if self.source_picker.can_go_up() {
                            if let Some(parent) = self.source_picker.current_dir.parent() {
                                self.enter_sample_directory(parent.to_path_buf());
                            }
                        }
                    } else {
                        self.source_picker.query.pop();
                        self.source_picker.filter();
                    }
                }
                KeyCode::Up => {
                    self.source_picker.move_up();
                }
                KeyCode::Down => {
                    self.source_picker.move_down();
                }
                KeyCode::Char(c) => {
                    self.source_picker.query.push(c);
                    self.source_picker.filter();
                }
                _ => {}
            },
        }
    }

    /// Assign selected sample to pad and preload it
    fn assign_sample_to_pad(&mut self, pad_idx: usize) {
        if let Some(item) = self.source_picker.selected_item().cloned() {
            // Preload into cache for instant playback
            if let Some(ref mut engine) = self.sample_engine {
                let _ = engine.preload(&item.path);
            }
            self.sample_pads.assign_sample_to_pad(pad_idx, item.path, Some(item.name));
        }
    }

    /// Handle keys in source picker mode
    fn handle_source_picker_key(&mut self, key: KeyEvent) {
        // Set visible height for scrolling (popup height minus header rows)
        // Popup is 18 rows, minus 2 for border, 1 for tabs, 1 for search, 1 for hint = 13
        self.source_picker.visible_height = 13;

        match self.source_picker.input_mode {
            PickerInputMode::Normal => match key.code {
                KeyCode::Esc => {
                    self.mode = AppMode::PaneSelect;
                }
                KeyCode::Char('i') => {
                    self.source_picker.input_mode = PickerInputMode::Insert;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.source_picker.move_down();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.source_picker.move_up();
                }
                KeyCode::Char('g') => {
                    self.source_picker.selected = 0;
                    self.source_picker.scroll_offset = 0;
                }
                KeyCode::Char('G') => {
                    let last = self.source_picker.filtered.len().saturating_sub(1);
                    self.source_picker.selected = last;
                    self.source_picker.clamp_scroll();
                }
                KeyCode::Tab => {
                    self.source_picker.tab = match self.source_picker.tab {
                        SourcePickerTab::MpvSockets => SourcePickerTab::AudioFiles,
                        SourcePickerTab::AudioFiles => SourcePickerTab::SuperCollider,
                        SourcePickerTab::SuperCollider => SourcePickerTab::MpvSockets,
                    };
                    self.scan_sources();
                }
                KeyCode::BackTab => {
                    self.source_picker.tab = match self.source_picker.tab {
                        SourcePickerTab::MpvSockets => SourcePickerTab::SuperCollider,
                        SourcePickerTab::SuperCollider => SourcePickerTab::AudioFiles,
                        SourcePickerTab::AudioFiles => SourcePickerTab::MpvSockets,
                    };
                    self.scan_sources();
                }
                KeyCode::Enter => {
                    if let AppMode::SourcePicker(deck) = self.mode {
                        self.select_source_for_deck(deck);
                    }
                    self.mode = AppMode::PaneSelect;
                }
                _ => {}
            },
            PickerInputMode::Insert => match key.code {
                KeyCode::Esc => {
                    self.source_picker.input_mode = PickerInputMode::Normal;
                }
                KeyCode::Enter => {
                    if let AppMode::SourcePicker(deck) = self.mode {
                        self.select_source_for_deck(deck);
                    }
                    self.mode = AppMode::PaneSelect;
                }
                KeyCode::Backspace => {
                    self.source_picker.query.pop();
                    self.source_picker.filter();
                }
                KeyCode::Tab => {
                    self.source_picker.tab = match self.source_picker.tab {
                        SourcePickerTab::MpvSockets => SourcePickerTab::AudioFiles,
                        SourcePickerTab::AudioFiles => SourcePickerTab::SuperCollider,
                        SourcePickerTab::SuperCollider => SourcePickerTab::MpvSockets,
                    };
                    self.scan_sources();
                }
                KeyCode::BackTab => {
                    self.source_picker.tab = match self.source_picker.tab {
                        SourcePickerTab::MpvSockets => SourcePickerTab::SuperCollider,
                        SourcePickerTab::SuperCollider => SourcePickerTab::AudioFiles,
                        SourcePickerTab::AudioFiles => SourcePickerTab::MpvSockets,
                    };
                    self.scan_sources();
                }
                KeyCode::Up => {
                    self.source_picker.move_up();
                }
                KeyCode::Down => {
                    self.source_picker.move_down();
                }
                KeyCode::Char(c) => {
                    self.source_picker.query.push(c);
                    self.source_picker.filter();
                }
                _ => {}
            },
        }
    }

    /// Assign selected source to deck
    fn select_source_for_deck(&mut self, deck: Deck) {
        if let Some(item) = self.source_picker.selected_item().cloned() {
            // Deck::C uses cue_channel which is not in the channels Vec
            if deck == Deck::C {
                self.select_source_for_deck_c(&item);
                return;
            }

            let channel_idx = match deck {
                Deck::A => self.mixer.dj.deck_a_channel,
                Deck::B => self.mixer.dj.deck_b_channel,
                Deck::C => unreachable!(),
            };

            // Free old SC synths if switching away from SC source
            match deck {
                Deck::A => {
                    if let Some(ref old) = self.sc_deck_a {
                        let _ = old.free_all();
                    }
                    self.sc_deck_a = None;
                }
                Deck::B => {
                    if let Some(ref old) = self.sc_deck_b {
                        let _ = old.free_all();
                    }
                    self.sc_deck_b = None;
                }
                Deck::C => {
                    if let Some(ref old) = self.sc_deck_c {
                        let _ = old.free_all();
                    }
                    self.sc_deck_c = None;
                }
            }

            if item.is_socket {
                // MPV socket - create and connect client
                let socket_path = item.path.to_string_lossy().to_string();
                let mut client = MpvClient::new(&socket_path);

                let connected = client.connect().is_ok();

                // Update channel state
                if let Some(channel) = self.mixer.channels.get_mut(channel_idx) {
                    channel.name = item.name.clone();
                    channel.connected = connected;

                    // Sync initial volume from MPV
                    if connected {
                        if let Ok(vol) = client.get_volume() {
                            channel.fader = vol / 100.0;
                        }
                        if let Ok(paused) = client.get_pause() {
                            channel.playing = !paused;
                        }
                        // Add astats filter for real-time metering
                        let _ = client.ensure_astats();
                        client.start_metering();
                    }
                }

                // Get file path for BPM analysis before storing client
                let file_path = if connected {
                    client.get_path().ok().map(PathBuf::from)
                } else {
                    None
                };

                // Store client for this deck
                match deck {
                    Deck::A => self.mpv_deck_a = Some(client),
                    Deck::B => self.mpv_deck_b = Some(client),
                    Deck::C => self.mpv_deck_c = Some(client),
                }

                // Also add to legacy manager
                let source = AudioSource::new(item.name, socket_path);
                self.audio_manager.add_source(source);

                // Trigger BPM analysis if we have a file path
                if let Some(path) = file_path {
                    let on_result = Arc::new(Mutex::new(move |result: crate::audio::BpmResult| {
                        // BPM result will be picked up on next tick
                        tracing::debug!("BPM detected for channel {}: {:.1} (conf: {:.2})", channel_idx, result.bpm, result.confidence);
                    }));
                    BpmAnalyzer::analyze_file(&path, on_result);
                }
            } else if item.is_udp {
                // UDP source (e.g., SuperCollider) - create client and connect
                let addr = item.path.to_string_lossy().to_string();
                // Strip "udp://" prefix if present
                let addr = addr.strip_prefix("udp://").unwrap_or(&addr);
                let base_node_id = match deck {
                    Deck::A => 1000,
                    Deck::B => 2000,
                    Deck::C => 3000,
                };
                let mut client = SuperColliderClient::new(addr, base_node_id);
                let connected = client.connect().is_ok();

                if let Some(channel) = self.mixer.channels.get_mut(channel_idx) {
                    channel.name = item.name.clone();
                    channel.connected = connected;
                    channel.source_id = Some(addr.to_string());
                }

                // Send SynthDef, create monitor (bus 0→2), create group, create mixer synth
                if connected {
                    let _ = client.send_synth_def();
                    let _ = client.create_monitor_synth();
                    let _ = client.create_group();
                    let _ = client.create_synth();

                    // Sync current mixer settings to the new synth
                    if let Some(channel) = self.mixer.channels.get(channel_idx) {
                        let xf_gain = if deck == Deck::A {
                            self.calculate_crossfader_gains().0
                        } else {
                            self.calculate_crossfader_gains().1
                        };
                        let vol = (channel.fader * xf_gain * self.mixer.master.fader).clamp(0.0, 1.0);
                        let _ = client.set_volume(vol);
                        let _ = client.set_lpf(channel.lpf_freq);
                        let _ = client.set_hpf(channel.hpf_freq);
                        let _ = client.set_eq(channel.eq_low, channel.eq_mid, channel.eq_high);
                        let _ = client.set_pan(channel.pan);
                    }
                }

                // Store client for this deck
                match deck {
                    Deck::A => self.sc_deck_a = Some(client),
                    Deck::B => self.sc_deck_b = Some(client),
                    Deck::C => self.sc_deck_c = Some(client),
                }
            } else {
                // Audio file - would launch MPV with socket
                // TODO: Spawn mpv --input-ipc-server=/tmp/mpv-deck-{a|b}.sock <file>
                if let Some(channel) = self.mixer.channels.get_mut(channel_idx) {
                    channel.name = item.name;
                }
            }
        }
    }

    /// Assign selected source to Deck C (CUE channel)
    fn select_source_for_deck_c(&mut self, item: &SourcePickerItem) {
        // Free old SC synths if switching away from SC source
        if let Some(ref old) = self.sc_deck_c {
            let _ = old.free_all();
        }
        self.sc_deck_c = None;

        if item.is_socket {
            let socket_path = item.path.to_string_lossy().to_string();
            let mut client = MpvClient::new(&socket_path);
            let connected = client.connect().is_ok();

            self.mixer.cue_channel.name = item.name.clone();
            self.mixer.cue_channel.connected = connected;

            if connected {
                if let Ok(vol) = client.get_volume() {
                    self.mixer.cue_channel.fader = vol / 100.0;
                }
                if let Ok(paused) = client.get_pause() {
                    self.mixer.cue_channel.playing = !paused;
                }
                // Add astats filter for real-time metering
                let _ = client.ensure_astats();
                client.start_metering();
            }

            // Get file path for BPM analysis before storing client
            let file_path = if connected {
                client.get_path().ok().map(PathBuf::from)
            } else {
                None
            };

            self.mpv_deck_c = Some(client);

            let source = AudioSource::new(item.name.clone(), socket_path);
            self.audio_manager.add_source(source);

            // Trigger BPM analysis if we have a file path
            if let Some(path) = file_path {
                let on_result = Arc::new(Mutex::new(move |result: crate::audio::BpmResult| {
                    tracing::debug!("BPM detected for CUE: {:.1} (conf: {:.2})", result.bpm, result.confidence);
                }));
                BpmAnalyzer::analyze_file(&path, on_result);
            }
        } else if item.is_udp {
            let addr = item.path.to_string_lossy().to_string();
            let addr = addr.strip_prefix("udp://").unwrap_or(&addr);
            let mut client = SuperColliderClient::new(addr, 3000);
            let connected = client.connect().is_ok();

            self.mixer.cue_channel.name = item.name.clone();
            self.mixer.cue_channel.connected = connected;
            self.mixer.cue_channel.source_id = Some(addr.to_string());

            if connected {
                let _ = client.send_synth_def();
                let _ = client.create_monitor_synth();
                let _ = client.create_group();
                let _ = client.create_synth();

                let vol = self.mixer.cue_channel.fader;
                let _ = client.set_volume(vol);
                let _ = client.set_lpf(self.mixer.cue_channel.lpf_freq);
                let _ = client.set_hpf(self.mixer.cue_channel.hpf_freq);
                let _ = client.set_eq(self.mixer.cue_channel.eq_low, self.mixer.cue_channel.eq_mid, self.mixer.cue_channel.eq_high);
                let _ = client.set_pan(self.mixer.cue_channel.pan);
            }

            self.sc_deck_c = Some(client);
        } else {
            self.mixer.cue_channel.name = item.name.clone();
        }
    }

    /// Calculate crossfader gains for deck A and B based on current position and curve
    /// Crossfader position: -1.0 = full A, 0.0 = center (both 100%), 1.0 = full B
    fn calculate_crossfader_gains(&self) -> (f32, f32) {
        let xf = self.mixer.dj.crossfader; // -1.0 to 1.0
        // Center (0): both 100%, Left (-1): A=100% B=0%, Right (+1): A=0% B=100%
        let a = if xf <= 0.0 { 1.0 } else { 1.0 - xf };
        let b = if xf >= 0.0 { 1.0 } else { 1.0 + xf };
        (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
    }

    /// Sync volume to MPV/SC for a specific deck, combining fader, crossfader, and master
    fn sync_deck_volume(&mut self, deck_a: bool) {
        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        let master_muted = self.mixer.master.muted;
        let master = self.mixer.master.fader;
        let solo_active = self.mixer.solo_active;

        if deck_a {
            let ch = self.mixer.channels.get(self.mixer.dj.deck_a_channel);
            let fader = ch.map(|c| c.fader).unwrap_or(1.0);
            let muted = ch.map(|c| c.muted).unwrap_or(false);
            let solo = ch.map(|c| c.solo).unwrap_or(false);
            let effective_muted = master_muted || muted || (solo_active && !solo);
            // Fader: 0.0 = 0%, 0.5 = 50% (-6dB), 1.0 = 100% (0dB)
            // Master: 0.5 = 1.0x (unity), 1.0 = 2.0x (+6dB boost)
            let vol = if effective_muted { 0.0 } else {
                (fader * gain_a * master * 2.0 * 100.0).clamp(0.0, 200.0)
            };
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_volume(vol);
            }
            let sc_vol = if effective_muted { 0.0 } else {
                (fader * gain_a * master * 2.0).clamp(0.0, 2.0)
            };
            if let Some(ref client) = self.sc_deck_a {
                let _ = client.set_volume(sc_vol);
            }
        } else {
            let ch = self.mixer.channels.get(self.mixer.dj.deck_b_channel);
            let fader = ch.map(|c| c.fader).unwrap_or(1.0);
            let muted = ch.map(|c| c.muted).unwrap_or(false);
            let solo = ch.map(|c| c.solo).unwrap_or(false);
            let effective_muted = master_muted || muted || (solo_active && !solo);
            let vol = if effective_muted { 0.0 } else {
                (fader * gain_b * master * 2.0 * 100.0).clamp(0.0, 200.0)
            };
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_volume(vol);
            }
            let sc_vol = if effective_muted { 0.0 } else {
                (fader * gain_b * master * 2.0).clamp(0.0, 2.0)
            };
            if let Some(ref client) = self.sc_deck_b {
                let _ = client.set_volume(sc_vol);
            }
        }
    }

    /// Sync volume change to MPV for a channel (applies crossfader gain)
    pub fn sync_volume_to_mpv(&mut self, channel_idx: usize) {
        if channel_idx == self.mixer.dj.deck_a_channel {
            self.sync_deck_volume(true);
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            self.sync_deck_volume(false);
        }
        self.sync_capture_dsp_params();
    }

    /// Apply solo logic and sync all channels to MPV.
    /// When any deck has solo active, non-soloed decks are muted.
    /// When no deck has solo, individual mute states are used.
    pub fn sync_solo_to_all_mpv(&mut self) {
        let solo_active = self.mixer.solo_active;

        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        let master = self.mixer.master.fader;
        let deck_a_ch = self.mixer.dj.deck_a_channel;
        let deck_b_ch = self.mixer.dj.deck_b_channel;

        // Compute all values before mutable borrows
        let a_solo = self.mixer.channels.get(deck_a_ch).map(|c| c.solo).unwrap_or(false);
        let a_muted = if solo_active { !a_solo } else {
            self.mixer.channels.get(deck_a_ch).map(|c| c.muted).unwrap_or(false)
        };
        let a_fader = self.mixer.channels.get(deck_a_ch).map(|c| c.fader).unwrap_or(0.5);
        let a_vol = if a_muted { 0.0 } else {
            (a_fader * gain_a * master * 2.0 * 100.0).clamp(0.0, 200.0)
        };

        let b_solo = self.mixer.channels.get(deck_b_ch).map(|c| c.solo).unwrap_or(false);
        let b_muted = if solo_active { !b_solo } else {
            self.mixer.channels.get(deck_b_ch).map(|c| c.muted).unwrap_or(false)
        };
        let b_fader = self.mixer.channels.get(deck_b_ch).map(|c| c.fader).unwrap_or(0.5);
        let b_vol = if b_muted { 0.0 } else {
            (b_fader * gain_b * master * 2.0 * 100.0).clamp(0.0, 200.0)
        };

        let c_solo = self.mixer.cue_channel.solo;
        let c_muted = if solo_active { !c_solo } else { self.mixer.cue_channel.muted };
        let c_fader = self.mixer.cue_channel.fader;
        let c_vol = if c_muted { 0.0 } else {
            (c_fader * gain_a * master * 2.0 * 100.0).clamp(0.0, 200.0)
        };

        let mut msgs = Vec::new();
        msgs.push(format!("SOLO: active={} A:solo={} mute={} vol={:.0} B:solo={} mute={} vol={:.0}",
            solo_active, a_solo, a_muted, a_vol, b_solo, b_muted, b_vol));

        // Apply to MPV clients — collect results without holding self mutably
        let a_result = if let Some(ref mut client) = self.mpv_deck_a {
            let r1 = client.set_mute(a_muted);
            let r2 = client.set_volume(a_vol);
            let r3 = client.get_volume();
            Some((r1, r2, r3))
        } else { None };
        if let Some((r1, r2, r3)) = a_result {
            if let Err(e) = r1 { msgs.push(format!("ERR A mute: {}", e)); }
            if let Err(e) = r2 { msgs.push(format!("ERR A vol: {}", e)); }
            match r3 { Ok(v) => msgs.push(format!("A vol→{:.0}", v)), Err(e) => msgs.push(format!("A get_vol: {}", e)) }
        } else { msgs.push("NO MPV CLIENT A".to_string()); }

        let b_result = if let Some(ref mut client) = self.mpv_deck_b {
            let r1 = client.set_mute(b_muted);
            let r2 = client.set_volume(b_vol);
            let r3 = client.get_volume();
            Some((r1, r2, r3))
        } else { None };
        if let Some((r1, r2, r3)) = b_result {
            if let Err(e) = r1 { msgs.push(format!("ERR B mute: {}", e)); }
            if let Err(e) = r2 { msgs.push(format!("ERR B vol: {}", e)); }
            match r3 { Ok(v) => msgs.push(format!("B vol→{:.0}", v)), Err(e) => msgs.push(format!("B get_vol: {}", e)) }
        } else { msgs.push("NO MPV CLIENT B".to_string()); }

        let c_result = if let Some(ref mut client) = self.mpv_deck_c {
            let r1 = client.set_mute(c_muted);
            let r2 = client.set_volume(c_vol);
            Some((r1, r2))
        } else { None };
        if let Some((r1, r2)) = c_result {
            if let Err(e) = r1 { msgs.push(format!("ERR C mute: {}", e)); }
            if let Err(e) = r2 { msgs.push(format!("ERR C vol: {}", e)); }
        } else { msgs.push("NO MPV CLIENT C".to_string()); }

        if let Some(ref client) = self.sc_deck_a {
            let sc_vol = if a_muted { 0.0 } else { (a_fader * gain_a * master * 2.0).clamp(0.0, 2.0) };
            let _ = client.set_volume(sc_vol);
        }
        if let Some(ref client) = self.sc_deck_b {
            let sc_vol = if b_muted { 0.0 } else { (b_fader * gain_b * master * 2.0).clamp(0.0, 2.0) };
            let _ = client.set_volume(sc_vol);
        }

        for msg in msgs { self.log_debug(msg); }
    }

    /// Sync mute state to MPV/SC for a channel
    pub fn sync_mute_to_mpv(&mut self, channel_idx: usize) {
        let muted = self.mixer.channels.get(channel_idx)
            .map(|c| c.muted)
            .unwrap_or(false);
        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        let master = self.mixer.master.fader;

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_mute(muted);
            }
            // SuperCollider: mute by setting volume to 0, unmute restores fader level with gains
            if let Some(ref client) = self.sc_deck_a {
                let vol = if muted {
                    0.0
                } else {
                    let fader = self.mixer.channels.get(channel_idx)
                        .map(|c| c.fader)
                        .unwrap_or(0.5);
                    (fader * gain_a * master * 2.0).clamp(0.0, 2.0)
                };
                let _ = client.set_volume(vol);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_mute(muted);
            }
            if let Some(ref client) = self.sc_deck_b {
                let vol = if muted {
                    0.0
                } else {
                    let fader = self.mixer.channels.get(channel_idx)
                        .map(|c| c.fader)
                        .unwrap_or(0.5);
                    (fader * gain_b * master * 2.0).clamp(0.0, 2.0)
                };
                let _ = client.set_volume(vol);
            }
        }
        self.sync_capture_dsp_params();
    }

    /// Sync play/pause state to MPV/SC for a channel
    pub fn sync_playpause_to_mpv(&mut self, channel_idx: usize) {
        let playing = self.mixer.channels.get(channel_idx)
            .map(|c| c.playing)
            .unwrap_or(false);

        let paused = !playing;

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_pause(paused);
            }
            if let Some(ref mut client) = self.sc_deck_a {
                let _ = client.set_pause(paused);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_pause(paused);
            }
            if let Some(ref mut client) = self.sc_deck_b {
                let _ = client.set_pause(paused);
            }
        }

        // Pause/resume capture when all decks stop/any deck plays
        if let Some(ref capture) = self.audio_capture {
            let any_playing = self.mixer.channels.iter().any(|c| c.playing);
            if any_playing {
                capture.resume();
            } else {
                capture.pause();
            }
        }
    }

    /// Sync playback speed (BPM) to MPV for a channel
    pub fn sync_speed_to_mpv(&mut self, channel_idx: usize) {
        let speed = self.mixer.channels.get(channel_idx)
            .map(|c| c.playback_speed)
            .unwrap_or(1.0);

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_speed(speed);
            }
            if let Some(ref client) = self.sc_deck_a {
                let _ = client.set_speed(speed);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_speed(speed);
            }
            if let Some(ref client) = self.sc_deck_b {
                let _ = client.set_speed(speed);
            }
        }
    }

    /// Sync current control to MPV if it's a fader
    fn sync_current_control_to_mpv(&mut self) {
        match self.mixer.focus {
            SelectionFocus::Channel(ch_idx) => {
                match self.mixer.selected_control {
                    ChannelControl::Fader => {
                        self.sync_volume_to_mpv(ch_idx);
                    }
                    ChannelControl::Bpm => {
                        self.sync_speed_to_mpv(ch_idx);
                    }
                    ChannelControl::Pan => {
                        self.sync_pan_to_mpv(ch_idx);
                    }
                    ChannelControl::EqLow | ChannelControl::EqMid | ChannelControl::EqHigh => {
                        self.sync_eq_to_mpv(ch_idx);
                    }
                    ChannelControl::LowPassFilter => {
                        self.sync_lpf_to_mpv(ch_idx);
                    }
                    ChannelControl::HighPassFilter => {
                        self.sync_hpf_to_mpv(ch_idx);
                    }
                    _ => {}
                }
            }
            SelectionFocus::Global => {
                // Crossfader adjusts both deck volumes
                if self.mixer.selected_global == GlobalControl::Crossfader {
                    self.sync_crossfader_to_mpv();
                }
                // Master fader adjusts both deck volumes
                if self.mixer.selected_global == GlobalControl::MasterFader {
                    self.sync_deck_volume(true);
                    self.sync_deck_volume(false);
                }
            }
        }
    }

    /// Sync EQ to MPV for a channel
    fn sync_eq_to_mpv(&mut self, channel_idx: usize) {
        let (low, mid, high, low_kill, mid_kill, high_kill) = self.mixer.channels.get(channel_idx)
            .map(|c| (c.eq_low, c.eq_mid, c.eq_high, c.eq_low_kill, c.eq_mid_kill, c.eq_high_kill))
            .unwrap_or((0.0, 0.0, 0.0, false, false, false));

        // If kill is active, use -96dB (effectively silent)
        let effective_low = if low_kill { -96.0 } else { low };
        let effective_mid = if mid_kill { -96.0 } else { mid };
        let effective_high = if high_kill { -96.0 } else { high };

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
            }
            if let Some(ref client) = self.sc_deck_a {
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
            }
            if let Some(ref client) = self.sc_deck_b {
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
            }
        }
        self.sync_capture_dsp_params();
    }

    /// Sync LPF to MPV for a channel
    fn sync_lpf_to_mpv(&mut self, channel_idx: usize) {
        let freq = self.mixer.channels.get(channel_idx)
            .map(|c| c.lpf_freq)
            .unwrap_or(20000.0);

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_lpf(freq);
            }
            if let Some(ref client) = self.sc_deck_a {
                let _ = client.set_lpf(freq);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_lpf(freq);
            }
            if let Some(ref client) = self.sc_deck_b {
                let _ = client.set_lpf(freq);
            }
        }
        self.sync_capture_dsp_params();
    }

    /// Sync HPF to MPV/SC for a channel
    fn sync_hpf_to_mpv(&mut self, channel_idx: usize) {
        let freq = self.mixer.channels.get(channel_idx)
            .map(|c| c.hpf_freq)
            .unwrap_or(20.0);

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_hpf(freq);
            }
            if let Some(ref client) = self.sc_deck_a {
                let _ = client.set_hpf(freq);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_hpf(freq);
            }
            if let Some(ref client) = self.sc_deck_b {
                let _ = client.set_hpf(freq);
            }
        }
        self.sync_capture_dsp_params();
    }

    /// Sync pan to MPV/SC for a channel
    fn sync_pan_to_mpv(&mut self, channel_idx: usize) {
        let pan = self.mixer.channels.get(channel_idx)
            .map(|c| c.pan)
            .unwrap_or(0.0);

        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_pan(pan);
            }
            if let Some(ref client) = self.sc_deck_a {
                let _ = client.set_pan(pan);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_pan(pan);
            }
            if let Some(ref client) = self.sc_deck_b {
                let _ = client.set_pan(pan);
            }
        }
        self.sync_capture_dsp_params();
    }

    /// Sync crossfader position to both deck volumes
    fn sync_crossfader_to_mpv(&mut self) {
        // Update both decks - they each apply crossfader gain internally
        self.sync_deck_volume(true);
        self.sync_deck_volume(false);
        // Also update audio capture DSP params
        self.sync_capture_dsp_params();
    }

    /// Sync current mixer state to audio capture DSP parameters (no-op without BlackHole)
    fn sync_capture_dsp_params(&mut self) {}
    
    /// Cleanup resources before exit (clear all recording buffers)
    #[allow(dead_code)]
    pub fn cleanup(&mut self) {
        if let Some(ref mut player) = self.rack_player {
            player.clear_all_buffers();
        }
    }
    
    /// Add a debug log message (keeps last 100 messages)
    /// Only logs when DEBUG env var is set (e.g. DEBUG=1 ./tidal-mixer)
    pub fn log_debug(&mut self, msg: impl Into<String>) {
        if std::env::var("DEBUG").is_err() {
            return;
        }
        self.debug_log.push(msg.into());
        if self.debug_log.len() > 100 {
            self.debug_log.remove(0);
        }
    }
    
    /// Check if debug mode is enabled via DEBUG env var
    #[allow(dead_code)]
    pub fn is_debug_enabled() -> bool {
        std::env::var("DEBUG").is_ok()
    }
}
