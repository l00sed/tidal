//! Main mixer layout that arranges all channel strips

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::state::{MixerState, RackMode, RackState, SelectionFocus, GlobalControl, ChannelControl, SamplePadGrid, PAD_KEYS};
use crate::ui::channel::{ChannelStrip, MasterStrip};
use crate::ui::colors::*;
use crate::ui::sampler::{CountInOverlay, PadConfigPane, RackRow};
use crate::ui::widgets::Crossfader;
use crate::app::{Deck, MixerLayout, SelectedPane, SourcePickerState, SourcePickerTab, PickerInputMode, OutputPickerTarget};

/// The main mixer view
pub struct MixerView<'a> {
    state: &'a MixerState,
    pads: &'a SamplePadGrid,
    show_help: bool,
    editing: bool,
    control_select: bool,
    frame: u8,
    selected_pane: SelectedPane,
    source_picker: Option<(Deck, &'a SourcePickerState)>,
    sample_picker: Option<(usize, &'a SourcePickerState)>,
    selected_pad_idx: Option<usize>,
    pad_config_mode: bool,
    pad_config_editing: bool,
    racks: Option<&'a RackState>,
    scroll_offset: usize,
    master_output_device: Option<&'a str>,
    cue_output_device: Option<&'a str>,
    output_picker_active: bool,
    output_picker_target: OutputPickerTarget,
    master_output_devices: &'a [String],
    cue_output_devices: &'a [String],
    selected_master_output_idx: usize,
    selected_cue_output_idx: usize,
    debug_log: Option<&'a [String]>,
    samples_dir: Option<&'a std::path::Path>,
}

impl<'a> MixerView<'a> {
    pub fn new(state: &'a MixerState, pads: &'a SamplePadGrid) -> Self {
        Self {
            state,
            pads,
            show_help: false,
            editing: false,
            control_select: false,
            frame: 0,
            selected_pane: SelectedPane::DeckA,
            source_picker: None,
            sample_picker: None,
            selected_pad_idx: None,
            pad_config_mode: false,
            pad_config_editing: false,
            racks: None,
            scroll_offset: 0,
            master_output_device: None,
            cue_output_device: None,
            output_picker_active: false,
            output_picker_target: OutputPickerTarget::Master,
            master_output_devices: &[],
            cue_output_devices: &[],
            selected_master_output_idx: 0,
            selected_cue_output_idx: 0,
            debug_log: None,
            samples_dir: None,
        }
    }

    pub fn show_help(mut self, show: bool) -> Self {
        self.show_help = show;
        self
    }

    pub fn editing(mut self, editing: bool) -> Self {
        self.editing = editing;
        self
    }
    
    pub fn control_select(mut self, control_select: bool) -> Self {
        self.control_select = control_select;
        self
    }

    pub fn frame(mut self, frame: u8) -> Self {
        self.frame = frame;
        self
    }

    pub fn selected_pane(mut self, pane: SelectedPane) -> Self {
        self.selected_pane = pane;
        self
    }
    
    pub fn source_picker(mut self, deck: Deck, picker: &'a SourcePickerState) -> Self {
        self.source_picker = Some((deck, picker));
        self
    }
    
    pub fn sample_picker(mut self, pad_idx: usize, picker: &'a SourcePickerState) -> Self {
        self.sample_picker = Some((pad_idx, picker));
        self
    }

    pub fn pad_config_mode(mut self, config_mode: bool) -> Self {
        self.pad_config_mode = config_mode;
        self
    }

    pub fn pad_config_editing(mut self, editing: bool) -> Self {
        self.pad_config_editing = editing;
        self
    }

    pub fn racks(mut self, racks: &'a RackState) -> Self {
        self.racks = Some(racks);
        self
    }

    pub fn scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = offset;
        self
    }
    
    pub fn selected_pad_idx(mut self, idx: Option<usize>) -> Self {
        self.selected_pad_idx = idx;
        self
    }

    pub fn master_output_device(mut self, device: Option<&'a str>) -> Self {
        self.master_output_device = device;
        self
    }

    pub fn cue_output_device(mut self, device: Option<&'a str>) -> Self {
        self.cue_output_device = device;
        self
    }

    pub fn output_picker_active(mut self, active: bool) -> Self {
        self.output_picker_active = active;
        self
    }

    pub fn output_picker_target(mut self, target: OutputPickerTarget) -> Self {
        self.output_picker_target = target;
        self
    }

    pub fn master_output_devices(mut self, devices: &'a [String]) -> Self {
        self.master_output_devices = devices;
        self
    }

    pub fn cue_output_devices(mut self, devices: &'a [String]) -> Self {
        self.cue_output_devices = devices;
        self
    }

    pub fn selected_master_output_idx(mut self, idx: usize) -> Self {
        self.selected_master_output_idx = idx;
        self
    }

    pub fn selected_cue_output_idx(mut self, idx: usize) -> Self {
        self.selected_cue_output_idx = idx;
        self
    }
    
    pub fn debug_log(mut self, log: &'a [String]) -> Self {
        self.debug_log = Some(log);
        self
    }

    pub fn samples_dir(mut self, dir: Option<&'a std::path::Path>) -> Self {
        self.samples_dir = dir;
        self
    }
}

impl<'a> Widget for MixerView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Split area for debug log at bottom (only when DEBUG env var is set)
        let debug_enabled = std::env::var("DEBUG").is_ok();
        let (main_area, debug_area) = if debug_enabled && self.debug_log.is_some() && !self.debug_log.unwrap().is_empty() {
            let chunks = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(5),  // 5 lines for debug log
            ]).split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };
        
        // Always use full-width layout
        self.render_full_width_layout(main_area, buf);

        // Render help overlay if enabled
        if self.show_help {
            self.render_help_overlay(main_area, buf);
        }
        
        // Render source picker overlay if active
        if let Some((deck, picker)) = self.source_picker {
            self.render_source_picker(main_area, buf, deck, picker);
        }
        
        // Sample picker is rendered inline in render_dj_center

        // Render output device picker overlay if active
        if self.output_picker_active {
            self.render_output_picker(main_area, buf);
        }
        
        // Render debug log if present
        if let Some(area) = debug_area {
            if let Some(log) = self.debug_log {
                self.render_debug_log(area, buf, log);
            }
        }
    }
}

impl<'a> MixerView<'a> {
    /// Full-width layout that scales to terminal size
    fn render_full_width_layout(&self, area: Rect, buf: &mut Buffer) {
        // Main layout: header, mixer area, footer
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),  // Header (compact)
                Constraint::Min(16),    // Mixer area
                Constraint::Length(1),  // Footer (single line)
            ])
            .split(area);

        self.render_header(main_chunks[0], buf);
        self.render_mixer_full_width(main_chunks[1], buf);
        self.render_footer(main_chunks[2], buf);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        // Minimalist header - just mode indicator and status
        let mode_str = if self.editing { "[EDIT]" } else { "" };
        let title = format!("TERMIXER {}", mode_str);
        
        let title_style = Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD);
        buf.set_string(area.x + 1, area.y, &title, title_style);

        // Status on the right: pane name, optionally with control name
        let pane_label = match self.selected_pane {
            SelectedPane::DeckA => "DECK A",
            SelectedPane::DeckB => "DECK B",
            SelectedPane::DeckC => "DECK C",
            SelectedPane::Master => "MASTER",
            SelectedPane::DjCenter => "PADS",
            SelectedPane::Loops => "LOOPS",
            SelectedPane::Crossfader => "CROSSFADE",
        };

        let status = if self.control_select || self.editing {
            // ControlSelect or Edit mode: show pane name | control name
            // Skip the control name for Crossfader pane (it's redundant)
            if matches!(self.selected_pane, SelectedPane::Crossfader) {
                pane_label.to_string()
            } else {
                match self.state.focus {
                    SelectionFocus::Channel(_) => {
                        format!("{} | {}", pane_label, self.state.selected_control.label())
                    }
                    SelectionFocus::Global => {
                        format!("{} | {}", pane_label, self.state.selected_global.label())
                    }
                }
            }
        } else {
            // PaneSelect mode: just show the pane name
            pane_label.to_string()
        };

        let status_x = area.x + area.width.saturating_sub(status.len() as u16 + 1);
        buf.set_string(status_x, area.y, &status, Style::default().fg(METER_TRACK));

        // Thin separator line
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Style::default().fg(SEPARATOR));
    }

    /// Full-width mixer layout - DJ section centered, decks on sides
    fn render_mixer_full_width(&self, area: Rect, buf: &mut Buffer) {
        // Layout:
        // [Deck A] [DJ Center (pads)] [Deck B] [Deck C] [Master]
        //                   [Loops]
        //                   [Crossfader]
        //
        // DJ Center must stay ≥20 for the 4×4 pad grid. If there isn't room
        // for Master at minimum widths, Master is pushed to overflow (right
        // of viewport, reachable via horizontal scrolling).
        // Compute the horizontal layout (columns + scroll) from the viewport
        // width and which pane is selected. Centralised so the renderer,
        // scroll logic, and mouse hit-testing all agree.
        let layout = MixerLayout::compute(area.width, self.selected_pane, None, None);

        let loops_height = (area.height as f32 * 0.20) as u16;
        let loops_height = loops_height.max(3);

        // Build constraints ONLY for visible columns (start..=end).
        // This prevents ratatui from squeezing when off-screen columns
        // inflate the constraint sum beyond the viewport width.
        let mut constraints = vec![];
        for i in layout.start..=layout.end {
            let w = match i {
                0 => layout.deck_a,
                1 => layout.dj,
                2 => layout.deck_b,
                3 => layout.deck_c,
                4 => layout.master,
                _ => unreachable!(),
            };
            constraints.push(Constraint::Length(w));
        }
        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        // Map logical column index to chunk index: chunk_idx = col_idx - start
        let start = layout.start as usize;
        let chunk = |col: usize| -> Rect { horizontal_chunks[col - start] };

        // Only show selected control when in control select mode (or editing)
        let show_control = self.control_select || self.editing;

        // ── Deck A (column 0) ──
        if layout.start == 0
            && let Some(channel) = self.state.channels.get(self.state.dj.deck_a_channel)
        {
            let pane_selected = self.selected_pane == SelectedPane::DeckA;
            let control = if show_control && pane_selected { Some(self.state.selected_control) } else { None };
            ChannelStrip::new(channel)
                .selected(pane_selected, control)
                .deck_label(Some("A"))
                .deck_color(DECK_A)
                .editing(show_control && pane_selected)
                .frame(self.frame)
                .render(chunk(0), buf);
        }

        // ── DJ Center (column 1) — split vertically into Pads, Loops, Crossfader ──
        if layout.start <= 1 && 1 <= layout.end {
            let crossfader_height = 5u16;
            let dj_vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(16),                 // Pads area
                    Constraint::Length(loops_height),    // Loops pane
                    Constraint::Length(crossfader_height), // Crossfader pane
                ])
                .split(chunk(1));

            let dj_center_area = dj_vertical[0];
            let loops_area = dj_vertical[1];
            let crossfader_area = dj_vertical[2];

            self.render_dj_center(dj_center_area, buf);
            self.render_loops(loops_area, buf);
            self.render_crossfader(crossfader_area, buf);
        }

        // ── Deck B (column 2) ──
        if layout.start <= 2 && 2 <= layout.end
            && let Some(channel) = self.state.channels.get(self.state.dj.deck_b_channel)
        {
            let pane_selected = self.selected_pane == SelectedPane::DeckB;
            let control = if show_control && pane_selected { Some(self.state.selected_control) } else { None };
            ChannelStrip::new(channel)
                .selected(pane_selected, control)
                .deck_label(Some("B"))
                .deck_color(DECK_B)
                .editing(show_control && pane_selected)
                .frame(self.frame)
                .render(chunk(2), buf);
        }

        // ── Deck C (column 3) ──
        if layout.start <= 3 && 3 <= layout.end {
            self.render_cue_pane(chunk(3), buf);
        }

        // ── Master (column 4) ──
        if layout.start <= 4 && 4 <= layout.end {
            let master_pane_selected = self.selected_pane == SelectedPane::Master;
            let master_control_selected = master_pane_selected && show_control;
            let master_control = if master_control_selected { Some(self.state.selected_global) } else { None };
            let any_playing = self.state.channels.iter().any(|c| c.playing)
                || self.state.cue_channel.playing;
            MasterStrip::new(&self.state.master)
                .pane_selected(master_pane_selected)
                .selected(master_control_selected, master_control)
                .editing(show_control && master_pane_selected)
                .frame(self.frame)
                .any_channel_playing(any_playing)
                .render(chunk(4), buf);
        }
    }

    /// Centered DJ section with pads
    fn render_dj_center(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.control_select || self.editing;
        let border_color = if self.selected_pane == SelectedPane::DjCenter {
            if is_active { BORDER_ACTIVE } else { BORDER_NAVIGATED }
        } else {
            BG_LIGHT
        };
        
        let block = Block::default()
            .title(" PADS ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        block.render(area, buf);

        if let Some((pad_idx, picker)) = self.sample_picker {
            // Sample picker replaces config pane
            self.render_sample_picker_inline(inner, buf, pad_idx, picker);
        } else if self.pad_config_mode {
            // Config pane replaces pad grid
            PadConfigPane::new(self.pads)
                .editing(self.pad_config_editing)
                .samples_dir(self.samples_dir)
                .render(inner, buf);
        } else {
            // Pads (centered in the section)
            self.render_pad_grid_centered(inner, buf);
        }
    }

    /// Render pads centered horizontally in the available area
    fn render_pad_grid_centered(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 12 || area.height < 6 {
            return;
        }

        // Pads: 5 wide x 3 tall with 1 char gap between columns
        // This matches the visual gap from double horizontal borders between rows
        let cell_w = 5u16;
        let cell_h = 3u16;
        let gap_x = 1u16;  // Gap between columns
        
        let grid_w = cell_w * 4 + gap_x * 3;  // 4 cells + 3 gaps
        let grid_h = cell_h * 4;
        
        // Center the grid in available space
        let offset_x = (area.width.saturating_sub(grid_w)) / 2;
        let offset_y = (area.height.saturating_sub(grid_h)) / 2;

        for row in 0..4 {
            for col in 0..4 {
                let pad_idx = row * 4 + col;
                let x = area.x + offset_x + col as u16 * (cell_w + gap_x);
                let y = area.y + offset_y + row as u16 * cell_h;
                
                if x + cell_w <= area.x + area.width && y + cell_h <= area.y + area.height {
                    let cell_area = Rect::new(x, y, cell_w, cell_h);
                    self.render_pad_cell(cell_area, buf, pad_idx);
                }
            }
        }
    }
    
    /// Render a single pad cell with border
    fn render_pad_cell(&self, area: Rect, buf: &mut Buffer, pad_idx: usize) {
        let pad = &self.pads.pads[pad_idx];
        let is_triggered = pad.triggered;
        let is_selected = self.selected_pad_idx == Some(pad_idx);
        
        let border_color = if is_selected || is_triggered {
            BORDER_ACTIVE
        } else if pad.has_sample() {
            // Brighter color for pads with samples
            let (r, g, b) = pad.color;
            Color::Rgb(r.saturating_add(30), g.saturating_add(30), b.saturating_add(30))
        } else {
            BORDER_DEFAULT
        };
        let style = Style::default().fg(border_color);

        // Draw box border
        let horiz = "─".repeat((area.width - 2) as usize);
        buf.set_string(area.x, area.y, "┌", style);
        buf.set_string(area.x + 1, area.y, &horiz, style);
        buf.set_string(area.x + area.width - 1, area.y, "┐", style);
        
        buf.set_string(area.x, area.y + area.height - 1, "└", style);
        buf.set_string(area.x + 1, area.y + area.height - 1, &horiz, style);
        buf.set_string(area.x + area.width - 1, area.y + area.height - 1, "┘", style);
        
        // Side borders for middle rows
        for row in 1..(area.height - 1) {
            buf.set_string(area.x, area.y + row, "│", style);
            buf.set_string(area.x + area.width - 1, area.y + row, "│", style);
        }

        // Key label centered in inner area
        let key = pad.key_char().to_ascii_uppercase();
        let inner_w = area.width.saturating_sub(2);
        let cx = area.x + 1 + inner_w / 2;
        let cy = area.y + area.height / 2;
        
        let key_style = if is_selected || is_triggered {
            Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)
        } else if pad.has_sample() {
            Style::default().fg(SLIDER_MID)
        } else {
            Style::default().fg(HINT_DEFAULT)
        };
        buf.set_string(cx, cy, key.to_string(), key_style);
    }

    /// Render the loops/racks pane (below DJ center, between Deck A and Deck B)
    fn render_loops(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.control_select || self.editing;
        let border_color = if self.selected_pane == SelectedPane::Loops {
            if is_active { BORDER_ACTIVE } else { BORDER_NAVIGATED }
        } else {
            BG_LIGHT
        };

        let block = Block::default()
            .title(" LOOPS ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 1 || inner.width < 5 {
            return;
        }

        let rack_state = match self.racks {
            Some(s) => s,
            None => return,
        };

        let pane_selected = self.selected_pane == SelectedPane::Loops;

        // Rack rows (scrollable)
        let racks_area = inner;
        let max_visible = racks_area.height as usize;
        let scroll_offset = self.scroll_offset;

        for (display_idx, global_idx) in (scroll_offset..rack_state.racks.len()).enumerate() {
            if display_idx >= max_visible {
                break;
            }
            let rack = &rack_state.racks[global_idx];
            let row_y = racks_area.y + display_idx as u16;
            if row_y < racks_area.y + racks_area.height {
                let row_area = Rect::new(racks_area.x, row_y, racks_area.width, 1);
                let is_selected = pane_selected && rack_state.selected_rack == Some(global_idx);
                let is_recording = matches!(rack_state.mode, RackMode::Recording) 
                    && rack_state.selected_rack == Some(global_idx);
                let count_in = match rack_state.mode {
                    RackMode::CountIn { step, .. } if rack_state.selected_rack == Some(global_idx) => {
                        Some((step, 6))
                    }
                    _ => None,
                };
                let selected_control = if is_selected {
                    rack_state.selected_rack_control
                } else {
                    None
                };
                RackRow::new(rack)
                    .selected(is_selected)
                    .selected_control(selected_control)
                    .frame(self.frame)
                    .recording(is_recording)
                    .count_in_opt(count_in)
                    .render(row_area, buf);
            }
        }

        // Count-in overlay
        if let RackMode::CountIn { step, frame } = rack_state.mode {
            if pane_selected && rack_state.selected_rack.is_some() {
                CountInOverlay::new(step, frame).render(racks_area, buf);
            }
        }
    }

    /// Render the crossfader pane (below loops)
    fn render_crossfader(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.control_select || self.editing;
        let border_color = if self.selected_pane == SelectedPane::Crossfader {
            if is_active { BORDER_ACTIVE } else { BORDER_NAVIGATED }
        } else {
            BG_LIGHT
        };

        let block = Block::default()
            .title(" CROSSFADE ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 1 || inner.width < 10 {
            return;
        }

        // Crossfader with horizontal padding
        let padded = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(10),
                Constraint::Length(2),
            ])
            .split(inner);

        let show_control = self.control_select || self.editing;
        let crossfader_selected = show_control && self.selected_pane == SelectedPane::Crossfader;
        Crossfader::new(self.state.dj.crossfader)
            .selected(crossfader_selected)
            .labels("A", "B")
            .render(padded[1], buf);
    }


    /// Render the CUE deck (Deck C) with channel strip and controls
    fn render_cue_pane(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.control_select || self.editing;
        let pane_selected = self.selected_pane == SelectedPane::DeckC;
        
        // Render CUE pane border
        let border_color = if pane_selected {
            if is_active { BORDER_ACTIVE } else { BORDER_NAVIGATED }
        } else {
            BG_LIGHT
        };

        let border_title_style = if pane_selected {
            Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)
        } else if self.state.cue_channel.connected {
            Style::default().fg(border_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_GHOST)
        };

        let border_title = if self.state.cue_channel.connected { " C ● " } else { " C ○ " };

        let block = Block::default()
            .title(Span::styled(border_title, border_title_style))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 3 || inner.width < 6 {
            return;
        }

        // Split inner area: top part for channel strip, separator, bottom for controls
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),      // Channel strip
                Constraint::Length(1),   // Separator line
                Constraint::Length(3),   // Controls (-> A | -> B, sep, OUTPUT)
            ])
            .split(inner);

        // Render the CUE channel strip (no border - it's nested inside the CUE pane)
        let show_control = self.control_select || self.editing;
        let control = if show_control && pane_selected { Some(self.state.selected_control) } else { None };
        ChannelStrip::new(&self.state.cue_channel)
            .selected(pane_selected, control)
            .deck_label(Some("C"))
            .deck_color(DECK_C)
            .editing(show_control && pane_selected)
            .frame(self.frame)
            .show_border(false)
            .render(chunks[0], buf);

        // Render separator between channel strip and controls
        // Use ┼ at center to connect upward to M|->A and downward to S|->B
        let sep_area = chunks[1];
        let sep_style = Style::default().fg(SEPARATOR);
        let sep_center = sep_area.x + sep_area.width / 2;
        for x in sep_area.x..sep_area.x + sep_area.width {
            if x != sep_center {
                buf.set_string(x, sep_area.y, "─", sep_style);
            }
        }
        buf.set_string(sep_center, sep_area.y, "┼", sep_style);

        // Render CUE controls (S | -> B, sep, OUTPUT) - two-column layout like M/S
        let controls_area = chunks[2];

        // Two equal columns: S │ -> B
        let sep_x = controls_area.x + controls_area.width / 2;
        let left_w = sep_x - controls_area.x;
        let right_w = controls_area.x + controls_area.width - sep_x - 1;

        // Center each label within its half
        let solo_label = "S";
        let send_b_label = "-> B";
        let solo_x = controls_area.x + 1 + (left_w.saturating_sub(solo_label.len() as u16 + 2)) / 2;
        let send_b_x = sep_x + 2 + (right_w.saturating_sub(send_b_label.len() as u16 + 2)) / 2;

        // │ separator between S and -> B on the button row
        buf.set_string(sep_x, controls_area.y, "│", sep_style);

        // Separator line below with ┴ junction where │ meets it
        if controls_area.height > 1 {
            let sep_y = controls_area.y + 1;
            for x in controls_area.x..controls_area.x + controls_area.width {
                buf.set_string(x, sep_y, "─", sep_style);
            }
            buf.set_string(sep_x, sep_y, "┴", sep_style);
        }

        // S (Solo) button - with background highlight when active
        let solo_selected = pane_selected && show_control && self.state.selected_control == ChannelControl::Solo;
        let solo_active = self.state.cue_channel.solo;
        
        // Fill left column background if solo is active
        if solo_active {
            for x in controls_area.x + 1..sep_x.saturating_sub(1) {
                buf.set_string(x, controls_area.y, " ", Style::default().bg(BORDER_ACTIVE));
            }
        }
        
        let solo_style = if solo_active {
            Style::default().fg(Color::Black).bg(BORDER_ACTIVE)
        } else if solo_selected {
            Style::default().fg(BORDER_ACTIVE)
        } else {
            Style::default().fg(METER_TRACK)
        };
        buf.set_string(solo_x, controls_area.y, solo_label, solo_style);

        // -> B button
        let send_b_selected = pane_selected && show_control && self.state.selected_control == ChannelControl::CueSendToB;
        let send_b_style = if send_b_selected {
            Style::default().fg(BORDER_ACTIVE)
        } else {
            Style::default().fg(METER_TRACK)
        };
        buf.set_string(send_b_x, controls_area.y, send_b_label, send_b_style);

        // OUTPUT button (centered)
        let output_selected = pane_selected && show_control && self.state.selected_control == ChannelControl::CueOutputSelect;
        let output_style = if output_selected {
            Style::default().fg(BORDER_ACTIVE)
        } else {
            Style::default().fg(METER_TRACK)
        };
        if controls_area.height > 2 {
            let output_label = "OUTPUT";
            let output_x = controls_area.x + (controls_area.width.saturating_sub(output_label.len() as u16)) / 2;
            buf.set_string(output_x, controls_area.y + 2, output_label, output_style);
        }
    }

    /// Render a vertical slider with label at bottom
    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        // Single line footer with context hints
        let hint = if self.editing {
            if self.state.selected_global == GlobalControl::Crossfader {
                "hjkl:adjust  a:A  b:B  0/c:center  Enter/Esc:done"
            } else {
                // Check if a filter, pan, or BPM is selected
                match self.state.focus {
                    SelectionFocus::Channel(_) => {
                        match self.state.selected_control {
                            ChannelControl::FilterCutoff | ChannelControl::FilterFreq => {
                                "hjkl:adjust  H:min  L:max  0:reset  Enter/Esc:done"
                            }
                            ChannelControl::Pan => {
                                "hjkl:adjust  H:left  L:right  0/c:center  Enter/Esc:done"
                            }
                            ChannelControl::Scrub => {
                                "hjkl:scrub  HJKL:coarse  Enter/Esc:done"
                            }
                            ChannelControl::Bpm => {
                                "hjkl:adjust BPM  0/c:x1.00  Enter/Esc:done"
                            }
                            _ => "hjkl:adjust  0:reset  c:center  Enter/Esc:done"
                        }
                    }
                    SelectionFocus::Global => "hjkl:adjust  0:reset  c:center  Enter/Esc:done"
                }
            }
        } else if self.pads.active {
            "4567/RTYU/FGHJ/VBNM:trigger  Space:stop  P/Esc:exit"
        } else {
            match self.state.focus {
                SelectionFocus::Channel(_) => {
                    if self.state.selected_control.is_continuous() {
                        "hjkl:nav  Enter:edit  m:mute  s:solo  Tab:pane  ?:help"
                    } else {
                        "hjkl:nav  Enter:toggle  Tab:pane  ?:help"
                    }
                }
                SelectionFocus::Global => {
                    // Show crossfader slam shortcuts when Crossfader pane is selected
                    if self.selected_pane == SelectedPane::Crossfader {
                        "a:A  b:B  Enter:edit  Tab:deck  ?:help"
                    } else {
                        "hjkl:nav  Enter:edit  Tab:deck  ?:help"
                    }
                }
            }
        };

        buf.set_string(area.x + 1, area.y, hint, Style::default().fg(TEXT_DIM));

        // Mode indicators on right side
        let mut right_x = area.x + area.width;
        
        if !self.state.master.playing {
            let label = " PAUSED ";
            right_x = right_x.saturating_sub(label.len() as u16 + 1);
            buf.set_string(right_x, area.y, label,
                Style::default().fg(Color::Black).bg(BORDER_ACTIVE).add_modifier(Modifier::BOLD));
        }

        if self.pads.active {
            let label = " PADS ";
            right_x = right_x.saturating_sub(label.len() as u16 + 1);
            buf.set_string(right_x, area.y, label,
                Style::default().fg(Color::Black).bg(BORDER_ACTIVE).add_modifier(Modifier::BOLD));
        }

        if self.state.solo_active {
            let label = " SOLO ";
            right_x = right_x.saturating_sub(label.len() as u16 + 1);
            buf.set_string(right_x, area.y, label,
                Style::default().fg(Color::Black).bg(BORDER_ACTIVE).add_modifier(Modifier::BOLD));
        }

        if self.state.mute_active() {
            let label = " MUTE ";
            right_x = right_x.saturating_sub(label.len() as u16 + 1);
            buf.set_string(right_x, area.y, label,
                Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD));
        }
    }

    fn render_help_overlay(&self, area: Rect, buf: &mut Buffer) {
        let help_width = 52u16.min(area.width.saturating_sub(4));
        let help_height = 24u16.min(area.height.saturating_sub(4));
        let help_x = area.x + (area.width.saturating_sub(help_width)) / 2;
        let help_y = area.y + (area.height.saturating_sub(help_height)) / 2;
        let help_area = Rect::new(help_x, help_y, help_width, help_height);

        // Clear background
        for y in help_area.y..help_area.y + help_area.height {
            for x in help_area.x..help_area.x + help_area.width {
                buf.set_string(x, y, " ", Style::default().bg(Color::Black));
            }
        }

        let help_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(TEXT_BRIGHT))
            .title(" ? Help ");

        let inner = help_block.inner(help_area);
        help_block.render(help_area, buf);

        let help_text = vec![
            Line::from(Span::styled("Navigation", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  Tab      Switch panes (Deck A/Pads/Deck B/Master)"),
            Line::from("  Enter    Select pane / Enter control mode"),
            Line::from("  Esc      Go back one level"),
            Line::from("  hjkl     Navigate controls / adjust values"),
            Line::from(""),
            Line::from(Span::styled("Editing", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  Enter    Edit control / Toggle button"),
            Line::from("  0        Reset to default"),
            Line::from("  c        Center (pan/crossfader)"),
            Line::from(""),
            Line::from(Span::styled("Source & Pads", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  A/B      Open source picker for deck A/B"),
            Line::from("  P        Toggle pad trigger mode"),
            Line::from("  Enter    Assign sample (in pads pane)"),
            Line::from(""),
            Line::from(Span::styled("Quick Keys", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  m/s      Mute / Solo"),
            Line::from("  x        Cycle crossfader curve"),
            Line::from("  q/?      Quit / Toggle help"),
        ];

        let paragraph = Paragraph::new(help_text).wrap(Wrap { trim: true });
        paragraph.render(inner, buf);
    }
    
    fn render_source_picker(&self, area: Rect, buf: &mut Buffer, deck: Deck, picker: &SourcePickerState) {
        // Centered popup
        let popup_width = 60u16.min(area.width.saturating_sub(4));
        let popup_height = 20u16.min(area.height.saturating_sub(4));
        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

        // Clear background
        Clear.render(popup_area, buf);
        for y in popup_area.y..popup_area.y + popup_area.height {
            for x in popup_area.x..popup_area.x + popup_area.width {
                buf.set_string(x, y, " ", Style::default().bg(BG_POPUP));
            }
        }

        let title = match deck {
            Deck::A => " SOURCE [Deck A] ",
            Deck::B => " SOURCE [Deck B] ",
            Deck::C => " SOURCE [Deck C] ",
        };
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_ACTIVE))
            .title(Span::styled(title, Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        // Layout: tabs, search, list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Tabs
                Constraint::Length(1),  // Search input
                Constraint::Min(5),     // File list
                Constraint::Length(1),  // Hint
            ])
            .split(inner);

        // Tabs — scroll horizontally so the active tab is always visible
        let viewport_width = chunks[0].width as usize;
        // Ensure the active tab is visible (mutates tab_scroll_offset through interior mutability)
        // Since we can't mutate picker here, we compute the offset inline
        let tab_widths: Vec<(SourcePickerTab, usize)> = vec![
            (SourcePickerTab::MpvSockets, 16),
            (SourcePickerTab::AudioFiles, 16),
            (SourcePickerTab::SuperCollider, 18),
            (SourcePickerTab::DeckActions, 17),
        ];
        let mut tab_x = 0;
        let mut active_x = 0;
        let mut active_width = 16;
        for (tab, width) in &tab_widths {
            if *tab == picker.tab {
                active_x = tab_x;
                active_width = *width;
            }
            tab_x += width;
        }
        let mut tab_scroll_offset = picker.tab_scroll_offset;
        if active_x < tab_scroll_offset {
            tab_scroll_offset = active_x;
        } else if active_x + active_width > tab_scroll_offset + viewport_width {
            tab_scroll_offset = active_x + active_width - viewport_width;
        }

        let all_tabs: Vec<(SourcePickerTab, &str, &str)> = vec![
            (SourcePickerTab::MpvSockets, " [MPV Sockets] ", "  MPV Sockets  "),
            (SourcePickerTab::AudioFiles, " [Audio Files] ", "  Audio Files  "),
            (SourcePickerTab::SuperCollider, " [SuperCollider] ", "  SuperCollider  "),
            (SourcePickerTab::DeckActions, " [Deck Actions] ", "  Deck Actions  "),
        ];

        // Build the full tab line as styled graphemes, then render the visible slice
        let mut tab_line = Line::default();
        let mut char_offset = 0;
        for (tab, active_label, inactive_label) in &all_tabs {
            let label = if picker.tab == *tab { *active_label } else { *inactive_label };
            let is_active = picker.tab == *tab;
            let style = if is_active {
                Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_DEFAULT)
            };
            for ch in label.chars() {
                if char_offset >= tab_scroll_offset && char_offset < tab_scroll_offset + viewport_width {
                    tab_line.spans.push(Span::styled(ch.to_string(), style));
                }
                char_offset += 1;
            }
        }
        buf.set_line(chunks[0].x, chunks[0].y, &tab_line, chunks[0].width);

        // Update scroll offset so the active tab stays visible
        // (will be applied on next keypress via ensure_tab_visible)

        // Search input with mode indicator
        let mode_label = match picker.input_mode {
            PickerInputMode::Normal => Span::styled(" NOR ", Style::default().fg(Color::Black).bg(TEXT_DEFAULT)),
            PickerInputMode::Insert => Span::styled(" INS ", Style::default().fg(Color::Black).bg(STATUS_PLAYING)),
        };
        let search_text = format!(" > {}_", picker.query);
        let search_line = Line::from(vec![
            mode_label,
            Span::raw(" "),
            Span::styled(&search_text, Style::default().fg(TEXT_BRIGHT)),
        ]);
        buf.set_line(chunks[1].x, chunks[1].y, &search_line, chunks[1].width);

        // File list
        let list_area = chunks[2];
        let visible_items = list_area.height as usize;

        for (i, &item_idx) in picker.filtered.iter().enumerate().skip(picker.scroll_offset).take(visible_items) {
            if let Some(item) = picker.items.get(item_idx) {
                let y = list_area.y + i as u16 - picker.scroll_offset as u16;
                if y >= list_area.y + list_area.height {
                    break;
                }

                let is_selected = i == picker.selected;
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(BORDER_ACTIVE)
                } else {
                    Style::default().fg(TEXT_BRIGHT)
                };
                
                let icon = if item.is_socket { "⚡" } else if item.is_udp { "◉ " } else { "♪ " };
                let line = format!("{} {}", icon, item.name);
                let display = if line.len() > list_area.width as usize {
                    format!("{}…", &line[..list_area.width as usize - 1])
                } else {
                    format!("{:width$}", line, width = list_area.width as usize)
                };
                
                buf.set_string(list_area.x, y, &display, style);
            }
        }

        // Hint line
        let hint = match picker.input_mode {
            PickerInputMode::Normal => "i:insert  j/k:nav  h/l:tabs  g/G:top/bottom  Enter:select  Esc:quit",
            PickerInputMode::Insert => "Esc:normal  Tab:switch  Enter:select  Type to filter",
        };
        buf.set_string(chunks[3].x, chunks[3].y, hint, Style::default().fg(TEXT_GHOST));
    }

    fn render_sample_picker_inline(&self, area: Rect, buf: &mut Buffer, pad_idx: usize, picker: &SourcePickerState) {
        let key = PAD_KEYS[pad_idx / 4][pad_idx % 4].to_ascii_uppercase();
        let title = format!(" SAMPLE [{}] ", key);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_ACTIVE))
            .title(Span::styled(title, Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)));

        let inner = block.inner(area);
        block.render(area, buf);

        self.render_sample_picker_content(inner, buf, pad_idx, picker);
    }

    fn render_sample_picker_content(&self, area: Rect, buf: &mut Buffer, _pad_idx: usize, picker: &SourcePickerState) {

        // Layout: path, search, list, hint
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Current path
                Constraint::Length(1),  // Search input
                Constraint::Min(5),     // File list
                Constraint::Length(1),  // Hint
            ])
            .split(area);

        // Current path
        let rel_path = picker.relative_path();
        let path_display = if rel_path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", rel_path)
        };
        let path_truncated = if path_display.len() > chunks[0].width as usize {
            format!("…{}", &path_display[path_display.len() - chunks[0].width as usize + 1..])
        } else {
            path_display
        };
        buf.set_string(chunks[0].x, chunks[0].y, &path_truncated, Style::default().fg(TEXT_DEFAULT));

        // Search input with mode indicator
        let mode_label = match picker.input_mode {
            PickerInputMode::Normal => Span::styled(" NOR ", Style::default().fg(Color::Black).bg(TEXT_DEFAULT)),
            PickerInputMode::Insert => Span::styled(" INS ", Style::default().fg(Color::Black).bg(STATUS_PLAYING)),
        };
        let search_text = format!(" > {}_", picker.query);
        let search_line = Line::from(vec![
            mode_label,
            Span::raw(" "),
            Span::styled(&search_text, Style::default().fg(TEXT_BRIGHT)),
        ]);
        buf.set_line(chunks[1].x, chunks[1].y, &search_line, chunks[1].width);

        // File list
        let list_area = chunks[2];
        let visible_items = list_area.height as usize;

        for (i, &item_idx) in picker.filtered.iter().enumerate().skip(picker.scroll_offset).take(visible_items) {
            if let Some(item) = picker.items.get(item_idx) {
                let y = list_area.y + i as u16 - picker.scroll_offset as u16;
                if y >= list_area.y + list_area.height {
                    break;
                }

                let is_selected = i == picker.selected;
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(BORDER_ACTIVE)
                } else if item.is_dir {
                    Style::default().fg(DECK_A)
                } else {
                    Style::default().fg(TEXT_BRIGHT)
                };
                
                // Nerd Font icons: folder or audio file
                let icon = if item.is_dir { "\u{f07b}" } else { "\u{f001}" };
                let line = format!("{} {}", icon, item.name);
                let display = if line.len() > list_area.width as usize {
                    let truncated: String = line.chars().take(list_area.width as usize - 1).collect();
                    format!("{}…", truncated)
                } else {
                    format!("{:width$}", line, width = list_area.width as usize)
                };
                
                buf.set_string(list_area.x, y, &display, style);
            }
        }

        // Hint line
        let hint = match picker.input_mode {
            PickerInputMode::Normal => "i:insert  j/k:nav  h/l:dirs  Space:preview  Enter:select",
            PickerInputMode::Insert => "Esc:normal  Enter:select  Backspace:up dir  Type to filter",
        };
        buf.set_string(chunks[3].x, chunks[3].y, hint, Style::default().fg(TEXT_GHOST));
    }

    /// Render output device picker overlay
    fn render_output_picker(&self, area: Rect, buf: &mut Buffer) {
        let (title, devices, selected_idx) = match self.output_picker_target {
            OutputPickerTarget::Master => (
                " OUTPUT [Master] ",
                self.master_output_devices,
                self.selected_master_output_idx,
            ),
            OutputPickerTarget::Cue => (
                " OUTPUT [CUE] ",
                self.cue_output_devices,
                self.selected_cue_output_idx,
            ),
        };

        // Centered popup
        let popup_width = 50u16.min(area.width.saturating_sub(4));
        let popup_height = (devices.len() as u16 + 6).min(area.height.saturating_sub(4));
        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

        // Clear background
        Clear.render(popup_area, buf);
        for y in popup_area.y..popup_area.y + popup_area.height {
            for x in popup_area.x..popup_area.x + popup_area.width {
                buf.set_string(x, y, " ", Style::default().bg(BG_POPUP));
            }
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_ACTIVE))
            .title(Span::styled(title.trim(), Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        // Layout: device list, hint
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),     // Device list
                Constraint::Length(1),  // Hint
            ])
            .split(inner);

        // Device list
        let list_area = chunks[0];
        for (i, device) in devices.iter().enumerate() {
            if i as u16 >= list_area.height {
                break;
            }
            
            let is_selected = i == selected_idx;
            let style = if is_selected {
                Style::default().fg(Color::Black).bg(BORDER_ACTIVE)
            } else {
                Style::default().fg(TEXT_BRIGHT)
            };
            
            let display = if device.len() > list_area.width as usize {
                format!("{}…", &device[..list_area.width as usize - 1])
            } else {
                format!("{:width$}", device, width = list_area.width as usize)
            };
            
            buf.set_string(list_area.x, list_area.y + i as u16, &display, style);
        }

        // Hint line
        let hint = "j/k:nav  Enter:select  Esc:cancel";
        buf.set_string(chunks[1].x, chunks[1].y, hint, Style::default().fg(TEXT_GHOST));
    }
    
    /// Render debug log at the bottom of the screen
    fn render_debug_log(&self, area: Rect, buf: &mut Buffer, log: &[String]) {
        use ratatui::widgets::{Block, Borders, Paragraph};
        use ratatui::text::{Line, Span};
        
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" DEBUG ")
            .border_style(Style::default().fg(BORDER_DEFAULT));
        
        let inner = block.inner(area);
        block.render(area, buf);
        
        // Show last N lines that fit in the area
        let max_lines = inner.height as usize;
        let start = log.len().saturating_sub(max_lines);
        let visible_logs: Vec<Line> = log[start..]
            .iter()
            .map(|msg| Line::from(Span::styled(msg.clone(), Style::default().fg(TEXT_DEFAULT))))
            .collect();
        
        let paragraph = Paragraph::new(visible_logs);
        paragraph.render(inner, buf);
    }
}
