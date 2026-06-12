//! Application state and event handling

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::audio::{AudioSource, AudioSourceManager, MpvClient, SampleEngine};
use crate::state::{ChannelControl, CrossfaderCurve, GlobalControl, MixerState, SamplePadGrid, SelectionFocus};

/// Which deck is being configured
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deck {
    A,
    B,
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
pub enum SourcePickerTab {
    MpvSockets,
    AudioFiles,
}

/// Source picker state
#[derive(Debug, Clone)]
pub struct SourcePickerState {
    pub tab: SourcePickerTab,
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
    pub is_dir: bool,
}

impl SourcePickerState {
    pub fn new() -> Self {
        Self {
            tab: SourcePickerTab::AudioFiles,
            query: String::new(),
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            current_dir: PathBuf::new(),
            root_dir: PathBuf::new(),
            visible_height: 12, // Default, will be updated by UI
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
    DeckB,
    Master,
}

impl SelectedPane {
    pub fn next(self) -> Self {
        match self {
            SelectedPane::DeckA => SelectedPane::DjCenter,
            SelectedPane::DjCenter => SelectedPane::DeckB,
            SelectedPane::DeckB => SelectedPane::Master,
            SelectedPane::Master => SelectedPane::DeckA,
        }
    }
    
    pub fn prev(self) -> Self {
        match self {
            SelectedPane::DeckA => SelectedPane::Master,
            SelectedPane::DjCenter => SelectedPane::DeckA,
            SelectedPane::DeckB => SelectedPane::DjCenter,
            SelectedPane::Master => SelectedPane::DeckB,
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
    // Sample playback engine (cached samples for instant playback)
    sample_engine: Option<SampleEngine>,
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
        
        // Initialize sample engine for instant playback
        let sample_engine = SampleEngine::new().ok();
        
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
            music_dir: cwd.clone(),
            samples_dir: cwd,
            selected_pad_idx: None,
            mpv_deck_a: None,
            mpv_deck_b: None,
            sample_engine,
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
        self.mixer.update_meters();
        self.sample_pads.update();
    }

    /// Check if we're in edit mode
    pub fn is_editing(&self) -> bool {
        self.mode == AppMode::Edit
    }
    
    /// Check if we're in control select mode (for highlighting)
    pub fn is_control_select(&self) -> bool {
        self.mode == AppMode::ControlSelect
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
            _ => {}
        }
        
        // Mode-specific handling
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

            // Tab or l/Right: next pane (round-robin)
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.next();
                self.sync_pane_to_mixer();
            }
            
            // Shift+Tab (BackTab) or h/Left: previous pane (round-robin)
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.prev();
                self.sync_pane_to_mixer();
            }
            
            // j/k do nothing in pane select (only horizontal navigation)
            KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Up | KeyCode::Down => {}

            // Enter: activate control select mode for this pane
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.mode = AppMode::ControlSelect;
                self.sync_pane_to_mixer();
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
            }
            SelectedPane::DeckB => {
                self.mixer.focus = SelectionFocus::Channel(self.mixer.dj.deck_b_channel);
                self.mixer.selected_channel = self.mixer.dj.deck_b_channel;
            }
            SelectedPane::DjCenter => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::Crossfader;
            }
            SelectedPane::Master => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::MasterFader;
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

            // Enter: edit selected control, toggle if button, or open sample picker for pad
            KeyCode::Enter | KeyCode::Char(' ') => {
                // Check if a pad is selected in DJ center
                if self.selected_pane == SelectedPane::DjCenter {
                    if let Some(pad_idx) = self.selected_pad_idx {
                        self.open_sample_picker(pad_idx);
                        return;
                    }
                }
                
                if self.is_current_control_continuous() {
                    self.mode = AppMode::Edit;
                } else {
                    self.toggle_current_control();
                }
            }

            // Quick toggles
            KeyCode::Char('m') => {
                if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.muted = !channel.muted;
                    }
                    self.sync_mute_to_mpv(ch_idx);
                }
            }
            KeyCode::Char('s') => {
                if let Some(channel) = self.mixer.selected_channel_mut() {
                    channel.solo = !channel.solo;
                }
                self.mixer.solo_active = self.mixer.channels.iter().any(|c| c.solo);
            }

            // Reset to default
            KeyCode::Char('0') => {
                self.reset_current_control();
            }

            // Center pan or crossfader
            KeyCode::Char('c') | KeyCode::Char('C') => {
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
            }

            _ => {}
        }
    }
    
    /// Navigate to next control down within current pane
    fn navigate_control_down(&mut self) {
        match self.selected_pane {
            SelectedPane::DeckA | SelectedPane::DeckB => {
                self.mixer.select_next_control();
            }
            SelectedPane::DjCenter => {
                // If we have a pad selected, move down in pad grid (round-robin)
                if let Some(pad_idx) = self.selected_pad_idx {
                    let row = pad_idx / 4;
                    let col = pad_idx % 4;
                    if row < 3 {
                        self.selected_pad_idx = Some((row + 1) * 4 + col);
                    } else {
                        // At bottom row, go to crossfader
                        self.selected_pad_idx = None;
                        self.mixer.selected_global = GlobalControl::Crossfader;
                    }
                } else {
                    // Navigate DJ controls (round-robin): Crossfader -> CueMix -> Pads -> Crossfader
                    match self.mixer.selected_global {
                        GlobalControl::CueMix | GlobalControl::HeadphoneVolume | GlobalControl::BoothVolume => {
                            // Enter pad grid at top row
                            self.selected_pad_idx = Some(0);
                        }
                        GlobalControl::Crossfader => {
                            // Round-robin: crossfader -> top controls
                            self.mixer.selected_global = GlobalControl::CueMix;
                        }
                        other => {
                            self.mixer.selected_global = other;
                        }
                    }
                }
            }
            SelectedPane::Master => {
                // Round-robin for master controls
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::MasterFader => GlobalControl::MasterMute,
                    GlobalControl::MasterMute => GlobalControl::MasterDim,
                    GlobalControl::MasterDim => GlobalControl::MasterMono,
                    GlobalControl::MasterMono => GlobalControl::MasterFader,
                    other => other,
                };
            }
        }
    }
    
    /// Navigate to previous control up within current pane (round-robin)
    fn navigate_control_up(&mut self) {
        match self.selected_pane {
            SelectedPane::DeckA | SelectedPane::DeckB => {
                self.mixer.select_prev_control();
            }
            SelectedPane::DjCenter => {
                if let Some(pad_idx) = self.selected_pad_idx {
                    let row = pad_idx / 4;
                    let col = pad_idx % 4;
                    if row > 0 {
                        self.selected_pad_idx = Some((row - 1) * 4 + col);
                    } else {
                        // At top row, go to CUE/PH/BT controls
                        self.selected_pad_idx = None;
                        self.mixer.selected_global = GlobalControl::CueMix;
                    }
                } else {
                    // Round-robin: CueMix -> Crossfader -> Pads -> CueMix
                    match self.mixer.selected_global {
                        GlobalControl::Crossfader => {
                            // Enter pad grid at bottom row
                            self.selected_pad_idx = Some(12); // bottom-left pad
                        }
                        GlobalControl::CueMix | GlobalControl::HeadphoneVolume | GlobalControl::BoothVolume => {
                            // Round-robin: top controls -> crossfader
                            self.mixer.selected_global = GlobalControl::Crossfader;
                        }
                        other => {
                            self.mixer.selected_global = other;
                        }
                    }
                }
            }
            SelectedPane::Master => {
                // Round-robin for master controls
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::MasterFader => GlobalControl::MasterMono,
                    GlobalControl::MasterMute => GlobalControl::MasterFader,
                    GlobalControl::MasterDim => GlobalControl::MasterMute,
                    GlobalControl::MasterMono => GlobalControl::MasterDim,
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
                    GlobalControl::CueMix => GlobalControl::BoothVolume,
                    GlobalControl::HeadphoneVolume => GlobalControl::CueMix,
                    GlobalControl::BoothVolume => GlobalControl::HeadphoneVolume,
                    other => other,
                };
            }
        } else if self.selected_pane == SelectedPane::DeckA || self.selected_pane == SelectedPane::DeckB {
            // On EQ controls, h/l can toggle between slider and kill switch
            if let Some(paired) = self.mixer.selected_control.eq_kill_pair() {
                self.mixer.selected_control = paired;
            }
        }
    }
    
    /// Navigate right within DJ center
    fn navigate_control_right(&mut self) {
        if self.selected_pane == SelectedPane::DjCenter {
            if let Some(pad_idx) = self.selected_pad_idx {
                let row = pad_idx / 4;
                let col = pad_idx % 4;
                // Round-robin: wrap from col 3 to col 0 on same row
                let new_col = if col == 3 { 0 } else { col + 1 };
                self.selected_pad_idx = Some(row * 4 + new_col);
            } else {
                // Round-robin for DJ center top controls
                self.mixer.selected_global = match self.mixer.selected_global {
                    GlobalControl::CueMix => GlobalControl::HeadphoneVolume,
                    GlobalControl::HeadphoneVolume => GlobalControl::BoothVolume,
                    GlobalControl::BoothVolume => GlobalControl::CueMix,
                    other => other,
                };
            }
        } else if self.selected_pane == SelectedPane::DeckA || self.selected_pane == SelectedPane::DeckB {
            // On EQ controls, h/l can toggle between slider and kill switch
            if let Some(paired) = self.mixer.selected_control.eq_kill_pair() {
                self.mixer.selected_control = paired;
            }
        }
    }
    
    /// Check if current control is continuous (vs toggle)
    fn is_current_control_continuous(&self) -> bool {
        match self.mixer.focus {
            SelectionFocus::Channel(_) => self.mixer.selected_control.is_continuous(),
            SelectionFocus::Global => {
                matches!(self.mixer.selected_global,
                    GlobalControl::Crossfader | GlobalControl::CueMix |
                    GlobalControl::HeadphoneVolume | GlobalControl::BoothVolume |
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
                    _ => {}
                }
            }
            SelectionFocus::Global => {
                match self.mixer.selected_global {
                    GlobalControl::CrossfaderCurve => {
                        self.mixer.dj.crossfader_curve = self.mixer.dj.crossfader_curve.next();
                    }
                    GlobalControl::MasterMute => {
                        self.mixer.master.muted = !self.mixer.master.muted;
                    }
                    GlobalControl::MasterDim => {
                        self.mixer.master.dim = !self.mixer.master.dim;
                    }
                    GlobalControl::MasterMono => {
                        self.mixer.master.mono = !self.mixer.master.mono;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Mode 3: Edit - hjkl adjusts values, Esc returns to ControlSelect
    fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit edit mode -> back to control select
            KeyCode::Esc | KeyCode::Enter => {
                self.mode = AppMode::ControlSelect;
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

            // Center (for pan/crossfader)
            KeyCode::Char('c') | KeyCode::Char('C') => {
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
                        ChannelControl::Fader => channel.fader = 0.75,
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
                    GlobalControl::Crossfader => self.mixer.dj.crossfader = 0.0,
                    GlobalControl::CueMix => self.mixer.dj.cue_mix = 0.5,
                    GlobalControl::HeadphoneVolume => self.mixer.dj.headphone_volume = 1.0,
                    GlobalControl::BoothVolume => self.mixer.dj.booth_volume = 1.0,
                    GlobalControl::MasterFader => self.mixer.master.fader = 0.75,
                    _ => {}
                }
            }
        }
    }

    /// Handle keys when sample pad mode is active
    fn handle_pad_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit pad mode
            KeyCode::Esc => {
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
            }

            // Stop all pads
            KeyCode::Char(' ') => {
                self.stop_all_samples();
            }

            // Pad trigger keys: 4567 / RTYU / FGHJ / VBNM
            KeyCode::Char(c) => {
                if let Some(pad_idx) = self.sample_pads.trigger_by_key(c) {
                    self.play_sample(pad_idx);
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
                    if let Some(ref mut engine) = self.sample_engine {
                        let _ = engine.play(sample_path);
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
    
    /// Preload all assigned samples into cache for instant playback
    pub fn preload_samples(&mut self) {
        if let Some(ref mut engine) = self.sample_engine {
            for pad in &self.sample_pads.pads {
                if let Some(ref path) = pad.sample_path {
                    if path.exists() {
                        let _ = engine.preload(path);
                    }
                }
            }
        }
    }

    /// Handle keys when sample pad config mode is active
    fn handle_pad_config_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit config mode
            KeyCode::Esc => {
                self.mode = AppMode::SamplePads;
                self.sample_pads.config_mode = false;
            }

            // Navigate pad selection with vim keys
            KeyCode::Char('h') | KeyCode::Left => {
                self.sample_pads.move_selection(0, -1);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.sample_pads.move_selection(0, 1);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.sample_pads.move_selection(1, 0);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.sample_pads.move_selection(-1, 0);
            }

            // Clear selected pad
            KeyCode::Delete | KeyCode::Backspace => {
                self.sample_pads.clear_selected_sample();
            }

            // Cycle play mode
            KeyCode::Char('m') | KeyCode::Char('M') => {
                self.sample_pads.cycle_play_mode();
            }

            // Assign sample (would open file picker in real implementation)
            KeyCode::Enter => {
                // TODO: Open file picker dialog
                // For now, just toggle config mode off
            }

            // Also allow pad keys to select in config mode
            KeyCode::Char(c) => {
                if let Some(idx) = self.sample_pads.pad_index_for_key(c) {
                    self.sample_pads.selected_pad = idx;
                }
            }

            _ => {}
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

    /// Update pad areas for mouse hit testing
    pub fn update_pad_areas(&mut self, areas: Vec<(usize, u16, u16, u16, u16)>) {
        self.pad_areas = areas;
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

    pub fn show_pads(&self) -> bool {
        matches!(self.mode, AppMode::SamplePads | AppMode::SamplePadConfig)
    }
    
    pub fn show_source_picker(&self) -> bool {
        matches!(self.mode, AppMode::SourcePicker(_))
    }
    
    /// Open source picker for specified deck
    fn open_source_picker(&mut self, deck: Deck) {
        self.source_picker = SourcePickerState::new();
        self.scan_sources();
        self.mode = AppMode::SourcePicker(deck);
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
        
        match key.code {
            // Close picker, return to control select
            KeyCode::Esc => {
                self.mode = AppMode::ControlSelect;
            }
            
            // Preview sample with Space
            KeyCode::Char(' ') => {
                self.preview_sample();
            }
            
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => {
                self.source_picker.move_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.source_picker.move_down();
            }
            
            // Select sample or enter directory
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                if let Some(item) = self.source_picker.selected_item().cloned() {
                    if item.is_dir {
                        // Enter directory
                        self.enter_sample_directory(item.path);
                    } else if let AppMode::SamplePicker(pad_idx) = self.mode {
                        // Assign sample to pad
                        self.assign_sample_to_pad(pad_idx);
                        self.mode = AppMode::ControlSelect;
                    }
                }
            }
            
            // Go up a directory (h or Left or Backspace when query empty)
            KeyCode::Char('h') | KeyCode::Left => {
                if self.source_picker.can_go_up() {
                    if let Some(parent) = self.source_picker.current_dir.parent() {
                        self.enter_sample_directory(parent.to_path_buf());
                    }
                }
            }
            
            // Backspace: delete from query, or go up if query empty
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
            
            // Type to filter
            KeyCode::Char(c) => {
                self.source_picker.query.push(c);
                self.source_picker.filter();
            }
            
            _ => {}
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
        
        match key.code {
            // Close picker
            KeyCode::Esc => {
                self.mode = AppMode::PaneSelect;
            }
            
            // Switch tabs (Tab and Shift+Tab both toggle between 2 tabs)
            KeyCode::Tab | KeyCode::BackTab => {
                self.source_picker.tab = match self.source_picker.tab {
                    SourcePickerTab::MpvSockets => SourcePickerTab::AudioFiles,
                    SourcePickerTab::AudioFiles => SourcePickerTab::MpvSockets,
                };
                self.scan_sources();
            }
            
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => {
                self.source_picker.move_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.source_picker.move_down();
            }
            
            // Select
            KeyCode::Enter => {
                if let AppMode::SourcePicker(deck) = self.mode {
                    self.select_source_for_deck(deck);
                }
                self.mode = AppMode::PaneSelect;
            }
            
            // Backspace deletes from query
            KeyCode::Backspace => {
                self.source_picker.query.pop();
                self.source_picker.filter();
            }
            
            // Type to filter
            KeyCode::Char(c) => {
                self.source_picker.query.push(c);
                self.source_picker.filter();
            }
            
            _ => {}
        }
    }
    
    /// Assign selected source to deck
    fn select_source_for_deck(&mut self, deck: Deck) {
        if let Some(item) = self.source_picker.selected_item().cloned() {
            let channel_idx = match deck {
                Deck::A => self.mixer.dj.deck_a_channel,
                Deck::B => self.mixer.dj.deck_b_channel,
            };
            
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
                    }
                }
                
                // Store client for this deck
                match deck {
                    Deck::A => self.mpv_deck_a = Some(client),
                    Deck::B => self.mpv_deck_b = Some(client),
                }
                
                // Also add to legacy manager
                let source = AudioSource::new(item.name, socket_path);
                self.audio_manager.add_source(source);
            } else {
                // Audio file - would launch MPV with socket
                // TODO: Spawn mpv --input-ipc-server=/tmp/mpv-deck-{a|b}.sock <file>
                if let Some(channel) = self.mixer.channels.get_mut(channel_idx) {
                    channel.name = item.name;
                }
            }
        }
    }
    
    /// Calculate crossfader gains for deck A and B based on current position and curve
    /// Crossfader position: -1.0 = full A, 0.0 = center (both 100%), 1.0 = full B
    fn calculate_crossfader_gains(&self) -> (f32, f32) {
        let xf = self.mixer.dj.crossfader; // -1.0 to 1.0
        
        match self.mixer.dj.crossfader_curve {
            CrossfaderCurve::Linear => {
                // Center (0): both 100%
                // Left (-1): A=100%, B=0%
                // Right (+1): A=0%, B=100%
                let a = if xf <= 0.0 { 1.0 } else { 1.0 - xf };
                let b = if xf >= 0.0 { 1.0 } else { 1.0 + xf };
                (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
            }
            CrossfaderCurve::Smooth => {
                // S-curve: smoother transitions, both full at center
                let a = if xf <= 0.0 { 
                    1.0 
                } else { 
                    let t = xf; // 0 to 1
                    (std::f32::consts::FRAC_PI_2 * t).cos()
                };
                let b = if xf >= 0.0 { 
                    1.0 
                } else { 
                    let t = -xf; // 0 to 1
                    (std::f32::consts::FRAC_PI_2 * t).cos()
                };
                (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
            }
            CrossfaderCurve::Cut => {
                // Sharp cut: stays at 100% until very edge, then drops quickly
                let cut_zone = 0.1; // Only last 10% of travel cuts
                let a = if xf <= 0.0 {
                    1.0
                } else if xf >= 1.0 - cut_zone {
                    // In cut zone: rapid fade
                    ((1.0 - xf) / cut_zone).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let b = if xf >= 0.0 {
                    1.0
                } else if xf <= -1.0 + cut_zone {
                    // In cut zone: rapid fade
                    ((1.0 + xf) / cut_zone).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                (a, b)
            }
            CrossfaderCurve::ConstantPower => {
                // Equal power: maintains perceived loudness during crossfade
                // Both at ~70.7% (-3dB) at center, summing to same loudness as one at 100%
                // But we want both at 100% at center, so we use a different curve
                let a = if xf <= 0.0 { 
                    1.0 
                } else { 
                    (std::f32::consts::FRAC_PI_2 * xf).cos().sqrt()
                };
                let b = if xf >= 0.0 { 
                    1.0 
                } else { 
                    (std::f32::consts::FRAC_PI_2 * (-xf)).cos().sqrt()
                };
                (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
            }
        }
    }
    
    /// Sync volume to MPV for a specific deck, combining fader and crossfader
    fn sync_deck_volume(&mut self, deck_a: bool) {
        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        
        if deck_a {
            let fader = self.mixer.channels.get(self.mixer.dj.deck_a_channel)
                .map(|c| c.fader)
                .unwrap_or(1.0);
            let vol = (fader * gain_a * 100.0).clamp(0.0, 100.0);
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_volume(vol);
            }
        } else {
            let fader = self.mixer.channels.get(self.mixer.dj.deck_b_channel)
                .map(|c| c.fader)
                .unwrap_or(1.0);
            let vol = (fader * gain_b * 100.0).clamp(0.0, 100.0);
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_volume(vol);
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
    }
    
    /// Sync mute state to MPV for a channel
    pub fn sync_mute_to_mpv(&mut self, channel_idx: usize) {
        let muted = self.mixer.channels.get(channel_idx)
            .map(|c| c.muted)
            .unwrap_or(false);
        
        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_mute(muted);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_mute(muted);
            }
        }
    }
    
    /// Sync play/pause state to MPV for a channel
    pub fn sync_playpause_to_mpv(&mut self, channel_idx: usize) {
        let playing = self.mixer.channels.get(channel_idx)
            .map(|c| c.playing)
            .unwrap_or(false);
        
        // MPV uses "pause" property (true = paused, false = playing)
        let paused = !playing;
        
        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_pause(paused);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_pause(paused);
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
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
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
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
            }
        }
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
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_lpf(freq);
            }
        }
    }
    
    /// Sync HPF to MPV for a channel
    fn sync_hpf_to_mpv(&mut self, channel_idx: usize) {
        let freq = self.mixer.channels.get(channel_idx)
            .map(|c| c.hpf_freq)
            .unwrap_or(20.0);
        
        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_hpf(freq);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_hpf(freq);
            }
        }
    }
    
    /// Sync pan to MPV for a channel
    fn sync_pan_to_mpv(&mut self, channel_idx: usize) {
        let pan = self.mixer.channels.get(channel_idx)
            .map(|c| c.pan)
            .unwrap_or(0.0);
        
        if channel_idx == self.mixer.dj.deck_a_channel {
            if let Some(ref mut client) = self.mpv_deck_a {
                let _ = client.set_pan(pan);
            }
        } else if channel_idx == self.mixer.dj.deck_b_channel {
            if let Some(ref mut client) = self.mpv_deck_b {
                let _ = client.set_pan(pan);
            }
        }
    }
    
    /// Sync crossfader position to both deck volumes
    fn sync_crossfader_to_mpv(&mut self) {
        // Update both decks - they each apply crossfader gain internally
        self.sync_deck_volume(true);
        self.sync_deck_volume(false);
    }
}
