//! Application state and event handling

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{AudioSource, AudioSourceManager, AudioOutput, BpmAnalyzer, MpvClient, RackPlayer, SampleEngine, SuperColliderClient};
use crate::audio::engine::AudioEngine;
use crate::state::{ChannelControl, GlobalControl, MixerState, PadControl, RackState, SamplePadGrid, SendTarget, SelectionFocus};

const SC_GAIN_BOOST: f32 = 8.0;
const SC_DEFAULT_BPM: f32 = 135.0;
const TIDAL_BPM_PATH: &str = "/tmp/termixer-bpm";

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
    DeckActions,
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
    /// Character offset for horizontal tab scrolling
    pub tab_scroll_offset: usize,
}

#[derive(Debug, Clone)]
pub struct SourcePickerItem {
    pub name: String,
    pub path: PathBuf,
    pub is_socket: bool,
    pub is_udp: bool,
    pub is_dir: bool,
    pub camelot_key: Option<String>,
}

impl SourcePickerState {
    pub fn new() -> Self {
        Self {
            tab: SourcePickerTab::AudioFiles,
            input_mode: PickerInputMode::Normal,
            query: String::new(),
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            current_dir: PathBuf::new(),
            root_dir: PathBuf::new(),
            visible_height: 12,
            tab_scroll_offset: 0,
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

    /// Cycle to the next tab (forward round-robin)
    pub fn next_tab(&mut self) {
        self.tab = match self.tab {
            SourcePickerTab::MpvSockets => SourcePickerTab::AudioFiles,
            SourcePickerTab::AudioFiles => SourcePickerTab::SuperCollider,
            SourcePickerTab::SuperCollider => SourcePickerTab::DeckActions,
            SourcePickerTab::DeckActions => SourcePickerTab::MpvSockets,
        };
        self.scroll_tab_into_view(60);
    }

    /// Cycle to the previous tab (backward round-robin)
    pub fn prev_tab(&mut self) {
        self.tab = match self.tab {
            SourcePickerTab::MpvSockets => SourcePickerTab::DeckActions,
            SourcePickerTab::DeckActions => SourcePickerTab::SuperCollider,
            SourcePickerTab::SuperCollider => SourcePickerTab::AudioFiles,
            SourcePickerTab::AudioFiles => SourcePickerTab::MpvSockets,
        };
        self.scroll_tab_into_view(60);
    }

    /// Ensure the selected tab is visible within the given viewport width.
    /// Adjusts tab_scroll_offset so the active tab label fits on screen.
    fn scroll_tab_into_view(&mut self, viewport_width: usize) {
        // Tab labels: active uses brackets (extra 2 chars), inactive padded to same width
        let tab_widths: Vec<(SourcePickerTab, usize)> = vec![
            (SourcePickerTab::MpvSockets, 16),   // " [MPV Sockets] " or "  MPV Sockets  "
            (SourcePickerTab::AudioFiles, 16),    // " [Audio Files] " or "  Audio Files  "
            (SourcePickerTab::SuperCollider, 18), // " [SuperCollider] " or "  SuperCollider  "
            (SourcePickerTab::DeckActions, 17),   // " [Deck Actions] " or "  Deck Actions  "
        ];

        // Compute x-position of each tab label
        let mut x = 0;
        let mut active_x = 0;
        let mut active_width = 16;
        for (tab, width) in &tab_widths {
            if *tab == self.tab {
                active_x = x;
                active_width = *width;
            }
            x += width;
        }

        // If active tab is before the viewport, scroll left
        if active_x < self.tab_scroll_offset {
            self.tab_scroll_offset = active_x;
        }
        // If active tab extends past the viewport, scroll right
        else if active_x + active_width > self.tab_scroll_offset + viewport_width {
            self.tab_scroll_offset = active_x + active_width - viewport_width;
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
    Crossfader,
    DeckB,
    DeckC,
    Master,
}

impl SelectedPane {
    pub fn next(self) -> Self {
        match self {
            SelectedPane::DeckA => SelectedPane::DjCenter,
            SelectedPane::DjCenter => SelectedPane::Loops,
            SelectedPane::Loops => SelectedPane::Crossfader,
            SelectedPane::Crossfader => SelectedPane::DeckB,
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
            SelectedPane::Crossfader => SelectedPane::Loops,
            SelectedPane::DeckB => SelectedPane::Crossfader,
            SelectedPane::DeckC => SelectedPane::DeckB,
            SelectedPane::Master => SelectedPane::DeckC,
        }
    }
}

/// Horizontal layout for the mixer row.
///
/// The 5 columns, left to right, are: Deck A, DJ Center (PADS), Deck B,
/// Deck C, Master. When the viewport is too narrow for everything, Master
/// drops into an overflow region to the right. Navigation reveals it by
/// scrolling the viewport left, dropping the fewest leftmost columns needed
/// so the selected column (and as many neighbours as fit) stay visible.
#[derive(Debug, Clone, Copy)]
pub struct MixerLayout {
    pub deck_a: u16,
    pub dj: u16,
    pub deck_b: u16,
    pub deck_c: u16,
    pub master: u16,
    /// First visible column index (0=DeckA, 1=DJ, 2=DeckB, 3=DeckC, 4=Master).
    pub start: u8,
    /// Last visible column index (inclusive).
    pub end: u8,
}

impl MixerLayout {
    pub fn compute(
        viewport_w: u16,
        selected: SelectedPane,
        cur_start: Option<usize>,
        cur_end: Option<usize>,
    ) -> Self {
        const DECK_MAX: u16 = 21;
        const DECK_MIN: u16 = 13;
        const MASTER: u16 = 21;
        const DJ_MIN: u16 = 25;
        const MIN_CORE: u16 = DECK_MIN * 3 + DJ_MIN; // 64

        // Minimum widths of the 5 columns, left to right.
        let mins = [DECK_MIN, DJ_MIN, DECK_MIN, DECK_MIN, MASTER];

        // Case 1: all 5 columns fit -> full layout, no scrolling.
        if viewport_w >= MIN_CORE + MASTER {
            let w = ((viewport_w - MASTER - DJ_MIN) / 3).clamp(DECK_MIN, DECK_MAX);
            return Self {
                deck_a: w,
                dj: viewport_w - w * 3 - MASTER,
                deck_b: w,
                deck_c: w,
                master: MASTER,
                start: 0,
                end: 4,
            };
        }

        // Case 2: overflow. Master is not in the normal 5-column layout.
        // Build the window of visible columns. Master always anchors the right
        // edge when selected. For other panes, center the window around the
        // selected column so it's always visible.
        let selected_col = match selected {
            SelectedPane::DeckA => 0usize,
            SelectedPane::DjCenter | SelectedPane::Loops | SelectedPane::Crossfader => 1,
            SelectedPane::DeckB => 2,
            SelectedPane::DeckC => 3,
            SelectedPane::Master => 4,
        };

        let (start, end) = if selected == SelectedPane::Master {
            // Master selected: anchor right edge at Master, grow left.
            let mut s = 4usize;
            let mut used = mins[4];
            while s > 0 {
                let cand = mins[s - 1];
                if used + cand <= viewport_w {
                    used += cand;
                    s -= 1;
                } else {
                    break;
                }
            }
            (s, 4usize)
        } else if let (Some(cs), Some(ce)) = (cur_start, cur_end) {
            // If the selected column is already visible and the window
            // fits the viewport, keep it. Otherwise recompute.
            let window_min: u16 = (cs..=ce).map(|i| mins[i]).sum();
            if selected_col >= cs && selected_col <= ce && window_min <= viewport_w {
                (cs, ce)
            } else {
                // Selection moved outside — shift window to include it,
                // preserving the current width where possible.
                let width = ce - cs;
                let (s, e) = if selected_col < cs {
                    // Shift left
                    (selected_col, selected_col + width)
                } else {
                    // Shift right
                    (selected_col.saturating_sub(width), selected_col)
                };
                let mut s = s;
                let mut e = e;
                // Clamp and shrink if needed
                s = s.min(4);
                e = e.min(4);
                // Shrink from the far edge if it doesn't fit
                while s < e && (s..=e).map(|i| mins[i]).sum::<u16>() > viewport_w {
                    if selected_col - s <= e - selected_col {
                        e -= 1;
                    } else {
                        s += 1;
                    }
                }
                (s, e)
            }
        } else {
            // No current window — find largest window around selected_col.
            let mut s = selected_col;
            let mut e = selected_col;
            let mut used = mins[selected_col];
            loop {
                let fit_left = s > 0 && used + mins[s - 1] <= viewport_w;
                let fit_right = e < 4 && used + mins[e + 1] <= viewport_w;
                if fit_left && fit_right {
                    if mins[s - 1] <= mins[e + 1] {
                        used += mins[s - 1];
                        s -= 1;
                    } else {
                        used += mins[e + 1];
                        e += 1;
                    }
                } else if fit_left {
                    used += mins[s - 1];
                    s -= 1;
                } else if fit_right {
                    used += mins[e + 1];
                    e += 1;
                } else {
                    break;
                }
            }
            (s, e)
        };

        let min_total: u16 = (start..=end).map(|i| mins[i]).sum();
        let extra = viewport_w.saturating_sub(min_total);

        let mut widths = [0u16; 5];
        if start == 4 && end == 4 {
            // Master is the only visible column — give it all the space.
            widths[4] = viewport_w;
        } else {
            // Master stays at its minimum (fits the full EQ); distribute the
            // extra space across the other visible columns.
            let non_master_visible: u16 = (start..=end).filter(|&i| i != 4).count() as u16;
            let per = extra / non_master_visible;
            let rem = extra % non_master_visible;

            let mut assigned = 0u16;
            for i in start..=end {
                widths[i] = if i == 4 {
                    MASTER
                } else {
                    let add = per + if assigned < rem { 1 } else { 0 };
                    assigned += 1;
                    mins[i] + add
                };
            }
        }
        // Dropped columns keep their minimum width (they sit off-screen left).
        widths[..start].copy_from_slice(&mins[..start]);

        Self {
            deck_a: widths[0],
            dj: widths[1],
            deck_b: widths[2],
            deck_c: widths[3],
            master: widths[4],
            start: start as u8,
            end: end as u8,
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
    // Pane areas for mouse hit testing
    crossfader_area: Option<PaneArea>,
    master_area: Option<PaneArea>,
    cue_area: Option<PaneArea>,
    loops_area: Option<PaneArea>,
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
    // Sample playback engine (cached samples for instant playback)
    sample_engine: Option<SampleEngine>,
    // Rust-native audio engine (replaces MPV/SC for DSP)
    pub audio_engine: Option<AudioEngine>,
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
    // Terminal width for horizontal pane scrolling
    pub term_width: u16,
    // Current mixer window bounds (column indices) — preserved when
    // navigating between already-visible panes.
    pub mixer_window_start: usize,
    pub mixer_window_end: usize,
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
    // Scroll offset for debug log (0 = latest, higher = older messages)
    pub debug_scroll: usize,
    // Counter for MPV state polling (poll every N ticks)
    mpv_poll_counter: u8,
    source_refresh_counter: u8,
    tidal_bpm_poll_counter: u8,
    // Timestamp (elapsed_ms) of last TUI-initiated volume push per deck (0,1,2)
    last_volume_push_ms: [u64; 3],
    // Consecutive poll failures per deck (A=0, B=1, C=2) — cleared deck after threshold
    consecutive_poll_failures: [u8; 3],
    // Pending BPM+key results from background analysis (channel_idx, bpm, key)
    pending_bpm: Arc<Mutex<Vec<(usize, f32, Option<String>)>>>,
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

/// Generic rectangular area for a pane (crossfader, master, CUE, etc.)
#[derive(Debug, Clone, Copy, Default)]
pub struct PaneArea {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl PaneArea {
    pub fn contains(&self, px: u16, py: u16) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Result of a mouse hit-test across all panes
#[derive(Debug, Clone)]
pub enum HitResult {
    /// Click on a channel strip control
    Channel(usize, ChannelControl),
    /// Click on a sample pad
    Pad(usize),
    /// Click/drag on the crossfader
    Crossfader,
    /// Click on master pane (specific control inferred from y position)
    Master,
    /// Click on CUE pane (specific control inferred from y position)
    Cue,
    /// Click on loops pane
    Loops,
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
            crossfader_area: None,
            master_area: None,
            cue_area: None,
            loops_area: None,
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
            sample_engine,
            audio_engine: None,
            rack_state: RackState::new(),
            rack_player,
            frame_counter: 0,
            elapsed_ms: 0,
            rack_scroll_offset: 0,
            terminal_height: 24,
            term_width: 0,
            mixer_window_start: 0,
            mixer_window_end: 4,
            master_output: AudioOutput::new(),
            cue_output: AudioOutput::new(),
            selected_master_output_idx: 0,
            selected_cue_output_idx: 0,
            output_picker_active: false,
            output_picker_target: OutputPickerTarget::Master,
            debug_log: Vec::new(),
            debug_scroll: 0,
            mpv_poll_counter: 0,
            source_refresh_counter: 0,
            tidal_bpm_poll_counter: 0,
            last_volume_push_ms: [0; 3],
            consecutive_poll_failures: [0; 3],
            pending_bpm: Arc::new(Mutex::new(Vec::new())),
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
        // First pass: add all sources to manager
        for (name, socket_path) in &sources {
            let source = AudioSource::new(name.clone(), socket_path.clone());
            self.audio_manager.add_source(source);
        }

        // Second pass: connect to MPV and load files into engine decoders
        let mut loaded_channels = Vec::new();
        for (i, (name, socket_path)) in sources.iter().enumerate() {
            if let Some(channel) = self.mixer.channels.get_mut(i) {
                channel.name = name.clone();
            }
            if i < 3 {
                let mut client = crate::audio::MpvClient::new(socket_path);
                if client.connect().is_ok() {
                    match client.get_path() {
                        Ok(path) => {
                            if let Some(ref engine) = self.audio_engine {
                                engine.load_file(i, path);
                                loaded_channels.push(i);
                            }
                        }
                        Err(e) => eprintln!("Audio: get_path failed for {}: {}", name, e),
                    }
                    let _ = client.ensure_astats();
                    client.start_metering();
                    match i {
                        0 => self.mpv_deck_a = Some(client),
                        1 => self.mpv_deck_b = Some(client),
                        2 => self.mpv_deck_c = Some(client),
                        _ => {}
                    }
                    if let Some(channel) = self.mixer.channels.get_mut(i) {
                        channel.connected = true;
                        channel.uses_supercollider = false;
                    }
                }
            }
        }
        // Sync crossfader/volume state to engine for all loaded channels
        for ch in &loaded_channels {
            self.sync_volume_to_mpv(*ch);
        }

        // Mute Deck A and B on startup to avoid mangled audio
        // Ensure fader is at unity gain (+0 dB) so unmuting gives immediate sound
        let deck_a_ch = self.mixer.dj.deck_a_channel;
        let deck_b_ch = self.mixer.dj.deck_b_channel;
        if let Some(ch) = self.mixer.channels.get_mut(deck_a_ch) {
            ch.muted = true;
            ch.fader = 0.5;  // Unity gain (+0 dB)
        }
        if let Some(ch) = self.mixer.channels.get_mut(deck_b_ch) {
            ch.muted = true;
            ch.fader = 0.5;  // Unity gain (+0 dB)
        }
        // Push mute state to engine
        for ch in &loaded_channels {
            self.sync_volume_to_mpv(*ch);
        }
    }

    /// Main tick - update meters, etc.
    pub fn tick(&mut self) {
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

        // Also read meters from Rust audio engine (preferred when available)
        if let Some(ref engine) = self.audio_engine {
            for i in 0..3 {
                if i < self.mixer.channels.len() {
                    let (pl, pr, rl, rr) = engine.meters[i].load();
                    if pl > 0.0 || pr > 0.0 {
                        real_channels.push((i, pl, pr, rl, rr));
                    }
                }
            }
            // Poll LFO debug from audio callback
            let lfo_line: Option<String> = engine.lfo_debug.lock().ok()
                .map(|mut s| if s.is_empty() { None } else { Some(std::mem::take(&mut *s)) })
                .unwrap_or(None);
            if let Some(line) = lfo_line {
                self.log_debug(line);
            }
        }

        self.mixer.update_meters(&real_channels);
        self.sample_pads.update();

        // Poll MPV state every 5 ticks (~250ms) for bidirectional sync
        self.mpv_poll_counter = self.mpv_poll_counter.wrapping_add(1);
        if self.mpv_poll_counter % 5 == 0 {
            self.poll_mpv_state();
        }

        self.tidal_bpm_poll_counter = self.tidal_bpm_poll_counter.wrapping_add(1);
        if self.tidal_bpm_poll_counter % 20 == 0 {
            self.poll_tidal_bpm();
        }

        // Refresh source picker every 10 ticks (~500ms) when open on MPV Sockets tab
        if matches!(self.mode, AppMode::SourcePicker(_))
            && matches!(self.source_picker.tab, SourcePickerTab::MpvSockets)
        {
            self.source_refresh_counter = self.source_refresh_counter.wrapping_add(1);
            if self.source_refresh_counter % 10 == 0 {
                // Save current selection to restore after refresh
                let prev_path = self.source_picker.filtered.get(self.source_picker.selected)
                    .and_then(|&idx| self.source_picker.items.get(idx))
                    .map(|item| item.path.clone());

                self.scan_sources();

                // Restore selection if the previously selected item still exists
                if let Some(path) = prev_path {
                    if let Some(new_idx) = self.source_picker.items.iter().position(|item| item.path == path) {
                        // Re-filter to find the new index in filtered list
                        self.source_picker.filter();
                        if let Some(filtered_idx) = self.source_picker.filtered.iter().position(|&idx| idx == new_idx) {
                            self.source_picker.selected = filtered_idx;
                        }
                    }
                }
            }
        }

        // Scrub: tick accumulation, decay speed, and poll positions
        self.tick_scrub();
        self.decay_scrub_speed();
        self.poll_scrub_positions();

        // Apply any pending BPM results from background analysis
        self.apply_pending_bpm();

        // Read real-time onset-detected BPM from MPV metering thread
        self.poll_onset_bpm();

        // Advance LFO phase and count sync ticks for all channels
        for ch in &mut self.mixer.channels {
            if ch.lfo_speed > 0.001 {
                // Start at peak when LFO activates from idle
                if ch.prev_lfo_speed <= 0.001 {
                    ch.lfo_phase = 0.25;
                }
                let freq_hz = 0.05 + ch.lfo_speed.powf(3.0) * 29.95;
                ch.lfo_phase = (ch.lfo_phase + freq_hz / 20.0) % 1.0;
            } else {
                ch.lfo_phase = 0.0;
            }
            ch.prev_lfo_speed = ch.lfo_speed;
            ch.lfo_sync_tick = ch.lfo_sync_tick.wrapping_add(1);
        }
        // Sync filters every tick — af-command updates are cheap (no graph rebuild)
        for ch_idx in 0..self.mixer.channels.len() {
            self.sync_filter_to_mpv(ch_idx);
        }
    }

    /// Poll each connected MPV deck for state changes and sync back to mixer.
    /// Reads play/pause and volume. Volume readback reverses the full formula
    /// (fader * gain * master * 2.0 * 200.0) to extract the per-channel fader.
    /// Skips volume readback for 1s after TUI pushes a change to avoid race conditions.
    /// Clears deck if socket disappears or track ends.
    fn poll_mpv_state(&mut self) {
        let deck_a_channel = self.mixer.dj.deck_a_channel;
        let deck_b_channel = self.mixer.dj.deck_b_channel;
        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        let master = self.mixer.master.fader;
        let now = self.elapsed_ms;
        let cooldown_ms = 1000;

        // Skip volume readback while solo is active — MPV volumes are muted
        // by solo logic and don't reflect true fader positions.
        let solo_active = self.mixer.solo_active;

        // Collect poll results to avoid borrow conflicts with clear_deck()
        struct PollResult {
            deck: Deck,
            pause_ok: bool,
            playing: bool,
            volume_ok: bool,
            fader: f32,
            time_pos: Option<f32>,
            duration: Option<f32>,
        }

        let mut results: Vec<PollResult> = Vec::new();

        // Read deck A
        if let Some(ref mut client) = self.mpv_deck_a {
            let mut pause_ok = false;
            let mut playing = false;
            let mut volume_ok = false;
            let mut fader = 0.0;
            if let Ok(paused) = client.get_pause() {
                playing = !paused;
                pause_ok = true;
            }
            if !solo_active && now.saturating_sub(self.last_volume_push_ms[0]) >= cooldown_ms {
                if let Ok(vol) = client.get_volume() {
                    let divisor = gain_a * master * 2.0 * 200.0;
                    if divisor > 0.0 {
                        fader = (vol / divisor).clamp(0.0, 1.0);
                        volume_ok = true;
                    }
                }
            }
            let time_pos = client.get_time_pos().ok();
            let duration = client.get_duration().ok();
            results.push(PollResult { deck: Deck::A, pause_ok, playing, volume_ok, fader, time_pos, duration });
        }

        // Read deck B
        if let Some(ref mut client) = self.mpv_deck_b {
            let mut pause_ok = false;
            let mut playing = false;
            let mut volume_ok = false;
            let mut fader = 0.0;
            if let Ok(paused) = client.get_pause() {
                playing = !paused;
                pause_ok = true;
            }
            if !solo_active && now.saturating_sub(self.last_volume_push_ms[1]) >= cooldown_ms {
                if let Ok(vol) = client.get_volume() {
                    let divisor = gain_b * master * 2.0 * 200.0;
                    if divisor > 0.0 {
                        fader = (vol / divisor).clamp(0.0, 1.0);
                        volume_ok = true;
                    }
                }
            }
            let time_pos = client.get_time_pos().ok();
            let duration = client.get_duration().ok();
            results.push(PollResult { deck: Deck::B, pause_ok, playing, volume_ok, fader, time_pos, duration });
        }

        // Read CUE (no crossfader, gain=1.0)
        if let Some(ref mut client) = self.mpv_deck_c {
            let mut pause_ok = false;
            let mut playing = false;
            let mut volume_ok = false;
            let mut fader = 0.0;
            if let Ok(paused) = client.get_pause() {
                playing = !paused;
                pause_ok = true;
            }
            if !solo_active && now.saturating_sub(self.last_volume_push_ms[2]) >= cooldown_ms {
                if let Ok(vol) = client.get_volume() {
                    let divisor = master * 2.0 * 200.0;
                    if divisor > 0.0 {
                        fader = (vol / divisor).clamp(0.0, 1.0);
                        volume_ok = true;
                    }
                }
            }
            let time_pos = client.get_time_pos().ok();
            let duration = client.get_duration().ok();
            results.push(PollResult { deck: Deck::C, pause_ok, playing, volume_ok, fader, time_pos, duration });
        }

        // Apply results and detect failures / track end
        let mut decks_to_clear: Vec<Deck> = Vec::new();

        for result in &results {
            let deck_idx = match result.deck {
                Deck::A => 0,
                Deck::B => 1,
                Deck::C => 2,
            };

            // Socket failure detection: if get_pause() failed, the socket is dead
            if !result.pause_ok {
                self.consecutive_poll_failures[deck_idx] += 1;
                if self.consecutive_poll_failures[deck_idx] >= 2 {
                    decks_to_clear.push(result.deck);
                }
                continue;
            }

            // Reset failure counter on success
            self.consecutive_poll_failures[deck_idx] = 0;

            // Apply volume/fader state
            let ch_idx = match result.deck {
                Deck::A => deck_a_channel,
                Deck::B => deck_b_channel,
                Deck::C => self.mixer.dj.deck_c_channel,
            };

            if result.deck == Deck::C {
                self.mixer.cue_channel.playing = result.playing;
                if result.volume_ok {
                    self.mixer.cue_channel.fader = result.fader;
                }
            } else if let Some(ch) = self.mixer.get_channel_mut(ch_idx) {
                ch.playing = result.playing;
                if result.volume_ok && !ch.muted {
                    ch.fader = result.fader;
                }
                if let Some(tp) = result.time_pos {
                    ch.time_pos = tp;
                }
                if let Some(dur) = result.duration {
                    ch.duration = dur;
                }

                // Track-end detection: playback stopped and position is at/near end
                // Only clear if we have a known duration and are very close to it.
                // This avoids false positives from playlists (MPV advances to next track).
                if !result.playing
                    && ch.duration > 0.0
                    && ch.time_pos >= ch.duration - 1.0
                    && ch.connected
                {
                    decks_to_clear.push(result.deck);
                }
            }
        }

        // Clear dead/ended decks (outside borrow scope)
        for deck in decks_to_clear {
            self.clear_deck(deck);
        }
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
            // Debug log scrolling (only when DEBUG=1 and log is non-empty)
            KeyCode::Char('[') if !self.debug_log.is_empty() => {
                let max_scroll = self.debug_log.len().saturating_sub(1);
                self.debug_scroll = (self.debug_scroll + 1).min(max_scroll);
                return;
            }
            KeyCode::Char(']') if !self.debug_log.is_empty() => {
                self.debug_scroll = self.debug_scroll.saturating_sub(1);
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
            // F8 toggles master play/pause from any mode
            KeyCode::F(8) => {
                self.mixer.master.playing = !self.mixer.master.playing;
                self.sync_all_playpause();
                return;
            }
            _ => {}
        }

        // Handle recording commit (global - works from any mode)
        // SPACE commits and stays in current mode
        if self.is_rack_recording() && key.code == KeyCode::Char(' ') {
            self.log_debug("Space pressed during recording - committing");
            self.commit_rack_recording();
            return;
        }
        
        // ESC during recording commits, then falls through to normal ESC handling
        if self.is_rack_recording() && key.code == KeyCode::Esc {
            self.log_debug("ESC pressed during recording - committing and exiting");
            self.commit_rack_recording();
            // Don't return - let ESC continue to be handled by mode handlers
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
                self.ensure_mixer_pane_visible();
            }

            // Shift+Tab: previous pane (round-robin)
            KeyCode::BackTab => {
                self.selected_pad_idx = None;
                self.selected_pane = self.selected_pane.prev();
                self.sync_pane_to_mixer();
                self.ensure_mixer_pane_visible();
            }

            // h/l: horizontal navigation across mixer layout
            // DeckA ↔ Crossfader ↔ DeckB ↔ DeckC ↔ Master
            KeyCode::Char('l') | KeyCode::Right => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DeckA => SelectedPane::Crossfader,
                    SelectedPane::DjCenter | SelectedPane::Loops => SelectedPane::DeckB,
                    SelectedPane::DeckB => SelectedPane::DeckC,
                    SelectedPane::DeckC => SelectedPane::Master,
                    SelectedPane::Master => SelectedPane::DeckA,
                    SelectedPane::Crossfader => SelectedPane::DeckB,
                };
                self.sync_pane_to_mixer();
                self.ensure_mixer_pane_visible();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DeckA => SelectedPane::Master,
                    SelectedPane::DjCenter | SelectedPane::Loops => SelectedPane::DeckA,
                    SelectedPane::DeckB => SelectedPane::Crossfader,
                    SelectedPane::DeckC => SelectedPane::DeckB,
                    SelectedPane::Master => SelectedPane::DeckC,
                    SelectedPane::Crossfader => SelectedPane::DeckA,
                };
                self.sync_pane_to_mixer();
                self.ensure_mixer_pane_visible();
            }

            // j/k: vertical pane navigation
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DjCenter => SelectedPane::Loops,
                    SelectedPane::Loops => SelectedPane::Crossfader,
                    SelectedPane::Crossfader => SelectedPane::DeckB,
                    SelectedPane::DeckB => SelectedPane::DeckC,
                    SelectedPane::DeckC => SelectedPane::Master,
                    SelectedPane::Master => SelectedPane::DeckA,
                    SelectedPane::DeckA => SelectedPane::DjCenter,
                };
                self.sync_pane_to_mixer();
                self.ensure_mixer_pane_visible();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_pad_idx = None;
                self.selected_pane = match self.selected_pane {
                    SelectedPane::DjCenter => SelectedPane::DeckA,
                    SelectedPane::Loops => SelectedPane::DjCenter,
                    SelectedPane::Crossfader => SelectedPane::Loops,
                    SelectedPane::DeckB => SelectedPane::Crossfader,
                    SelectedPane::DeckC => SelectedPane::DeckB,
                    SelectedPane::Master => SelectedPane::DeckC,
                    SelectedPane::DeckA => SelectedPane::Master,
                };
                self.sync_pane_to_mixer();
                self.ensure_mixer_pane_visible();
            }

            // Crossfader slam shortcuts (when Crossfader pane is selected)
            KeyCode::Char('a') => {
                if self.selected_pane == SelectedPane::Crossfader {
                    self.mixer.dj.crossfader = -1.0; // Slam to full A
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('b') => {
                if self.selected_pane == SelectedPane::Crossfader {
                    self.mixer.dj.crossfader = 1.0; // Slam to full B
                    self.sync_current_control_to_mpv();
                }
            }

            // Enter: activate control select mode for this pane
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.selected_pane == SelectedPane::Crossfader {
                    // Crossfader has one control - go straight to edit
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

            // X (shift-x): clear the focused deck
            KeyCode::Char('X') => {
                let deck = match self.selected_pane {
                    SelectedPane::DeckA => Some(Deck::A),
                    SelectedPane::DeckB => Some(Deck::B),
                    SelectedPane::DeckC => Some(Deck::C),
                    _ => None,
                };
                if let Some(deck) = deck {
                    self.clear_deck(deck);
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
                if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
                    let was_solo = self.mixer.selected_channel_mut().map(|c| c.solo).unwrap_or(false);
                    if !was_solo {
                        // Turning solo ON: save this channel's fader, restore any other soloed decks
                        self.mixer.save_fader_for_solo(ch_idx);
                        for i in 0..self.mixer.channels.len() {
                            if i != ch_idx && self.mixer.channels[i].solo {
                                self.mixer.channels[i].solo = false;
                                self.mixer.restore_fader_from_solo(i);
                            }
                        }
                        if self.mixer.cue_channel.solo {
                            self.mixer.cue_channel.solo = false;
                            self.mixer.restore_fader_from_solo(2);
                        }
                        if let Some(channel) = self.mixer.selected_channel_mut() {
                            channel.solo = true;
                        }
                    } else {
                        // Turning solo OFF: restore this channel's fader
                        if let Some(channel) = self.mixer.selected_channel_mut() {
                            channel.solo = false;
                        }
                        self.mixer.restore_fader_from_solo(ch_idx);
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
            SelectedPane::Crossfader => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::Crossfader;
            }
            SelectedPane::Master => {
                self.mixer.focus = SelectionFocus::Global;
                self.mixer.selected_global = GlobalControl::MasterPlayPause;
            }
            SelectedPane::DeckC => {
                self.mixer.focus = SelectionFocus::Channel(2);
                self.mixer.selected_channel = 2;
                self.mixer.selected_control = ChannelControl::Fader;
            }
        }
    }

    /// Scroll mixer viewport so the currently selected pane snaps to the
    /// Recompute the mixer window to ensure the selected pane is visible.
    /// Called after rendering to keep the window in sync with viewport size.
    pub fn ensure_mixer_pane_visible(&mut self) {
        let viewport_w = self.term_width.max(1) as u16;
        let layout = MixerLayout::compute(
            viewport_w,
            self.selected_pane,
            Some(self.mixer_window_start),
            Some(self.mixer_window_end),
        );
        self.mixer_window_start = layout.start as usize;
        self.mixer_window_end = layout.end as usize;
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
                    if let Some(rack_idx) = self.rack_state.selected_rack {
                        let control = self.rack_state.selected_rack_control
                            .unwrap_or(crate::state::RackControl::Volume);
                        match control {
                            crate::state::RackControl::Volume | crate::state::RackControl::Tempo => {
                                // Continuous control → enter Edit mode
                                self.mode = AppMode::Edit;
                            }
                            crate::state::RackControl::Mute => {
                                // Toggle mute
                                if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
                                    rack.mute = !rack.mute;
                                    if rack.playing {
                                        if let Some(ref mut player) = self.rack_player {
                                            let vol = if rack.mute { 0.0 } else { rack.volume };
                                            player.set_volume(rack_idx, vol);
                                        }
                                    }
                                }
                            }
                            crate::state::RackControl::PlayPause => {
                                self.toggle_rack_playback(rack_idx);
                            }
                        }
                        return;
                    }
                }
                if self.selected_pane == SelectedPane::DeckC {
                    // CUE deck controls
                    match self.mixer.selected_control {
                        ChannelControl::CueSendToA => {
                            self.mixer.send_cue_to_deck(SendTarget::A);
                            // Swap MPV clients: Deck C's source moves to Deck A
                            let old_a = self.mpv_deck_a.take();
                            self.mpv_deck_a = self.mpv_deck_c.take();
                            self.mpv_deck_c = old_a;
                            // Swap SC clients too
                            let old_sc_a = self.sc_deck_a.take();
                            self.sc_deck_a = self.sc_deck_c.take();
                            self.sc_deck_c = old_sc_a;
                            // Re-route audio devices
                            self.reroute_audio_devices_after_cue_send(true);
                            return;
                        }
                        ChannelControl::CueSendToB => {
                            self.mixer.send_cue_to_deck(SendTarget::B);
                            // Swap MPV clients: Deck C's source moves to Deck B
                            let old_b = self.mpv_deck_b.take();
                            self.mpv_deck_b = self.mpv_deck_c.take();
                            self.mpv_deck_c = old_b;
                            // Swap SC clients too
                            let old_sc_b = self.sc_deck_b.take();
                            self.sc_deck_b = self.sc_deck_c.take();
                            self.sc_deck_c = old_sc_b;
                            // Re-route audio devices
                            self.reroute_audio_devices_after_cue_send(false);
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
                    // If PlayPause on an empty deck, open source picker instead of toggling
                    if self.mixer.selected_control == ChannelControl::PlayPause {
                        if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
                            let is_empty = self.mixer.get_channel(ch_idx)
                                .map(|c| !c.connected)
                                .unwrap_or(true);
                            if is_empty {
                                let deck = if ch_idx == self.mixer.dj.deck_a_channel {
                                    Deck::A
                                } else if ch_idx == self.mixer.dj.deck_b_channel {
                                    Deck::B
                                } else if ch_idx == self.mixer.dj.deck_c_channel {
                                    Deck::C
                                } else {
                                    Deck::A // fallback
                                };
                                self.open_source_picker(deck);
                                return;
                            }
                        }
                    }
                    self.toggle_current_control();
                }
            }

            // r: start recording on selected rack
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.selected_pane == SelectedPane::Loops {
                    if let Some(rack_idx) = self.rack_state.selected_rack {
                        self.start_rack_recording(rack_idx);
                    }
                } else if self.selected_pane == SelectedPane::DeckA
                    || self.selected_pane == SelectedPane::DeckB
                    || self.selected_pane == SelectedPane::DeckC
                {
                    self.reset_deck_to_defaults();
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
                if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
                    let was_solo = self.mixer.selected_channel_mut().map(|c| c.solo).unwrap_or(false);
                    if !was_solo {
                        // Turning solo ON: save this channel's fader, restore any other soloed decks
                        self.mixer.save_fader_for_solo(ch_idx);
                        for i in 0..self.mixer.channels.len() {
                            if i != ch_idx && self.mixer.channels[i].solo {
                                self.mixer.channels[i].solo = false;
                                self.mixer.restore_fader_from_solo(i);
                            }
                        }
                        if self.mixer.cue_channel.solo {
                            self.mixer.cue_channel.solo = false;
                            self.mixer.restore_fader_from_solo(2);
                        }
                        if let Some(channel) = self.mixer.selected_channel_mut() {
                            channel.solo = true;
                        }
                    } else {
                        // Turning solo OFF: restore this channel's fader
                        if let Some(channel) = self.mixer.selected_channel_mut() {
                            channel.solo = false;
                        }
                        self.mixer.restore_fader_from_solo(ch_idx);
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
                if self.mixer.is_in_eq_or_filter_section() {
                    match self.mixer.selected_control {
                        // EQ bars: navigate down to Pan
                        ChannelControl::EqLow | ChannelControl::EqMid | ChannelControl::EqHigh => {
                            self.mixer.selected_control = ChannelControl::Pan;
                        }
                        // Filter/LFO knobs: cycle down through 4 controls
                        ChannelControl::FilterCutoff => {
                            self.mixer.selected_control = ChannelControl::FilterFreq;
                        }
                        ChannelControl::FilterFreq => {
                            self.mixer.selected_control = ChannelControl::LfoShape;
                        }
                        ChannelControl::LfoShape => {
                            self.mixer.selected_control = ChannelControl::LfoSpeed;
                        }
                        // LFO speed: navigate down to Pan
                        ChannelControl::LfoSpeed => {
                            self.mixer.selected_control = ChannelControl::Pan;
                        }
                        _ => {}
                    }
                } else {
                    match self.mixer.selected_control {
                        ChannelControl::Bpm => {
                            self.mixer.selected_control = ChannelControl::Key;
                        }
                        ChannelControl::Key => {
                            self.mixer.selected_control = ChannelControl::FilterCutoff;
                        }
                        _ => {
                            self.mixer.select_next_control(self.selected_pane == SelectedPane::DeckC);
                        }
                    }
                }
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
            SelectedPane::Crossfader => {} // Single control, nothing to navigate
            SelectedPane::Master => {
                if self.mixer.selected_global.eq_band_index().is_some() {
                    // From any EQ band: jump directly to master gain fader
                    self.mixer.selected_global = GlobalControl::MasterFader;
                } else {
                    self.mixer.selected_global = match self.mixer.selected_global {
                        GlobalControl::MasterPlayPause => GlobalControl::MasterEq32,
                        GlobalControl::MasterFader => GlobalControl::MasterMute,
                        GlobalControl::MasterMute => GlobalControl::MasterPlayPause,
                        GlobalControl::MasterOutputSelect => GlobalControl::MasterPlayPause,
                        other => other,
                    };
                }
            }
        }
    }

    /// Navigate to previous control up within current pane (round-robin)
    fn navigate_control_up(&mut self) {
        match self.selected_pane {
            SelectedPane::DeckA | SelectedPane::DeckB | SelectedPane::DeckC => {
                if self.mixer.is_in_eq_or_filter_section() {
                    match self.mixer.selected_control {
                        // EQ bars: navigate up to BPM
                        ChannelControl::EqLow | ChannelControl::EqMid | ChannelControl::EqHigh => {
                            self.mixer.selected_control = ChannelControl::Key;
                        }
                        // Key: navigate up to BPM
                        ChannelControl::Key => {
                            self.mixer.selected_control = ChannelControl::Bpm;
                        }
                        // Filter cutoff: navigate up to Key
                        ChannelControl::FilterCutoff => {
                            self.mixer.selected_control = ChannelControl::Key;
                        }
                        // Other filter/LFO knobs: cycle up (reverse)
                        ChannelControl::FilterFreq => {
                            self.mixer.selected_control = ChannelControl::FilterCutoff;
                        }
                        ChannelControl::LfoShape => {
                            self.mixer.selected_control = ChannelControl::FilterFreq;
                        }
                        ChannelControl::LfoSpeed => {
                            self.mixer.selected_control = ChannelControl::LfoShape;
                        }
                        _ => {}
                    }
                } else {
                    self.mixer.select_prev_control(self.selected_pane == SelectedPane::DeckC);
                }
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
            SelectedPane::Crossfader => {} // Single control, nothing to navigate
            SelectedPane::Master => {
                if self.mixer.selected_global.eq_band_index().is_some() {
                    // From any EQ band: jump directly to master play/pause
                    self.mixer.selected_global = GlobalControl::MasterPlayPause;
                } else {
                    self.mixer.selected_global = match self.mixer.selected_global {
                        GlobalControl::MasterPlayPause => GlobalControl::MasterOutputSelect,
                        GlobalControl::MasterFader => GlobalControl::MasterEq16k,
                        GlobalControl::MasterMute => GlobalControl::MasterFader,
                        GlobalControl::MasterOutputSelect => GlobalControl::MasterFader,
                        other => other,
                    };
                }
            }
        }
    }

    /// Navigate left within DJ center (CUE/PH/BT are horizontal, pads too)
    fn navigate_control_left(&mut self) {
        if self.selected_pane == SelectedPane::Loops {
            if self.rack_state.selected_rack.is_some() {
                self.rack_state.select_rack_control_up();
                return;
            }
        }
        if self.selected_pane == SelectedPane::Master {
            if self.mixer.selected_global.eq_band_index().is_some() {
                let eq_variants = GlobalControl::all_eq_variants();
                let current_idx = eq_variants.iter()
                    .position(|v| *v == self.mixer.selected_global)
                    .unwrap_or(0);
                let prev_idx = if current_idx == 0 { 9 } else { current_idx - 1 };
                self.mixer.selected_global = eq_variants[prev_idx];
                return;
            }
            match self.mixer.selected_global {
                GlobalControl::MasterMute => {
                    self.mixer.selected_global = GlobalControl::MasterOutputSelect;
                }
                GlobalControl::MasterOutputSelect => {
                    self.mixer.selected_global = GlobalControl::MasterMute;
                }
                GlobalControl::MasterPlayPause => {
                    self.mixer.master.playing = !self.mixer.master.playing;
                    self.sync_all_playpause();
                }
                _ => {}
            }
        } else if self.selected_pane == SelectedPane::DjCenter {
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
        } else if self.selected_pane == SelectedPane::DeckC {
            // CUE deck: special navigation for swapped layout
            if self.mixer.is_in_eq_or_filter_section() {
                // EQ/Filter section: move left
                self.mixer.selected_control = match self.mixer.selected_control {
                    // L/M/H: move left between bands
                    ChannelControl::EqLow => ChannelControl::FilterCutoff,
                    ChannelControl::EqLowKill => ChannelControl::EqLowKill,
                    ChannelControl::EqMid => ChannelControl::EqLow,
                    ChannelControl::EqMidKill => ChannelControl::EqLowKill,
                    ChannelControl::EqHigh => ChannelControl::EqMid,
                    ChannelControl::EqHighKill => ChannelControl::EqMidKill,
                    // HPF/LPF: move left to H
                    ChannelControl::FilterCutoff => ChannelControl::EqHigh,
                    ChannelControl::FilterFreq => ChannelControl::EqHigh,
                    ChannelControl::LfoShape => ChannelControl::EqHigh,
                    ChannelControl::LfoSpeed => ChannelControl::EqHigh,
                    _ => self.mixer.selected_control,
                };
            } else {
                match self.mixer.selected_control {
                    ChannelControl::Mute => {
                        // M → -> A (same visual row)
                        self.mixer.selected_control = ChannelControl::CueSendToA;
                    }
                    ChannelControl::CueSendToA => {
                        // -> A → M (same visual row)
                        self.mixer.selected_control = ChannelControl::Mute;
                    }
                    ChannelControl::Solo => {
                        // S → -> B (same visual row)
                        self.mixer.selected_control = ChannelControl::CueSendToB;
                    }
                    ChannelControl::CueSendToB => {
                        // -> B → S (same visual row)
                        self.mixer.selected_control = ChannelControl::Solo;
                    }
                    _ => {}
                }
            }
        } else if self.selected_pane == SelectedPane::DeckA || self.selected_pane == SelectedPane::DeckB {
            if self.mixer.is_in_eq_or_filter_section() {
                // EQ/Filter section: move left
                self.mixer.selected_control = match self.mixer.selected_control {
                    // L/M/H: move left between bands
                    ChannelControl::EqLow => ChannelControl::FilterCutoff,
                    ChannelControl::EqLowKill => ChannelControl::EqLowKill,
                    ChannelControl::EqMid => ChannelControl::EqLow,
                    ChannelControl::EqMidKill => ChannelControl::EqLowKill,
                    ChannelControl::EqHigh => ChannelControl::EqMid,
                    ChannelControl::EqHighKill => ChannelControl::EqMidKill,
                    // Filter: move left to H
                    ChannelControl::FilterCutoff => ChannelControl::EqHigh,
                    ChannelControl::FilterFreq => ChannelControl::EqHigh,
                    ChannelControl::LfoShape => ChannelControl::EqHigh,
                    ChannelControl::LfoSpeed => ChannelControl::EqHigh,
                    _ => self.mixer.selected_control,
                };
            } else {
                match self.mixer.selected_control {
                    ChannelControl::Mute | ChannelControl::Solo => {
                        // Toggle between Mute and Solo
                        self.mixer.selected_control = if self.mixer.selected_control == ChannelControl::Mute {
                            ChannelControl::Solo
                        } else {
                            ChannelControl::Mute
                        };
                    }
                    _ => {}
                }
            }
        }
    }

    /// Navigate right within DJ center
    fn navigate_control_right(&mut self) {
        if self.selected_pane == SelectedPane::Loops {
            if self.rack_state.selected_rack.is_some() {
                self.rack_state.select_rack_control_down();
                return;
            }
        }
        if self.selected_pane == SelectedPane::Master {
            if self.mixer.selected_global.eq_band_index().is_some() {
                let eq_variants = GlobalControl::all_eq_variants();
                let current_idx = eq_variants.iter()
                    .position(|v| *v == self.mixer.selected_global)
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % 10;
                self.mixer.selected_global = eq_variants[next_idx];
                return;
            }
            match self.mixer.selected_global {
                GlobalControl::MasterMute => {
                    self.mixer.selected_global = GlobalControl::MasterOutputSelect;
                }
                GlobalControl::MasterOutputSelect => {
                    self.mixer.selected_global = GlobalControl::MasterMute;
                }
                GlobalControl::MasterPlayPause => {
                    self.mixer.master.playing = !self.mixer.master.playing;
                    self.sync_all_playpause();
                }
                _ => {}
            }
        } else if self.selected_pane == SelectedPane::DjCenter {
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
        } else if self.selected_pane == SelectedPane::DeckC {
            // CUE deck: special navigation for swapped layout
            if self.mixer.is_in_eq_or_filter_section() {
                // EQ/Filter section: move right
                self.mixer.selected_control = match self.mixer.selected_control {
                    // L/M/H: move right between bands
                    ChannelControl::EqLow => ChannelControl::EqMid,
                    ChannelControl::EqLowKill => ChannelControl::EqMidKill,
                    ChannelControl::EqMid => ChannelControl::EqHigh,
                    ChannelControl::EqMidKill => ChannelControl::EqHighKill,
                    ChannelControl::EqHigh => ChannelControl::FilterCutoff,
                    ChannelControl::EqHighKill => ChannelControl::EqHighKill,
                    // Filter: move right to L
                    ChannelControl::FilterCutoff => ChannelControl::EqLow,
                    ChannelControl::FilterFreq => ChannelControl::EqLow,
                    ChannelControl::LfoShape => ChannelControl::EqLow,
                    ChannelControl::LfoSpeed => ChannelControl::EqLow,
                    _ => self.mixer.selected_control,
                };
            } else {
                match self.mixer.selected_control {
                    ChannelControl::Mute => {
                        // M → -> A (same visual row)
                        self.mixer.selected_control = ChannelControl::CueSendToA;
                    }
                    ChannelControl::CueSendToA => {
                        // -> A → M (same visual row)
                        self.mixer.selected_control = ChannelControl::Mute;
                    }
                    ChannelControl::Solo => {
                        // S → -> B (same visual row)
                        self.mixer.selected_control = ChannelControl::CueSendToB;
                    }
                    ChannelControl::CueSendToB => {
                        // -> B → S (same visual row)
                        self.mixer.selected_control = ChannelControl::Solo;
                    }
                    _ => {}
                }
            }
        } else if self.selected_pane == SelectedPane::DeckA || self.selected_pane == SelectedPane::DeckB {
            if self.mixer.is_in_eq_or_filter_section() {
                // EQ/Filter section: move right
                self.mixer.selected_control = match self.mixer.selected_control {
                    // L/M/H: move right between bands
                    ChannelControl::EqLow => ChannelControl::EqMid,
                    ChannelControl::EqLowKill => ChannelControl::EqMidKill,
                    ChannelControl::EqMid => ChannelControl::EqHigh,
                    ChannelControl::EqMidKill => ChannelControl::EqHighKill,
                    ChannelControl::EqHigh => ChannelControl::FilterCutoff,
                    ChannelControl::EqHighKill => ChannelControl::EqHighKill,
                    // Filter: move right to L
                    ChannelControl::FilterCutoff => ChannelControl::EqLow,
                    ChannelControl::FilterFreq => ChannelControl::EqLow,
                    ChannelControl::LfoShape => ChannelControl::EqLow,
                    ChannelControl::LfoSpeed => ChannelControl::EqLow,
                    _ => self.mixer.selected_control,
                };
            } else {
                match self.mixer.selected_control {
                    ChannelControl::Mute | ChannelControl::Solo => {
                        self.mixer.selected_control = if self.mixer.selected_control == ChannelControl::Mute {
                            ChannelControl::Solo
                        } else {
                            ChannelControl::Mute
                        };
                    }
                    _ => {}
                }
            }
        }
    }

    /// Check if current control is continuous (vs toggle)
    fn is_current_control_continuous(&self) -> bool {
        match self.mixer.focus {
            SelectionFocus::Channel(_) => self.mixer.selected_control.is_continuous(),
            SelectionFocus::Global => {
                self.mixer.selected_global.eq_band_index().is_some()
                    || matches!(self.mixer.selected_global,
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
                if control == ChannelControl::Solo {
                    let was_solo = self.mixer.get_channel(ch_idx).map(|c| c.solo).unwrap_or(false);
                    if !was_solo {
                        // Turning solo ON: save this channel's fader, restore any other soloed decks
                        self.mixer.save_fader_for_solo(ch_idx);
                        for i in 0..self.mixer.channels.len() {
                            if i != ch_idx && self.mixer.channels[i].solo {
                                self.mixer.channels[i].solo = false;
                                self.mixer.restore_fader_from_solo(i);
                            }
                        }
                        if self.mixer.cue_channel.solo {
                            self.mixer.cue_channel.solo = false;
                            self.mixer.restore_fader_from_solo(2);
                        }
                        self.mixer.toggle_selected();
                    } else {
                        // Turning solo OFF: restore this channel's fader
                        self.mixer.toggle_selected();
                        self.mixer.restore_fader_from_solo(ch_idx);
                    }
                    self.mixer.solo_active = self.mixer.channels.iter().any(|c| c.solo)
                        || self.mixer.cue_channel.solo;
                } else {
                    self.mixer.toggle_selected();
                }

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
                    GlobalControl::MasterPlayPause => {
                        self.mixer.master.playing = !self.mixer.master.playing;
                        self.sync_all_playpause();
                    }
                    _ => {}
                }
            }
        }
    }

    /// Mode 3: Edit - hjkl adjusts values, Esc returns to ControlSelect
    fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit edit mode -> back to control select (or pane select for Crossfader)
            KeyCode::Esc | KeyCode::Enter => {
                if self.selected_pane == SelectedPane::Crossfader {
                    // Crossfader skipped ControlSelect on entry, skip it on exit too
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
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(-0.05, -1.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(-1.0, false);
                } else {
                    self.mixer.adjust_selected(-0.05);
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(0.05, 1.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(1.0, false);
                } else {
                    self.mixer.adjust_selected(0.05);
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(0.05, 1.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(1.0, false);
                } else {
                    self.mixer.adjust_selected(0.05);
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(-0.05, -1.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(-1.0, false);
                } else {
                    self.mixer.adjust_selected(-0.05);
                    self.sync_current_control_to_mpv();
                }
            }

            // Coarse adjustment with Shift
            KeyCode::Char('H') => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(-0.05, -5.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(-1.0, true);
                } else if self.is_filter_selected() {
                    match self.mixer.selected_control {
                        ChannelControl::FilterCutoff => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.filter_cutoff = 0.0;
                            }
                        }
                        ChannelControl::FilterFreq => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.filter_freq = 0.0;
                            }
                        }
                        ChannelControl::LfoShape => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.lfo_shape = 0.0;
                            }
                        }
                        ChannelControl::LfoSpeed => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.lfo_speed = 0.0;
                            }
                        }
                        _ => {}
                    }
                    self.sync_current_control_to_mpv();
                } else if self.is_pan_selected() {
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.pan = -1.0;
                    }
                    self.sync_current_control_to_mpv();
                } else {
                    self.mixer.adjust_selected(-0.2);
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('L') => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(0.05, 5.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(1.0, true);
                } else if self.is_filter_selected() {
                    match self.mixer.selected_control {
                        ChannelControl::FilterCutoff => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.filter_cutoff = 1.0;
                            }
                        }
                        ChannelControl::FilterFreq => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.filter_freq = 1.0;
                            }
                        }
                        ChannelControl::LfoShape => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.lfo_shape = 1.0;
                            }
                        }
                        ChannelControl::LfoSpeed => {
                            if let Some(channel) = self.mixer.selected_channel_mut() {
                                channel.lfo_speed = 1.0;
                            }
                        }
                        _ => {}
                    }
                    self.sync_current_control_to_mpv();
                } else if self.is_pan_selected() {
                    if let Some(channel) = self.mixer.selected_channel_mut() {
                        channel.pan = 1.0;
                    }
                    self.sync_current_control_to_mpv();
                } else {
                    self.mixer.adjust_selected(0.2);
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('K') => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(0.05, 5.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(1.0, true);
                } else if self.is_volume_fader_selected() {
                    self.mixer.adjust_selected(0.25);
                } else {
                    self.mixer.adjust_selected(0.2);
                }
                self.sync_current_control_to_mpv();
            }
            KeyCode::Char('J') => {
                if self.selected_pane == SelectedPane::Loops {
                    self.adjust_rack_control(-0.05, -5.0);
                } else if self.mixer.selected_control == ChannelControl::Scrub {
                    self.start_scrub(-1.0, true);
                } else if self.is_volume_fader_selected() {
                    self.mixer.adjust_selected(-0.25);
                } else {
                    self.mixer.adjust_selected(-0.2);
                }
                self.sync_current_control_to_mpv();
            }

            // Crossfader slam shortcuts (when crossfader is active in Edit mode)
            KeyCode::Char('a') => {
                if matches!(self.mixer.focus, SelectionFocus::Global)
                    && matches!(self.mixer.selected_global, GlobalControl::Crossfader)
                {
                    self.mixer.dj.crossfader = -1.0; // Slam to full A
                    self.sync_current_control_to_mpv();
                }
            }
            KeyCode::Char('b') => {
                if matches!(self.mixer.focus, SelectionFocus::Global)
                    && matches!(self.mixer.selected_global, GlobalControl::Crossfader)
                {
                    self.mixer.dj.crossfader = 1.0; // Slam to full B
                    self.sync_current_control_to_mpv();
                }
            }

            // Reset
            KeyCode::Char('0') => {
                if self.selected_pane == SelectedPane::Loops {
                    self.reset_rack_control();
                } else {
                    self.reset_current_control();
                    self.sync_current_control_to_mpv();
                }
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
                            ChannelControl::Scrub => {
                                if let Some(channel) = self.mixer.selected_channel_mut() {
                                    channel.playback_speed = 1.0;
                                    channel.scrub_direction = 0.0;
                                    channel.scrub_speed = 0.0;
                                    channel.scrub_accumulator = 0.0;
                                }
                            }
                            ChannelControl::Bpm => {
                                if let Some(channel) = self.mixer.selected_channel_mut() {
                                    channel.target_bpm = if channel.base_bpm > 0.0 { channel.base_bpm } else { 120.0 };
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
                        ChannelControl::Scrub => {
                            channel.playback_speed = 1.0;
                            channel.scrub_direction = 0.0;
                            channel.scrub_speed = 0.0;
                            channel.scrub_accumulator = 0.0;
                        }
                        ChannelControl::Bpm => {
                            // Reset to x1.00: target_bpm = base BPM from first detection
                            channel.target_bpm = if channel.base_bpm > 0.0 { channel.base_bpm } else { 120.0 };
                        }
                        ChannelControl::Key => {
                            // Reset key to unknown (detection will re-populate)
                            channel.key = None;
                            channel.key_offset = 0;
                        }
                        ChannelControl::FilterCutoff => channel.filter_cutoff = 0.0,
                        ChannelControl::FilterFreq => channel.filter_freq = 0.5,
                        ChannelControl::LfoShape => channel.lfo_shape = 0.5,
                        ChannelControl::LfoSpeed => channel.lfo_speed = 0.0,
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
                    GlobalControl::Crossfader => self.mixer.dj.crossfader = 0.0,
                    ctrl if ctrl.eq_band_index().is_some() => {
                        let idx = ctrl.eq_band_index().unwrap();
                        self.mixer.master.master_eq[idx] = 0.0;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Reset all controls on the currently selected deck to defaults
    fn reset_deck_to_defaults(&mut self) {
        let ch_idx = match self.mixer.focus {
            SelectionFocus::Channel(ch) => ch,
            _ => return,
        };
        if let Some(channel) = self.mixer.get_channel_mut(ch_idx) {
            channel.fader = 0.5;
            channel.pan = 0.0;
            channel.eq_low = 0.0;
            channel.eq_mid = 0.0;
            channel.eq_high = 0.0;
            channel.eq_low_kill = false;
            channel.eq_mid_kill = false;
            channel.eq_high_kill = false;
            channel.filter_cutoff = 0.0;
            channel.filter_freq = 0.5;
            channel.muted = false;
            channel.solo = false;
            channel.target_bpm = if channel.base_bpm > 0.0 { channel.base_bpm } else { 120.0 };
            channel.scrub_direction = 0.0;
            channel.scrub_speed = 0.0;
            channel.scrub_accumulator = 0.0;
            channel.playback_speed = 1.0;
        }
        // Sync all controls to MPV/SC
        self.sync_volume_to_mpv(ch_idx);
        self.sync_pan_to_mpv(ch_idx);
        self.sync_eq_to_mpv(ch_idx);
        self.sync_filter_to_mpv(ch_idx);
        self.sync_mute_to_mpv(ch_idx);
        self.sync_bpm_to_mpv(ch_idx);
        self.mixer.solo_active = false;
    }

    /// Check if the currently selected control is a volume fader
    fn is_volume_fader_selected(&self) -> bool {
        match self.mixer.focus {
            SelectionFocus::Channel(_) => {
                matches!(self.mixer.selected_control, ChannelControl::Fader)
            }
            SelectionFocus::Global => {
                matches!(self.mixer.selected_global, GlobalControl::MasterFader)
            }
        }
    }

    /// Check if the currently selected control is a filter or LFO
    fn is_filter_selected(&self) -> bool {
        matches!(self.mixer.focus, SelectionFocus::Channel(_))
            && matches!(
                self.mixer.selected_control,
                ChannelControl::FilterCutoff | ChannelControl::FilterFreq |
                ChannelControl::LfoShape | ChannelControl::LfoSpeed
            )
    }

    /// Check if the currently selected control is pan
    fn is_pan_selected(&self) -> bool {
        matches!(self.mixer.focus, SelectionFocus::Channel(_))
            && matches!(self.mixer.selected_control, ChannelControl::Pan)
    }

    /// Handle keys when sample pad mode is active
    fn handle_pad_key(&mut self, key: KeyEvent) {
        match key.code {
            // Exit pad mode (recording already committed by global handler if active)
            KeyCode::Esc | KeyCode::Enter => {
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
                    let is_rack = self.is_rack_recording();
                    if let Some(ref mut engine) = self.sample_engine {
                        if is_rack {
                            let _ = engine.play_with_config_and_recording(sample_path, Some(&config), pad_idx);
                        } else {
                            let _ = engine.play_with_config(sample_path, Some(&config));
                        }
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
                    self.log_debug(format!("Stopped loop playback for rack {}", rack_idx));
                }
            } else {
                if let Some(ref mut player) = self.rack_player {
                    let volume = rack.volume;
                    let tempo = rack.tempo;
                    match player.play_loop(rack_idx, volume, tempo) {
                        Ok(_) => {
                            rack.playing = true;
                            self.log_debug(format!("Started loop playback for rack {}", rack_idx));
                        }
                        Err(e) => {
                            self.log_debug(format!("Failed to play loop {}: {}", rack_idx, e));
                        }
                    }
                } else {
                    self.log_debug("No rack player available");
                }
            }
        }
    }

    /// Adjust the currently selected rack control
    fn adjust_rack_control(&mut self, volume_delta: f32, tempo_delta: f32) {
        if let Some(rack_idx) = self.rack_state.selected_rack {
            if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
                let was_playing = rack.playing;
                match self.rack_state.selected_rack_control {
                    Some(crate::state::RackControl::Volume) => {
                        rack.volume = (rack.volume + volume_delta).clamp(0.0, 1.0);
                        if was_playing {
                            if let Some(ref mut player) = self.rack_player {
                                player.set_volume(rack_idx, rack.volume);
                            }
                        }
                    }
                    Some(crate::state::RackControl::Tempo) => {
                        rack.tempo = (rack.tempo + tempo_delta).clamp(20.0, 400.0);
                        if was_playing {
                            if let Some(ref mut player) = self.rack_player {
                                player.set_tempo(rack_idx, rack.tempo);
                            }
                        }
                    }
                    _ => {} // Mute and PlayPause are toggles, not adjustable
                }
            }
        }
    }

    /// Reset the currently selected rack control to its default value
    fn reset_rack_control(&mut self) {
        if let Some(rack_idx) = self.rack_state.selected_rack {
            if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
                let was_playing = rack.playing;
                match self.rack_state.selected_rack_control {
                    Some(crate::state::RackControl::Volume) => {
                        rack.volume = 0.8;
                        if was_playing {
                            if let Some(ref mut player) = self.rack_player {
                                player.set_volume(rack_idx, 0.8);
                            }
                        }
                    }
                    Some(crate::state::RackControl::Tempo) => {
                        rack.tempo = 120.0;
                        if was_playing {
                            if let Some(ref mut player) = self.rack_player {
                                player.set_tempo(rack_idx, 120.0);
                            }
                        }
                    }
                    _ => {}
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
            
            // Stop all samples to release Arc references before extracting recording
            if let Some(ref mut engine) = self.sample_engine {
                engine.stop_all();
            }
            
            // Stop recording and get the audio buffer (captures exact timing)
            if let Some(ref mut engine) = self.sample_engine {
                if let Some(mut recorded_audio) = engine.stop_pad_recording() {
                    if recorded_audio.is_empty() {
                        self.log_debug("Warning: Recorded audio buffer is empty (no samples triggered)");
                        // Still set an empty buffer so the rack can be played later
                        if let Some(ref mut player) = self.rack_player {
                            player.set_loop_buffer(rack_idx, recorded_audio, 44100, 2);
                        }
                    } else {
                        self.log_debug(format!("Recorded {} samples", recorded_audio.len()));
                        
                        // Normalize audio to prevent clipping
                        let max_amplitude = recorded_audio.iter()
                            .map(|&s| s.abs())
                            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                            .unwrap_or(1.0);
                        
                        if max_amplitude > 1.0 {
                            let scale = 0.95 / max_amplitude; // Scale to 95% to leave headroom
                            for sample in recorded_audio.iter_mut() {
                                *sample *= scale;
                            }
                            self.log_debug(format!("Normalized audio: peak was {:.2}, scaled by {:.2}", max_amplitude, scale));
                        }
                        
                        // Store the recorded audio in the rack player
                        if let Some(ref mut player) = self.rack_player {
                            player.set_loop_buffer(rack_idx, recorded_audio, 44100, 2);
                            
                            // Start playback
                            let (volume, tempo) = if let Some(rack) = self.rack_state.racks.get_mut(rack_idx) {
                                rack.playing = true;
                                (rack.volume, rack.tempo)
                            } else {
                                (0.8, 120.0)
                            };
                            match player.play_loop(rack_idx, volume, tempo) {
                                Ok(_) => self.log_debug(format!("Started loop playback for rack {}", rack_idx)),
                                Err(e) => self.log_debug(format!("Failed to play loop: {}", e)),
                            }
                        }
                    }
                } else {
                    self.log_debug("ERROR: stop_pad_recording() returned None - no per-pad recording was active");
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
                        
                        // Start per-pad audio recording for proper loop timing and DSP
                        if let Some(ref mut engine) = self.sample_engine {
                            engine.start_pad_recording(44100, 2);  // Per-pad recording
                            self.log_debug("Started per-pad audio recording");
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
                if let Some(hit) = self.hit_test_all(mouse.column, mouse.row) {
                    match hit {
                        HitResult::Channel(channel_idx, control) => {
                            self.mixer.selected_channel = channel_idx;
                            self.mixer.selected_control = control;
                            self.mixer.focus = SelectionFocus::Channel(channel_idx);
                            self.selected_pane = match channel_idx {
                                0 => SelectedPane::DeckA,
                                _ => SelectedPane::DeckB,
                            };

                            if control.is_continuous() {
                                self.drag_start_y = Some(mouse.row);
                                self.drag_start_value = self.get_current_control_value();
                            } else {
                                self.mixer.toggle_selected();
                            }
                        }
                        HitResult::Pad(pad_idx) => {
                            // Switch to DJ center pane and trigger pad
                            self.selected_pane = SelectedPane::DjCenter;
                            self.sample_pads.trigger_pad(pad_idx);
                            self.play_sample(pad_idx);
                        }
                        HitResult::Crossfader => {
                            self.selected_pane = SelectedPane::Crossfader;
                            self.mixer.focus = SelectionFocus::Global;
                            self.mixer.selected_global = GlobalControl::Crossfader;
                            // Click-to-position: calculate crossfader value from x
                            if let Some(area) = &self.crossfader_area {
                                let inner_x = mouse.column.saturating_sub(area.x);
                                let pos = (inner_x as f32 / area.w as f32) * 2.0 - 1.0;
                                self.mixer.dj.crossfader = pos.clamp(-1.0, 1.0);
                                self.drag_start_x = Some(mouse.row);
                                self.drag_start_value = Some(self.mixer.dj.crossfader);
                            }
                        }
                        HitResult::Master => {
                            self.selected_pane = SelectedPane::Master;
                            self.mixer.focus = SelectionFocus::Global;
                            // Determine which control based on y position within master area
                            if let Some(area) = &self.master_area {
                                let rel_y = mouse.row.saturating_sub(area.y);
                                let h = area.h;
                                if rel_y <= 4 {
                                    // Play/pause area
                                    self.mixer.selected_global = GlobalControl::MasterPlayPause;
                                    self.mixer.master.playing = !self.mixer.master.playing;
                                    self.sync_all_playpause();
                                } else if rel_y >= h.saturating_sub(3) {
                                    // Button row (M / OUT)
                                    let mid_x = area.x + area.w / 2;
                                    if mouse.column < mid_x {
                                        self.mixer.selected_global = GlobalControl::MasterMute;
                                        self.mixer.master.muted = !self.mixer.master.muted;
                                    } else {
                                        self.mixer.selected_global = GlobalControl::MasterOutputSelect;
                                    }
                                } else {
                                    // Fader area
                                    self.mixer.selected_global = GlobalControl::MasterFader;
                                    self.drag_start_y = Some(mouse.row);
                                    self.drag_start_value = Some(self.mixer.master.fader);
                                }
                            }
                        }
                        HitResult::Cue => {
                            self.selected_pane = SelectedPane::DeckC;
                            self.mixer.focus = SelectionFocus::Channel(2);
                            // Determine control from y position
                            if let Some(area) = &self.cue_area {
                                let rel_y = mouse.row.saturating_sub(area.y);
                                let h = area.h;
                                let controls_h = h.saturating_sub(4); // approximate
                                if rel_y > controls_h {
                                    // Button area at bottom
                                    let mid_x = area.x + area.w / 2;
                                    if mouse.column < mid_x {
                                        self.mixer.selected_control = ChannelControl::CueSendToA;
                                    } else {
                                        self.mixer.selected_control = ChannelControl::CueSendToB;
                                    }
                                    self.mixer.toggle_selected();
                                } else {
                                    // Fader area
                                    self.mixer.selected_control = ChannelControl::Fader;
                                    self.drag_start_y = Some(mouse.row);
                                    self.drag_start_value = self.get_current_control_value();
                                }
                            }
                        }
                        HitResult::Loops => {
                            self.selected_pane = SelectedPane::Loops;
                        }
                    }
                }
            }

            MouseEventKind::Drag(MouseButton::Left) => {
                // Handle crossfader horizontal drag
                if self.selected_pane == SelectedPane::Crossfader {
                    if let (Some(start_x), Some(start_value), Some(area)) =
                        (self.drag_start_x, self.drag_start_value, &self.crossfader_area)
                    {
                        let delta = (mouse.column as i16 - start_x as i16) as f32;
                        let sensitivity = 2.0 / area.w as f32;
                        self.mixer.dj.crossfader =
                            (start_value + delta * sensitivity).clamp(-1.0, 1.0);
                        return;
                    }
                }

                // Handle master fader vertical drag
                if self.selected_pane == SelectedPane::Master
                    && self.mixer.selected_global == GlobalControl::MasterFader
                {
                    if let (Some(start_y), Some(start_value)) =
                        (self.drag_start_y, self.drag_start_value)
                    {
                        let delta = (start_y as i16 - mouse.row as i16) as f32;
                        let sensitivity = 0.02;
                        self.mixer.master.fader =
                            (start_value + delta * sensitivity).clamp(0.0, 1.0);
                        return;
                    }
                }

                // Handle CUE fader vertical drag
                if self.selected_pane == SelectedPane::DeckC
                    && self.mixer.selected_control == ChannelControl::Fader
                {
                    if let (Some(start_y), Some(start_value)) =
                        (self.drag_start_y, self.drag_start_value)
                    {
                        let delta = (start_y as i16 - mouse.row as i16) as f32;
                        let sensitivity = 0.02;
                        if let Some(ch) = self.mixer.channels.get_mut(2) {
                            ch.fader =
                                (start_value + delta * sensitivity).clamp(0.0, 1.0);
                        }
                        return;
                    }
                }

                // Channel strip drag
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
                            ChannelControl::FilterCutoff => {
                                channel.filter_cutoff = (start_value + delta * sensitivity * 0.5).clamp(0.0, 1.0);
                            }
                            ChannelControl::FilterFreq => {
                                channel.filter_freq = (start_value + delta * sensitivity * 0.5).clamp(0.0, 1.0);
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
                // Release gate-mode pads (areas are in screen coordinates)
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

    /// Update pane areas for mouse hit testing
    pub fn update_pane_areas(
        &mut self,
        crossfader: Option<PaneArea>,
        master: Option<PaneArea>,
        cue: Option<PaneArea>,
        loops: Option<PaneArea>,
        pads: Vec<(usize, u16, u16, u16, u16)>,
    ) {
        self.crossfader_area = crossfader;
        self.master_area = master;
        self.cue_area = cue;
        self.loops_area = loops;
        self.pad_areas = pads;
    }

    /// General hit-test across all panes
    pub fn hit_test_all(&self, x: u16, y: u16) -> Option<HitResult> {
        // Areas are in screen coordinates (matching the renderer), so use
        // mouse position directly — no scroll offset needed.
        // Check channel strips first
        if let Some((idx, ctrl)) = self.hit_test(x, y) {
            return Some(HitResult::Channel(idx, ctrl));
        }
        // Check pads
        for &(pad_idx, px, py, pw, ph) in &self.pad_areas {
            if x >= px && x < px + pw && y >= py && y < py + ph {
                return Some(HitResult::Pad(pad_idx));
            }
        }
        // Check crossfader
        if let Some(area) = &self.crossfader_area {
            if area.contains(x, y) {
                return Some(HitResult::Crossfader);
            }
        }
        // Check master
        if let Some(area) = &self.master_area {
            if area.contains(x, y) {
                return Some(HitResult::Master);
            }
        }
        // Check CUE
        if let Some(area) = &self.cue_area {
            if area.contains(x, y) {
                return Some(HitResult::Cue);
            }
        }
        // Check loops
        if let Some(area) = &self.loops_area {
            if area.contains(x, y) {
                return Some(HitResult::Loops);
            }
        }
        None
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
            ChannelControl::FilterCutoff => ch.filter_cutoff,
            ChannelControl::FilterFreq => ch.filter_freq,
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

        // Try to get device list from a connected MPV client first.
        // Falls back to cpal enumeration if no clients are available.
        let mpv_devices = self.mpv_deck_a.as_mut()
            .or(self.mpv_deck_b.as_mut())
            .and_then(|c| c.get_audio_device_list().ok());

        match target {
            OutputPickerTarget::Master => {
                if let Some(devices) = &mpv_devices {
                    let pairs: Vec<_> = devices.iter()
                        .map(|d| (d.name.clone(), d.description.clone()))
                        .collect();
                    self.master_output.set_devices_from_mpv(&pairs);
                } else {
                    self.master_output.refresh_devices();
                }
                self.selected_master_output_idx = 0;
            }
            OutputPickerTarget::Cue => {
                if let Some(devices) = &mpv_devices {
                    let pairs: Vec<_> = devices.iter()
                        .map(|d| (d.name.clone(), d.description.clone()))
                        .collect();
                    self.cue_output.set_devices_from_mpv(&pairs);
                } else {
                    self.cue_output.refresh_devices();
                }
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

        if let Some(display_name) = devices.get(selected_idx) {
            let mpv_name = match self.output_picker_target {
                OutputPickerTarget::Master => {
                    self.master_output.select_device(display_name).ok().flatten()
                }
                OutputPickerTarget::Cue => {
                    self.cue_output.select_device(display_name).ok().flatten()
                }
            };

            // Route audio to the selected device.
            // Master → Deck A + Deck B (main speakers)
            // CUE → Deck C only (headphone preview)
            if let Some(ref mpv_dev) = mpv_name {
                match self.output_picker_target {
                    OutputPickerTarget::Master => {
                        if let Some(client) = self.mpv_deck_a.as_mut() {
                            client.set_audio_device(mpv_dev).ok();
                        }
                        if let Some(client) = self.mpv_deck_b.as_mut() {
                            client.set_audio_device(mpv_dev).ok();
                        }
                    }
                    OutputPickerTarget::Cue => {
                        if let Some(client) = self.mpv_deck_c.as_mut() {
                            client.set_audio_device(mpv_dev).ok();
                        }
                    }
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
            SourcePickerTab::DeckActions => {
                self.scan_deck_actions();
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
                            camelot_key: None,
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
                            camelot_key: None,
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
                                camelot_key: None,
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
            camelot_key: None,
        });
    }

    fn scan_deck_actions(&mut self) {
        let decks = [
            (Deck::A, "A", self.mpv_deck_a.is_some() || self.sc_deck_a.is_some()),
            (Deck::B, "B", self.mpv_deck_b.is_some() || self.sc_deck_b.is_some()),
            (Deck::C, "C", self.mpv_deck_c.is_some() || self.sc_deck_c.is_some()),
        ];
        for (_deck, label, connected) in &decks {
            let status = if *connected { "●" } else { "○" };
            self.source_picker.items.push(SourcePickerItem {
                name: format!("{} Clear Deck {}", status, label),
                path: PathBuf::new(),
                is_socket: false,
                is_udp: false,
                is_dir: false,
                camelot_key: None,
            });
        }
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
                camelot_key: None,
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
                        camelot_key: None,
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
                                camelot_key: None,
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
                    self.mode = AppMode::SamplePadConfig;
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
                            self.mode = AppMode::SamplePadConfig;
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
                            self.mode = AppMode::SamplePadConfig;
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
                KeyCode::Char('h') | KeyCode::Left => {
                    self.source_picker.prev_tab();
                    self.scan_sources();
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.source_picker.next_tab();
                    self.scan_sources();
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
                    self.source_picker.next_tab();
                    self.scan_sources();
                }
                KeyCode::BackTab => {
                    self.source_picker.prev_tab();
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
                    self.source_picker.next_tab();
                    self.scan_sources();
                }
                KeyCode::BackTab => {
                    self.source_picker.prev_tab();
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

    /// Fully reset a deck: drop MPV/SC client, reset channel state to defaults.
    fn clear_deck(&mut self, deck: Deck) {
        let ch_idx = match deck {
            Deck::A => self.mixer.dj.deck_a_channel,
            Deck::B => self.mixer.dj.deck_b_channel,
            Deck::C => self.mixer.dj.deck_c_channel,
        };

        // Mute and drop MPV client
        match deck {
            Deck::A => {
                if let Some(ref mut client) = self.mpv_deck_a {
                    let _ = client.set_mute(true);
                }
                self.mpv_deck_a = None;
            }
            Deck::B => {
                if let Some(ref mut client) = self.mpv_deck_b {
                    let _ = client.set_mute(true);
                }
                self.mpv_deck_b = None;
            }
            Deck::C => {
                if let Some(ref mut client) = self.mpv_deck_c {
                    let _ = client.set_mute(true);
                }
                self.mpv_deck_c = None;
            }
        }

        // Free SC synths
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

        // Stop Rust audio engine decoder
        if let Some(ref engine) = self.audio_engine {
            engine.stop_decoder(ch_idx);
        }
        // Reset channel to defaults (preserves name and index)
        if let Some(ch) = self.mixer.channels.get_mut(ch_idx) {
            let name = ch.name.clone();
            let index = ch.index;
            *ch = crate::state::MixerChannel::new(name, index);
        }
    }

    /// Assign selected source to deck
    fn select_source_for_deck(&mut self, deck: Deck) {
        // Handle DeckActions tab — clear the selected deck
        if self.source_picker.tab == SourcePickerTab::DeckActions {
            if let Some(item) = self.source_picker.selected_item().cloned() {
                if item.name.contains("Clear Deck A") {
                    self.clear_deck(Deck::A);
                } else if item.name.contains("Clear Deck B") {
                    self.clear_deck(Deck::B);
                } else if item.name.contains("Clear Deck C") {
                    self.clear_deck(Deck::C);
                }
            }
            self.mode = AppMode::PaneSelect;
            return;
        }

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
            // Also stop MPV decoder and mute MPV client if switching away from MPV
            match deck {
                Deck::A => {
                    if let Some(ref old) = self.sc_deck_a {
                        let _ = old.free_all();
                    }
                    self.sc_deck_a = None;
                    if let Some(ref mut client) = self.mpv_deck_a {
                        let _ = client.set_mute(true);
                    }
                    self.mpv_deck_a = None;
                    if let Some(ref engine) = self.audio_engine {
                        engine.stop_decoder(channel_idx);
                    }
                }
                Deck::B => {
                    if let Some(ref old) = self.sc_deck_b {
                        let _ = old.free_all();
                    }
                    self.sc_deck_b = None;
                    if let Some(ref mut client) = self.mpv_deck_b {
                        let _ = client.set_mute(true);
                    }
                    self.mpv_deck_b = None;
                    if let Some(ref engine) = self.audio_engine {
                        engine.stop_decoder(channel_idx);
                    }
                }
                Deck::C => {
                    if let Some(ref old) = self.sc_deck_c {
                        let _ = old.free_all();
                    }
                    self.sc_deck_c = None;
                    if let Some(ref mut client) = self.mpv_deck_c {
                        let _ = client.set_mute(true);
                    }
                    self.mpv_deck_c = None;
                    if let Some(ref engine) = self.audio_engine {
                        engine.stop_decoder(channel_idx);
                    }
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
                    channel.base_bpm = 0.0; // Reset for new track detection
                    channel.uses_supercollider = false;

                    // Sync initial volume from MPV
                    if connected {
                        if let Ok(vol) = client.get_volume() {
                            channel.fader = vol / 200.0;
                        }
                        if let Ok(paused) = client.get_pause() {
                            channel.playing = !paused;
                        }
                        // Add astats filter for real-time metering
                        let _ = client.ensure_astats();
                        client.start_metering();

                        // Query MPV metadata for key info (fast, no file decode needed)
                        if let Some(key) = client.get_key_from_metadata() {
                            tracing::debug!("Got key from MPV metadata for ch{}: {}", channel_idx, key);
                            channel.key = Some(key);
                            channel.key_offset = 0;
                        }
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

                // Load file into Rust audio engine decoder (always want engine processing)
                if let Some(ref engine) = self.audio_engine {
                    if let Some(ref path) = file_path {
                        let path_str = path.to_string_lossy().to_string();
                        engine.load_file(channel_idx, path_str);
                    }
                }

                // Sync crossfader/volume state to engine for new source
                self.sync_volume_to_mpv(channel_idx);

                // Trigger BPM+key analysis if we have a file path
                if let Some(ref path) = file_path {
                    let pending = self.pending_bpm.clone();
                    let on_result = Arc::new(Mutex::new(move |result: crate::audio::BpmResult| {
                        tracing::debug!("BPM detected for channel {}: {:.1} (conf: {:.2}), key: {:?}", channel_idx, result.bpm, result.confidence, result.key);
                        if let Ok(mut queue) = pending.lock() {
                            tracing::debug!("Pushing to pending_bpm: ch={}, key={:?}", channel_idx, result.key);
                            queue.push((channel_idx, result.bpm, result.key));
                        }
                    }));
                    BpmAnalyzer::analyze_file(path, on_result);
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
                let input_bus = 4;
                let mut client = SuperColliderClient::new(addr, base_node_id, input_bus);
                let connected = client.connect().is_ok();

                if let Some(channel) = self.mixer.get_channel_mut(channel_idx) {
                    channel.name = item.name.clone();
                    channel.connected = connected;
                    channel.playing = connected;
                    channel.bpm = Some(SC_DEFAULT_BPM);
                    channel.base_bpm = SC_DEFAULT_BPM;
                    channel.target_bpm = SC_DEFAULT_BPM;
                    channel.source_id = Some(addr.to_string());
                    channel.uses_supercollider = true;
                    channel.scrub_direction = 0.0;
                    channel.scrub_speed = 0.0;
                    channel.scrub_accumulator = 0.0;
                }

                // SuperDirt composite output routes to bus 4; whichever deck selects SC controls it.
                if connected {
                    let _ = client.send_synth_def();
                    let _ = client.create_group();
                    let _ = client.create_synth();

                    // Sync current mixer settings to the new synth
                    if let Some(channel) = self.mixer.get_channel(channel_idx) {
                        let xf_gain = if deck == Deck::A {
                            self.calculate_crossfader_gains().0
                        } else {
                            self.calculate_crossfader_gains().1
                        };
                        let vol = (channel.fader * xf_gain * self.mixer.master.fader * 2.0 * SC_GAIN_BOOST).clamp(0.0, 8.0);
                        let _ = client.set_volume(vol);
                        // Apply unified filter (crossfade between LPF and HPF)
                        let cutoff = channel.filter_cutoff;
                        let freq_pos = channel.filter_freq;
                        let intensity = cutoff.powf(1.2);
                        let log_min = 20f32.log10();
                        let log_max = 20000f32.log10();
                        let actual_freq = 10f32.powf(log_min + freq_pos * (log_max - log_min));

                        let blend = if actual_freq <= 300.0 {
                            0.0
                        } else if actual_freq >= 3000.0 {
                            1.0
                        } else {
                            let t = (actual_freq - 300.0) / (3000.0 - 300.0);
                            t * t * (3.0 - 2.0 * t)
                        };

                        let lpf_target = actual_freq + (20000.0 - actual_freq) * blend;
                        let hpf_target = 20.0 + (actual_freq - 20.0) * blend;
                        let effective_lpf = 20000.0 - (20000.0 - lpf_target) * intensity;
                        let soft_lpf = if effective_lpf < 1000.0 {
                            let norm = ((effective_lpf - 200.0) / 800.0).clamp(0.0, 1.0);
                            200.0 + 800.0 * norm.sqrt()
                        } else {
                            effective_lpf
                        };
                        let effective_hpf = 20.0 + (hpf_target - 20.0) * intensity;
                        let _ = client.set_lpf(soft_lpf.clamp(200.0, 20000.0));
                        let _ = client.set_hpf(effective_hpf.clamp(20.0, 8000.0));
                        // Apply kill switches: extreme cut values
                        let effective_low = if channel.eq_low_kill { -24.0 } else { channel.eq_low };
                        let effective_mid = if channel.eq_mid_kill { -24.0 } else { channel.eq_mid };
                        let effective_high = if channel.eq_high_kill { -24.0 } else { channel.eq_high };
                        let _ = client.set_eq(effective_low, effective_mid, effective_high);
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
                    channel.uses_supercollider = false;
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
        // Stop MPV decoder and mute MPV client if switching sources
        if let Some(ref mut client) = self.mpv_deck_c {
            let _ = client.set_mute(true);
        }
        self.mpv_deck_c = None;
        if let Some(ref engine) = self.audio_engine {
            engine.stop_decoder(self.mixer.dj.deck_c_channel);
        }

        if item.is_socket {
            let socket_path = item.path.to_string_lossy().to_string();
            let mut client = MpvClient::new(&socket_path);
            let connected = client.connect().is_ok();

            self.mixer.cue_channel.name = item.name.clone();
            self.mixer.cue_channel.connected = connected;
            self.mixer.cue_channel.base_bpm = 0.0; // Reset for new track
            self.mixer.cue_channel.uses_supercollider = false;

            if connected {
                if let Ok(vol) = client.get_volume() {
                    self.mixer.cue_channel.fader = vol / 200.0;
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

            // Trigger BPM+key analysis if we have a file path
            if let Some(path) = file_path {
                let pending = self.pending_bpm.clone();
                let channel_idx = self.mixer.dj.deck_c_channel;
                let on_result = Arc::new(Mutex::new(move |result: crate::audio::BpmResult| {
                    tracing::debug!("BPM detected for CUE: {:.1} (conf: {:.2}), key: {:?}", result.bpm, result.confidence, result.key);
                    if let Ok(mut queue) = pending.lock() {
                        queue.push((channel_idx, result.bpm, result.key));
                    }
                }));
                BpmAnalyzer::analyze_file(&path, on_result);
            }
        } else if item.is_udp {
            let addr = item.path.to_string_lossy().to_string();
            let addr = addr.strip_prefix("udp://").unwrap_or(&addr);
            let mut client = SuperColliderClient::new(addr, 3000, 4);
            let connected = client.connect().is_ok();

            self.mixer.cue_channel.name = item.name.clone();
            self.mixer.cue_channel.connected = connected;
            self.mixer.cue_channel.playing = connected;
            self.mixer.cue_channel.bpm = Some(SC_DEFAULT_BPM);
            self.mixer.cue_channel.base_bpm = SC_DEFAULT_BPM;
            self.mixer.cue_channel.target_bpm = SC_DEFAULT_BPM;
            self.mixer.cue_channel.source_id = Some(addr.to_string());
            self.mixer.cue_channel.uses_supercollider = true;
            self.mixer.cue_channel.scrub_direction = 0.0;
            self.mixer.cue_channel.scrub_speed = 0.0;
            self.mixer.cue_channel.scrub_accumulator = 0.0;

            if connected {
                let _ = client.send_synth_def();
                let _ = client.create_group();
                let _ = client.create_synth();

                let vol = (self.mixer.cue_channel.fader * self.mixer.master.fader * 2.0 * SC_GAIN_BOOST).clamp(0.0, 8.0);
                let _ = client.set_volume(vol);
                // Apply unified filter for CUE channel (crossfade between LPF and HPF)
                let cutoff = self.mixer.cue_channel.filter_cutoff;
                let freq_pos = self.mixer.cue_channel.filter_freq;
                let intensity = cutoff.powf(1.2);
                let log_min = 20f32.log10();
                let log_max = 20000f32.log10();
                let actual_freq = 10f32.powf(log_min + freq_pos * (log_max - log_min));

                let blend = if actual_freq <= 300.0 {
                    0.0
                } else if actual_freq >= 3000.0 {
                    1.0
                } else {
                    let t = (actual_freq - 300.0) / (3000.0 - 300.0);
                    t * t * (3.0 - 2.0 * t)
                };

                let lpf_target = actual_freq + (20000.0 - actual_freq) * blend;
                let hpf_target = 20.0 + (actual_freq - 20.0) * blend;
                let effective_lpf = 20000.0 - (20000.0 - lpf_target) * intensity;
                let soft_lpf = if effective_lpf < 1000.0 {
                    let norm = ((effective_lpf - 200.0) / 800.0).clamp(0.0, 1.0);
                    200.0 + 800.0 * norm.sqrt()
                } else {
                    effective_lpf
                };
                let effective_hpf = 20.0 + (hpf_target - 20.0) * intensity;
                let _ = client.set_lpf(soft_lpf.clamp(200.0, 20000.0));
                let _ = client.set_hpf(effective_hpf.clamp(20.0, 8000.0));
                // Apply kill switches: extreme cut values
                let effective_low = if self.mixer.cue_channel.eq_low_kill { -24.0 } else { self.mixer.cue_channel.eq_low };
                let effective_mid = if self.mixer.cue_channel.eq_mid_kill { -24.0 } else { self.mixer.cue_channel.eq_mid };
                let effective_high = if self.mixer.cue_channel.eq_high_kill { -24.0 } else { self.mixer.cue_channel.eq_high };
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
                let _ = client.set_pan(self.mixer.cue_channel.pan);
            }

            self.sc_deck_c = Some(client);
        } else {
            self.mixer.cue_channel.name = item.name.clone();
            self.mixer.cue_channel.uses_supercollider = false;
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

    /// Get the MPV client for a channel index.
    fn mpv_for_channel(&mut self, ch_idx: usize) -> Option<&mut MpvClient> {
        if ch_idx == self.mixer.dj.deck_a_channel {
            self.mpv_deck_a.as_mut()
        } else if ch_idx == self.mixer.dj.deck_b_channel {
            self.mpv_deck_b.as_mut()
        } else if ch_idx == self.mixer.dj.deck_c_channel {
            self.mpv_deck_c.as_mut()
        } else {
            None
        }
    }

    /// Get the SuperCollider client for a channel index.
    fn sc_for_channel(&mut self, ch_idx: usize) -> Option<&mut SuperColliderClient> {
        if ch_idx == self.mixer.dj.deck_a_channel {
            self.sc_deck_a.as_mut()
        } else if ch_idx == self.mixer.dj.deck_b_channel {
            self.sc_deck_b.as_mut()
        } else if ch_idx == self.mixer.dj.deck_c_channel {
            self.sc_deck_c.as_mut()
        } else {
            None
        }
    }

    /// Re-route audio devices after a CUE-to-deck send.
    ///
    /// After swapping MPV clients, the new Deck A/B (old CUE) process is still
    /// outputting to the CUE device, and the new CUE (old Deck A/B) is on Master.
    /// This swaps the audio outputs so each process outputs to the correct bus.
    ///
    /// Falls back to querying the opposite MPV process's current device when the
    /// user hasn't explicitly picked one from the output picker.
    fn reroute_audio_devices_after_cue_send(&mut self, to_deck_a: bool) {
        // Prefer explicit picker selection; fall back to querying the OPPOSITE process.
        // After the swap: mpv_deck_a/c = old CUE (was on CUE device), mpv_deck_c = old Deck A/B (was on Master).
        // So query the old Deck A/B process (now mpv_deck_c) to discover the Master device,
        // and query the old CUE process (now mpv_deck_a/b) to discover the CUE device.
        let master_dev = self.master_output.selected_mpv_name().map(|s| s.to_string())
            .or_else(|| {
                // mpv_deck_c is the old Deck A/B process — it was on Master
                self.mpv_deck_c.as_mut().and_then(|c| c.get_audio_device().ok())
            });
        let cue_dev = self.cue_output.selected_mpv_name().map(|s| s.to_string())
            .or_else(|| {
                // mpv_deck_a/b is the old CUE process — it was on CUE
                let client = if to_deck_a { self.mpv_deck_a.as_mut() } else { self.mpv_deck_b.as_mut() };
                client.and_then(|c| c.get_audio_device().ok())
            });

        tracing::debug!(
            "reroute_audio_devices: master_dev={:?}, cue_dev={:?}, to_deck_a={}",
            master_dev, cue_dev, to_deck_a
        );

        // Set the new Deck A/B to Master output
        if let Some(dev) = &master_dev {
            let client = if to_deck_a { self.mpv_deck_a.as_mut() } else { self.mpv_deck_b.as_mut() };
            if let Some(c) = client {
                tracing::debug!("Setting deck {} audio device to master: {}", if to_deck_a { "A" } else { "B" }, dev);
                c.set_audio_device(dev).ok();
            }
        }
        // Set the new CUE to CUE output
        if let Some(dev) = &cue_dev {
            if let Some(c) = self.mpv_deck_c.as_mut() {
                tracing::debug!("Setting deck C audio device to cue: {}", dev);
                c.set_audio_device(dev).ok();
            }
        }
    }

    /// Sync volume to MPV/SC for a specific deck, combining fader, crossfader, and master
    fn sync_deck_volume(&mut self, deck_a: bool) {
        let ch_idx = if deck_a {
            self.mixer.dj.deck_a_channel
        } else {
            self.mixer.dj.deck_b_channel
        };
        self.sync_volume_to_mpv(ch_idx);
    }

    /// Sync volume change to MPV for a channel (applies crossfader gain)
    pub fn sync_volume_to_mpv(&mut self, channel_idx: usize) {
        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        let gain = if channel_idx == self.mixer.dj.deck_b_channel { gain_b } else { gain_a };
        let master = self.mixer.master.fader;
        let solo_active = self.mixer.solo_active;

        let ch = self.mixer.get_channel(channel_idx);
        let fader = ch.map(|c| c.fader).unwrap_or(0.5);
        let muted = ch.map(|c| c.muted).unwrap_or(false);
        let solo = ch.map(|c| c.solo).unwrap_or(false);
        let effective_muted = self.mixer.master.muted || muted || (solo_active && !solo);

        // Mute MPV when engine has a decoder loaded (avoid double-audio)
        let engine_active = self.audio_engine.as_ref().map(|e| e.has_decoder(channel_idx)).unwrap_or(false);
        let vol = if engine_active || effective_muted { 0.0 } else {
            (fader * gain * master * 2.0 * 200.0).clamp(0.0, 200.0)
        };
        if let Some(client) = self.mpv_for_channel(channel_idx) {
            let _ = client.set_volume(vol);
        }
        let sc_vol = if effective_muted { 0.0 } else {
            (fader * gain * master * 2.0 * SC_GAIN_BOOST).clamp(0.0, 8.0)
        };
        if let Some(client) = self.sc_for_channel(channel_idx) {
            let _ = client.set_volume(sc_vol);
        }
        // Send to Rust audio engine (direct state write — no command channel)
        if let Some(ref engine) = self.audio_engine {
            engine.state.set_volume(channel_idx, fader);
            engine.state.set_muted(channel_idx, effective_muted);
            engine.state.set_solo_active(self.mixer.solo_active);
            engine.state.set_master_fader(self.mixer.master.fader);
            engine.state.set_crossfader(self.mixer.dj.crossfader);
        }
        if channel_idx < 3 {
            self.last_volume_push_ms[channel_idx] = self.elapsed_ms;
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
        let a_solo = self.mixer.get_channel(deck_a_ch).map(|c| c.solo).unwrap_or(false);
        let a_muted = if solo_active { !a_solo } else {
            self.mixer.get_channel(deck_a_ch).map(|c| c.muted).unwrap_or(false)
        };
        let a_fader = self.mixer.get_channel(deck_a_ch).map(|c| c.fader).unwrap_or(0.5);
        let a_vol = if a_muted { 0.0 } else {
            (a_fader * gain_a * master * 2.0 * 200.0).clamp(0.0, 200.0)
        };

        let b_solo = self.mixer.get_channel(deck_b_ch).map(|c| c.solo).unwrap_or(false);
        let b_muted = if solo_active { !b_solo } else {
            self.mixer.get_channel(deck_b_ch).map(|c| c.muted).unwrap_or(false)
        };
        let b_fader = self.mixer.get_channel(deck_b_ch).map(|c| c.fader).unwrap_or(0.5);
        let b_vol = if b_muted { 0.0 } else {
            (b_fader * gain_b * master * 2.0 * 200.0).clamp(0.0, 200.0)
        };

        let c_solo = self.mixer.cue_channel.solo;
        let c_muted = if solo_active { !c_solo } else { self.mixer.cue_channel.muted };
        let c_fader = self.mixer.cue_channel.fader;
        let c_vol = if c_muted { 0.0 } else {
            (c_fader * gain_a * master * 2.0 * 200.0).clamp(0.0, 200.0)
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
            let sc_vol = if a_muted { 0.0 } else { (a_fader * gain_a * master * 2.0 * SC_GAIN_BOOST).clamp(0.0, 8.0) };
            let _ = client.set_volume(sc_vol);
        }
        if let Some(ref client) = self.sc_deck_b {
            let sc_vol = if b_muted { 0.0 } else { (b_fader * gain_b * master * 2.0 * SC_GAIN_BOOST).clamp(0.0, 8.0) };
            let _ = client.set_volume(sc_vol);
        }

        // Update Rust audio engine solo state
        if let Some(ref engine) = self.audio_engine {
            engine.state.set_solo(deck_a_ch, a_solo);
            engine.state.set_solo(deck_b_ch, b_solo);
            engine.state.set_solo(2, c_solo);
            engine.state.set_solo_active(solo_active);
        }

        for msg in msgs { self.log_debug(msg); }
    }

    /// Start or accelerate a scrub on the currently selected deck channel.
    /// direction: -1.0 = reverse, 1.0 = forward
    /// coarse: true for H/J/K/L (faster acceleration)
    fn start_scrub(&mut self, direction: f32, coarse: bool) {
        if let SelectionFocus::Channel(ch_idx) = self.mixer.focus {
            if let Some(ch) = self.mixer.get_channel_mut(ch_idx) {
                if ch.uses_supercollider {
                    return;
                }

                ch.scrub_direction = direction;
                ch.scrub_coarse = coarse;
                // Accelerate: each keypress increases speed
                let accel = if coarse { 1.2 } else { 0.2 };
                ch.scrub_speed = (ch.scrub_speed + accel).clamp(0.1, 25.0);
            }
        }
    }

    /// Tick scrub state for all channels. Called every frame (~50ms).
    /// Advances accumulated seek amount and sends seek commands to MPV.
    pub fn tick_scrub(&mut self) {
        let dt = 0.05; // ~50ms per tick
        let deck_a_ch = self.mixer.dj.deck_a_channel;
        let deck_b_ch = self.mixer.dj.deck_b_channel;
        let deck_c_ch = self.mixer.dj.deck_c_channel;

        for ch_idx in [deck_a_ch, deck_b_ch, deck_c_ch] {
            let (direction, speed, uses_supercollider) = self.mixer.get_channel(ch_idx)
                .map(|c| (c.scrub_direction, c.scrub_speed, c.uses_supercollider))
                .unwrap_or((0.0, 0.0, false));

            if uses_supercollider || direction == 0.0 || speed <= 0.0 {
                continue;
            }

            // Seek amount = direction * speed * dt (in seconds)
            let seek_amount = direction * speed * dt;

            // Accumulate in per-channel field
            if let Some(ch) = self.mixer.get_channel_mut(ch_idx) {
                ch.scrub_accumulator += seek_amount;

                // Seek when accumulator reaches a minimum threshold
                if ch.scrub_accumulator.abs() >= 0.02 {
                    let seek_to = ch.scrub_accumulator;
                    ch.scrub_accumulator = 0.0;

                    if let Some(client) = self.mpv_for_channel(ch_idx) {
                        let _ = client.send_command(vec![
                            "seek".into(),
                            serde_json::json!(seek_to),
                            "relative".into(),
                        ]);
                    }
                }
            }
        }
    }

    /// Decay scrub speed for all channels. Called every frame.
    /// Speed decays toward 0 when no keys are being pressed.
    pub fn decay_scrub_speed(&mut self) {
        let decay = 0.85; // Speed decays by 15% per tick
        for ch in &mut self.mixer.channels {
            if ch.scrub_speed > 0.01 {
                ch.scrub_speed *= decay;
                if ch.scrub_speed < 0.01 {
                    ch.scrub_speed = 0.0;
                    ch.scrub_direction = 0.0;
                }
            }
        }
    }

    /// Poll time_pos and duration from MPV for all deck channels.
    pub fn poll_scrub_positions(&mut self) {
        let deck_a_ch = self.mixer.dj.deck_a_channel;
        let deck_b_ch = self.mixer.dj.deck_b_channel;
        let deck_c_ch = self.mixer.dj.deck_c_channel;

        for ch_idx in [deck_a_ch, deck_b_ch, deck_c_ch] {
            let time_pos = self.mpv_for_channel(ch_idx)
                .and_then(|c| c.get_time_pos().ok());
            let duration = self.mpv_for_channel(ch_idx)
                .and_then(|c| c.get_duration().ok());

            if let Some(ch) = self.mixer.get_channel_mut(ch_idx) {
                if let Some(tp) = time_pos {
                    ch.time_pos = tp;
                }
                if let Some(dur) = duration {
                    ch.duration = dur;
                }
            }
        }
    }

    /// Apply pending BPM results from background analysis to channel state
    fn apply_pending_bpm(&mut self) {
        let results: Vec<(usize, f32, Option<String>)> = {
            if let Ok(mut queue) = self.pending_bpm.lock() {
                queue.drain(..).collect()
            } else {
                Vec::new()
            }
        };
        for (ch_idx, bpm, key) in results {
            if let Some(ch) = self.mixer.get_channel_mut(ch_idx) {
                tracing::debug!("apply_pending_bpm ch={}: bpm={:.1}, key={:?}", ch_idx, bpm, key);
                ch.bpm = Some(bpm);
                if key.is_some() {
                    ch.key = key;
                    ch.key_offset = 0;
                }
            }
        }
    }

    fn poll_tidal_bpm(&mut self) {
        let Ok(text) = std::fs::read_to_string(TIDAL_BPM_PATH) else {
            return;
        };
        let Ok(bpm) = text.trim().parse::<f32>() else {
            return;
        };
        if !(10.0..=400.0).contains(&bpm) {
            return;
        }

        for idx in 0..self.mixer.channels.len() {
            if let Some(ch) = self.mixer.get_channel_mut(idx)
                && ch.uses_supercollider
            {
                ch.bpm = Some(bpm);
                ch.base_bpm = bpm;
                ch.target_bpm = bpm;
            }
        }

        if self.mixer.cue_channel.uses_supercollider {
            self.mixer.cue_channel.bpm = Some(bpm);
            self.mixer.cue_channel.base_bpm = bpm;
            self.mixer.cue_channel.target_bpm = bpm;
        }
    }

    /// Read real-time onset-detected BPM from each MPV metering thread.
    /// Read real-time onset-detected BPM from each MPV metering thread.
    /// Captures base_bpm on first detection for stable speed factor.
    fn poll_onset_bpm(&mut self) {
        let deck_a_ch = self.mixer.dj.deck_a_channel;
        let deck_b_ch = self.mixer.dj.deck_b_channel;

        if let Some(ref client) = self.mpv_deck_a {
            let bpm = client.get_detected_bpm();
            if bpm > 0.0 {
                if let Some(ch) = self.mixer.get_channel_mut(deck_a_ch) {
                    if ch.base_bpm == 0.0 {
                        ch.base_bpm = bpm;
                        ch.target_bpm = bpm; // Start at x1.00
                    }
                    ch.bpm = Some(bpm);
                }
            }
        }
        if let Some(ref client) = self.mpv_deck_b {
            let bpm = client.get_detected_bpm();
            if bpm > 0.0 {
                if let Some(ch) = self.mixer.get_channel_mut(deck_b_ch) {
                    if ch.base_bpm == 0.0 {
                        ch.base_bpm = bpm;
                        ch.target_bpm = bpm;
                    }
                    ch.bpm = Some(bpm);
                }
            }
        }
        // Deck C maps to channel 6 (CUE)
        if let Some(ref client) = self.mpv_deck_c {
            let bpm = client.get_detected_bpm();
            if bpm > 0.0 {
                if let Some(ch) = self.mixer.get_channel_mut(6) {
                    if ch.base_bpm == 0.0 {
                        ch.base_bpm = bpm;
                        ch.target_bpm = bpm;
                    }
                    ch.bpm = Some(bpm);
                }
            }
        }
    }

    /// Sync mute state to MPV/SC for a channel
    pub fn sync_mute_to_mpv(&mut self, channel_idx: usize) {
        let muted = self.mixer.get_channel(channel_idx)
            .map(|c| c.muted)
            .unwrap_or(false);
        let fader = self.mixer.get_channel(channel_idx)
            .map(|c| c.fader)
            .unwrap_or(0.5);
        let (gain_a, gain_b) = self.calculate_crossfader_gains();
        let gain = if channel_idx == self.mixer.dj.deck_b_channel { gain_b } else { gain_a };
        let master = self.mixer.master.fader;

        if let Some(client) = self.mpv_for_channel(channel_idx) {
            let _ = client.set_mute(muted);
            let vol = if muted {
                0.0
            } else {
                (fader * gain * master * 2.0 * 200.0).clamp(0.0, 200.0)
            };
            let _ = client.set_volume(vol);
        }
        if let Some(client) = self.sc_for_channel(channel_idx) {
            let vol = if muted {
                0.0
            } else {
                (fader * gain * master * 2.0 * SC_GAIN_BOOST).clamp(0.0, 8.0)
            };
            let _ = client.set_volume(vol);
        }
        if let Some(ref engine) = self.audio_engine {
            engine.state.set_muted(channel_idx, muted);
        }
        self.sync_capture_dsp_params();
    }

    /// Sync play/pause state to MPV/SC for a channel
    pub fn sync_playpause_to_mpv(&mut self, channel_idx: usize) {
        let playing = self.mixer.get_channel(channel_idx)
            .map(|c| c.playing)
            .unwrap_or(false);
        let paused = !playing;

        if let Some(client) = self.mpv_for_channel(channel_idx) {
            let _ = client.set_pause(paused);
        }
        if let Some(client) = self.sc_for_channel(channel_idx) {
            let _ = client.set_pause(paused);
        }

        // If master is paused but a deck is now playing, unpause master
        if !self.mixer.master.playing && playing {
            self.mixer.master.playing = true;
        }
    }

    /// Sync master play/pause to all connected channels
    pub fn sync_all_playpause(&mut self) {
        let playing = self.mixer.master.playing;

        if playing {
            // Resume: only resume channels that were playing before the pause
            let prev = self.mixer.previously_playing.clone();

            for (idx, &was_playing) in prev.iter().enumerate() {
                if let Some(channel) = self.mixer.get_channel_mut(idx) {
                    channel.playing = was_playing;
                }
                let paused = !was_playing;
                if let Some(client) = self.mpv_for_channel(idx) {
                    let _ = client.set_pause(paused);
                }
                if let Some(client) = self.sc_for_channel(idx) {
                    let _ = client.set_pause(paused);
                }
            }
        } else {
            // Pause: save actual MPV playback state (channel.playing may be stale),
            // then pause all.
            self.mixer.previously_playing.clear();
            for idx in 0..self.mixer.channels.len() {
                let was_playing = self.mpv_for_channel(idx)
                    .and_then(|c| c.get_pause().ok())
                    .map(|paused| !paused)
                    .unwrap_or_else(|| {
                        self.mixer.get_channel(idx)
                            .map(|c| c.playing)
                            .unwrap_or(false)
                    });
                self.mixer.previously_playing.push(was_playing);
                if let Some(channel) = self.mixer.get_channel_mut(idx) {
                    channel.playing = false;
                }
                if let Some(client) = self.mpv_for_channel(idx) {
                    let _ = client.set_pause(true);
                }
                if let Some(client) = self.sc_for_channel(idx) {
                    let _ = client.set_pause(true);
                }
            }
            // Also save and pause CUE channel
            let cue_was_playing = self.mpv_for_channel(2)
                .and_then(|c| c.get_pause().ok())
                .map(|paused| !paused)
                .unwrap_or(self.mixer.cue_channel.playing);
            self.mixer.previously_playing.push(cue_was_playing);
            self.mixer.cue_channel.playing = false;
            if let Some(client) = self.mpv_for_channel(2) {
                let _ = client.set_pause(true);
            }
            if let Some(client) = self.sc_for_channel(2) {
                let _ = client.set_pause(true);
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
                    ChannelControl::Scrub => {
                        // Scrub is handled by tick_scrub, no sync needed here
                    }
                    ChannelControl::Bpm => {
                        self.sync_bpm_to_mpv(ch_idx);
                    }
                    ChannelControl::Key => {
                        // Key adjustment changes playback_speed directly, sync to MPV
                        if let Some(channel) = self.mixer.get_channel(ch_idx) {
                            let speed = channel.playback_speed;
                            if let Some(client) = self.mpv_for_channel(ch_idx) {
                                let _ = client.set_speed(speed);
                            }
                        }
                    }
                    ChannelControl::Pan => {
                        self.sync_pan_to_mpv(ch_idx);
                    }
                    ChannelControl::EqLow | ChannelControl::EqMid | ChannelControl::EqHigh => {
                        self.sync_eq_to_mpv(ch_idx);
                    }
                    ChannelControl::FilterCutoff | ChannelControl::FilterFreq
                    | ChannelControl::LfoShape | ChannelControl::LfoSpeed => {
                        self.sync_filter_to_mpv(ch_idx);
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
                // Master EQ: sync to all decks
                if self.mixer.selected_global.eq_band_index().is_some() {
                    self.sync_master_eq_to_all_decks();
                }
            }
        }
    }

    /// Sync EQ to MPV for a channel
    fn sync_eq_to_mpv(&mut self, channel_idx: usize) {
        let (low, mid, high, low_kill, mid_kill, high_kill) = self.mixer.get_channel(channel_idx)
            .map(|c| (c.eq_low, c.eq_mid, c.eq_high, c.eq_low_kill, c.eq_mid_kill, c.eq_high_kill))
            .unwrap_or((0.0, 0.0, 0.0, false, false, false));

        let effective_low = if low_kill { -24.0 } else { low };
        let effective_mid = if mid_kill { -24.0 } else { mid };
        let effective_high = if high_kill { -24.0 } else { high };

        let engine_active = self.audio_engine.as_ref().map(|e| e.has_decoder(channel_idx)).unwrap_or(false);
        if !engine_active {
            if let Some(client) = self.mpv_for_channel(channel_idx) {
                let _ = client.set_eq(effective_low, effective_mid, effective_high);
            }
        }
        if let Some(client) = self.sc_for_channel(channel_idx) {
            let _ = client.set_eq(effective_low, effective_mid, effective_high);
        }
        // Send to Rust audio engine
        if let Some(ref engine) = self.audio_engine {
            engine.state.set_eq(channel_idx, low, mid, high);
            engine.state.set_eq_kill(channel_idx, low_kill, mid_kill, high_kill);
        }
        self.sync_capture_dsp_params();
    }

    /// Sync unified filter to MPV for a channel
    ///
    /// The filter has two controls:
    /// - `filter_cutoff` (0.0–1.0): intensity of the filter effect
    /// - `filter_freq` (0.0–1.0): filter position
    ///   - 0.0 = 20 Hz (lowpass)
    ///   - 0.5 = 1000 Hz (center / no effect)
    ///   - 1.0 = 20000 Hz (highpass)
    ///
    /// Plus LFO controls that modulate the cutoff intensity:
    /// - `lfo_shape` (0.0 = square, 1.0 = sine): waveform morph
    /// - `lfo_speed` (0.0 = slow ~0.05Hz, 1.0 = fast ~10Hz): modulation rate (cubic curve)
    ///
    /// A crossfade zone (300Hz–3kHz) smoothly transitions between LPF and HPF
    /// so sweeping through center is continuous with no dead zone.
    /// Each filter sweeps from "fully open" to target as cutoff increases,
    /// so at zero cutoff there is zero effect regardless of frequency position.
    fn sync_filter_to_mpv(&mut self, channel_idx: usize) {
        let (cutoff, freq_pos, lfo_shape, lfo_speed) = self.mixer.get_channel(channel_idx)
            .map(|c| (c.filter_cutoff, c.filter_freq, c.lfo_shape, c.lfo_speed))
            .unwrap_or((0.0, 0.5, 0.0, 0.0));

        // Power curve for smooth intensity ramp (exponent 1.2)
        let raw_cutoff = cutoff.powf(1.2);

        // When LFO speed is 0, bypass LFO entirely — shape has no effect
        let modulated = if lfo_speed <= 0.001 {
            raw_cutoff
        } else {
            // LFO modulates the user's cutoff intensity
            // Shape: 0.0 → square (toggles between 0 and 1 every half cycle)
            // Shape: 1.0 → sine (smoothly sweeps between 0 and 1)
            let phase = self.mixer.get_channel(channel_idx)
                .map(|c| c.lfo_phase)
                .unwrap_or(0.0);
            let raw_lfo = (phase * std::f32::consts::TAU).sin();
            let sq = if raw_lfo > 0.0 { 1.0 } else { 0.0 };
            let sine = raw_lfo * 0.5 + 0.5;
            let lfo_out = sq * (1.0 - lfo_shape) + sine * lfo_shape;
            raw_cutoff * lfo_out
        };

        // Ease-out the last 10% for a gentle approach to max
        let intensity = if modulated > 0.9 {
            let t = (modulated - 0.9) / 0.1;
            0.9 + 0.1 * (1.0 - (1.0 - t).powf(3.0))
        } else {
            modulated
        };

        // Map freq_pos (0–1) to actual frequency (20–20000 Hz, log scale)
        let log_min = 20f32.log10();
        let log_max = 20000f32.log10();
        let actual_freq = 10f32.powf(log_min + freq_pos * (log_max - log_min));

        // Crossfade zone: 300Hz–3kHz
        // blend=0 at 300Hz (pure LPF), blend=1 at 3000Hz (pure HPF)
        let blend = if actual_freq <= 300.0 {
            0.0
        } else if actual_freq >= 3000.0 {
            1.0
        } else {
            let t = (actual_freq - 300.0) / (3000.0 - 300.0);
            t * t * (3.0 - 2.0 * t) // smoothstep
        };

        // LPF target: actual_freq at blend=0, 20000 (open) at blend=1
        let lpf_target = actual_freq + (20000.0 - actual_freq) * blend;
        // HPF target: 20 (open) at blend=0, actual_freq at blend=1
        let hpf_target = 20.0 + (actual_freq - 20.0) * blend;

        // Both sweep from open to target as intensity increases
        let effective_lpf = 20000.0 - (20000.0 - lpf_target) * intensity;
        // Soft approach to 200Hz floor: spread 200-1000Hz with sqrt
        // so descent slows as it nears the clamp
        let soft_lpf = if effective_lpf < 1000.0 {
            let norm = ((effective_lpf - 200.0) / 800.0).clamp(0.0, 1.0);
            200.0 + 800.0 * norm.sqrt()
        } else {
            effective_lpf
        };

        let effective_hpf = 20.0 + (hpf_target - 20.0) * intensity;

        // Skip MPV lavfi calls when the engine has a decoder for this channel.
        // SC controls route over OSC and should always receive updates.
        let engine_active = self.audio_engine.as_ref().map(|e| e.has_decoder(channel_idx)).unwrap_or(false);
        if !engine_active {
            if let Some(client) = self.mpv_for_channel(channel_idx) {
                let le = client.set_lpf(soft_lpf.clamp(200.0, 20000.0)).err();
                let he = client.set_hpf(effective_hpf.clamp(20.0, 8000.0)).err();
                if let Some(e) = le { self.debug_log.push(format!("lpf: {}", e)); }
                if let Some(e) = he { self.debug_log.push(format!("hpf: {}", e)); }
            }
        }
        if let Some(client) = self.sc_for_channel(channel_idx) {
            let _ = client.set_lpf(soft_lpf.clamp(200.0, 20000.0));
            let _ = client.set_hpf(effective_hpf.clamp(20.0, 8000.0));
        }

        // Send to Rust audio engine
        if let Some(ref engine) = self.audio_engine {
            engine.state.set_filter_cutoff(channel_idx, cutoff);
            engine.state.set_filter_freq(channel_idx, freq_pos);
            engine.state.set_lfo(channel_idx, lfo_speed, lfo_shape);
        }

        self.sync_capture_dsp_params();
    }

    /// Sync master EQ to all active decks (MPV + SuperCollider)
    fn sync_master_eq_to_all_decks(&mut self) {
        use crate::state::MASTER_EQ_FREQUENCIES;
        let bands = self.mixer.master.master_eq;

        // Send to all 3 MPV decks
        for ch_idx in 0..3 {
            if let Some(client) = self.mpv_for_channel(ch_idx) {
                let _ = client.set_master_eq(&bands, &MASTER_EQ_FREQUENCIES);
            }
            if let Some(client) = self.sc_for_channel(ch_idx) {
                let _ = client.set_master_eq(&bands);
            }
        }
        self.sync_capture_dsp_params();
    }

    /// Sync pan to MPV/SC for a channel
    fn sync_pan_to_mpv(&mut self, channel_idx: usize) {
        let pan = self.mixer.get_channel(channel_idx)
            .map(|c| c.pan)
            .unwrap_or(0.0);

        let engine_active = self.audio_engine.as_ref().map(|e| e.has_decoder(channel_idx)).unwrap_or(false);
        if !engine_active {
            if let Some(client) = self.mpv_for_channel(channel_idx) {
                let _ = client.set_pan(pan);
            }
        }
        if let Some(client) = self.sc_for_channel(channel_idx) {
            let _ = client.set_pan(pan);
        }
        // Send to Rust audio engine
        if let Some(ref engine) = self.audio_engine {
            engine.state.set_pan(channel_idx, pan);
        }
        self.sync_capture_dsp_params();
    }

    /// Sync BPM-based speed to MPV for a channel
    /// Speed = target_bpm / base_bpm (stable reference from first detection)
    fn sync_bpm_to_mpv(&mut self, channel_idx: usize) {
        if self.mixer.get_channel(channel_idx)
            .map(|c| c.uses_supercollider)
            .unwrap_or(false)
        {
            return;
        }

        let (target_bpm, base_bpm, key_offset) = self.mixer.get_channel(channel_idx)
            .map(|c| (c.target_bpm, c.base_bpm, c.key_offset))
            .unwrap_or((120.0, 120.0, 0));

        let base = if base_bpm > 0.0 { base_bpm } else { 120.0 };
        let bpm_factor = (target_bpm / base).clamp(0.1, 4.0);
        let semitone_factor = 2.0_f32.powf(key_offset as f32 / 12.0);
        let speed = (bpm_factor * semitone_factor).clamp(0.1, 4.0);
        if let Some(client) = self.mpv_for_channel(channel_idx) {
            let _ = client.set_speed(speed);
        }
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
        // Auto-scroll to bottom when new messages arrive (unless user is scrolling)
        if self.debug_scroll > 0 {
            self.debug_scroll = 0;
        }
    }
    
    /// Check if debug mode is enabled via DEBUG env var
    #[allow(dead_code)]
    pub fn is_debug_enabled() -> bool {
        std::env::var("DEBUG").is_ok()
    }
}
