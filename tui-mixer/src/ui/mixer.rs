//! Main mixer layout that arranges all channel strips

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::state::{MixerState, SelectionFocus, GlobalControl, SamplePadGrid};
use crate::ui::channel::{ChannelStrip, MasterStrip};
use crate::ui::widgets::{Crossfader, HorizontalBar};
use crate::app::{Deck, SelectedPane, SourcePickerState, SourcePickerTab, PickerInputMode};

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
    
    pub fn selected_pad_idx(mut self, idx: Option<usize>) -> Self {
        self.selected_pad_idx = idx;
        self
    }
}

impl<'a> Widget for MixerView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Always use full-width layout
        self.render_full_width_layout(area, buf);

        // Render help overlay if enabled
        if self.show_help {
            self.render_help_overlay(area, buf);
        }
        
        // Render source picker overlay if active
        if let Some((deck, picker)) = self.source_picker {
            self.render_source_picker(area, buf, deck, picker);
        }
        
        // Render sample picker overlay if active
        if let Some((pad_idx, picker)) = self.sample_picker {
            self.render_sample_picker(area, buf, pad_idx, picker);
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
        let title = format!("TIDAL {}", mode_str);
        
        let title_style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        buf.set_string(area.x + 1, area.y, &title, title_style);

        // Curve indicator
        let curve = format!("curve:{}", self.state.dj.crossfader_curve.label().to_lowercase());
        let curve_x = area.x + 12;
        buf.set_string(curve_x, area.y, &curve, Style::default().fg(Color::DarkGray));

        // Mode/status on the right
        let status = match self.state.focus {
            SelectionFocus::Channel(_) => format!(
                "{}/{}  {}",
                self.state.selected_channel + 1,
                self.state.channels.len(),
                self.state.selected_control.label()
            ),
            SelectionFocus::Global => format!(
                "DJ  {}",
                self.state.selected_global.label()
            ),
        };
        let status_x = area.x + area.width.saturating_sub(status.len() as u16 + 1);
        buf.set_string(status_x, area.y, &status, Style::default().fg(Color::DarkGray));

        // Thin separator line
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Style::default().fg(Color::Rgb(40, 40, 40)));
    }

    /// Full-width mixer layout - DJ section centered, A/B decks on sides
    fn render_mixer_full_width(&self, area: Rect, buf: &mut Buffer) {
        // Layout: [Deck A (max 16)] [DJ Center (stretches)] [Deck B (max 16)] [Master (10)]
        let deck_max_width = 16u16;
        let master_width = 10u16;
        
        // Calculate minimum DJ center width (enough for 4x4 pads + borders)
        let min_dj_width = 20u16;
        
        // Calculate deck widths - capped at max, but shrink if needed
        let total_fixed = deck_max_width * 2 + master_width + min_dj_width;
        let deck_width = if area.width >= total_fixed {
            deck_max_width
        } else {
            // Shrink decks proportionally
            ((area.width.saturating_sub(master_width + min_dj_width)) / 2).max(10)
        };
        
        // DJ center gets the remaining space (stretches)
        let dj_center_width = area.width.saturating_sub(deck_width * 2 + master_width);
        
        let constraints = vec![
            Constraint::Length(deck_width),      // Deck A
            Constraint::Length(dj_center_width), // DJ Center (pads + crossfader)
            Constraint::Length(deck_width),      // Deck B
            Constraint::Length(master_width),    // Master
        ];

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        // Only show selected control when in control select mode (or editing)
        let show_control = self.control_select || self.editing;

        // Deck A (channel 0)
        if let Some(channel) = self.state.channels.get(self.state.dj.deck_a_channel) {
            let pane_selected = self.selected_pane == SelectedPane::DeckA;
            let control = if show_control && pane_selected { Some(self.state.selected_control) } else { None };
            ChannelStrip::new(channel)
                .selected(pane_selected, control)
                .deck_label(Some("A"))
                .deck_color(Color::Cyan)
                .editing(self.editing && pane_selected)
                .frame(self.frame)
                .render(chunks[0], buf);
        }

        // DJ Center Section (crossfader + pads)
        self.render_dj_center(chunks[1], buf);

        // Deck B (channel 1)
        if let Some(channel) = self.state.channels.get(self.state.dj.deck_b_channel) {
            let pane_selected = self.selected_pane == SelectedPane::DeckB;
            let control = if show_control && pane_selected { Some(self.state.selected_control) } else { None };
            ChannelStrip::new(channel)
                .selected(pane_selected, control)
                .deck_label(Some("B"))
                .deck_color(Color::Magenta)
                .editing(self.editing && pane_selected)
                .frame(self.frame)
                .render(chunks[2], buf);
        }

        // Master - pane is selected if we're on Master pane, controls only in control_select mode
        let master_pane_selected = self.selected_pane == SelectedPane::Master;
        let master_control_selected = master_pane_selected && show_control;
        MasterStrip::new(&self.state.master)
            .pane_selected(master_pane_selected)
            .selected(master_control_selected)
            .render(chunks[3], buf);
    }

    /// Centered DJ section with crossfader and pads
    fn render_dj_center(&self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.selected_pane == SelectedPane::DjCenter {
            Color::Yellow
        } else {
            Color::Rgb(50, 50, 50)
        };
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        block.render(area, buf);

        // Layout: top controls, pads (centered), crossfader at bottom
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),  // Top: CUE/PHONES/BOOTH
                Constraint::Min(4),     // Pads (centered)
                Constraint::Length(3),  // Crossfader at bottom (needs 3 rows for cap)
            ])
            .split(inner);

        // Top controls
        self.render_dj_controls_inline(sections[0], buf);

        // Pads (centered in the middle section)
        self.render_pad_grid_centered(sections[1], buf);

        // Crossfader at bottom - only highlight in control_select mode
        let show_control = self.control_select || self.editing;
        let xfader_selected = show_control 
            && self.selected_pane == SelectedPane::DjCenter
            && self.selected_pad_idx.is_none()
            && self.state.selected_global == GlobalControl::Crossfader;
        Crossfader::new(self.state.dj.crossfader)
            .selected(xfader_selected)
            .labels("A", "B")
            .render(sections[2], buf);
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
        
        let border_color = if is_selected {
            Color::Yellow
        } else if is_triggered {
            Color::Yellow
        } else if pad.has_sample() {
            // Brighter color for pads with samples
            let (r, g, b) = pad.color;
            Color::Rgb(r.saturating_add(30).min(255), g.saturating_add(30).min(255), b.saturating_add(30).min(255))
        } else {
            Color::Rgb(40, 40, 40)
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if pad.has_sample() {
            Style::default().fg(Color::Rgb(120, 120, 120))
        } else {
            Style::default().fg(Color::Rgb(70, 70, 70))
        };
        buf.set_string(cx, cy, &key.to_string(), key_style);
    }

    fn render_dj_controls_inline(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 12 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
            ])
            .split(area);

        // Only highlight controls in control_select mode and when no pad is selected
        let show_control = (self.control_select || self.editing) 
            && self.selected_pane == SelectedPane::DjCenter
            && self.selected_pad_idx.is_none();

        // CUE MIX
        let cue_selected = show_control && self.state.selected_global == GlobalControl::CueMix;
        self.render_mini_slider(chunks[0], buf, "CUE", self.state.dj.cue_mix, cue_selected);

        // PHONES
        let ph_selected = show_control && self.state.selected_global == GlobalControl::HeadphoneVolume;
        self.render_mini_slider(chunks[1], buf, "PH", self.state.dj.headphone_volume, ph_selected);

        // BOOTH
        let bt_selected = show_control && self.state.selected_global == GlobalControl::BoothVolume;
        self.render_mini_slider(chunks[2], buf, "BT", self.state.dj.booth_volume, bt_selected);
    }

    fn render_mini_slider(&self, area: Rect, buf: &mut Buffer, label: &str, value: f32, selected: bool) {
        let style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        
        buf.set_string(area.x, area.y, label, style);
        
        let bar_w = area.width.saturating_sub(3) as usize;
        if bar_w > 0 {
            let filled = (value * bar_w as f32) as usize;
            let bar: String = (0..bar_w).map(|i| if i < filled { '━' } else { '─' }).collect();
            buf.set_string(area.x + 3, area.y, &bar, style);
        }
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        // Single line footer with context hints
        let hint = if self.editing {
            "hjkl:adjust  0:reset  c:center  Enter/Esc:done"
        } else if self.pads.active {
            "4567/RTYU/FGHJ/VBNM:trigger  Space:stop  P/Esc:exit"
        } else {
            match self.state.focus {
                SelectionFocus::Channel(_) => {
                    if self.state.selected_control.is_continuous() {
                        "hjkl:nav  Enter:edit  m:mute  s:solo  Tab:DJ  ?:help"
                    } else {
                        "hjkl:nav  Enter:toggle  Tab:DJ  ?:help"
                    }
                }
                SelectionFocus::Global => "hjkl:nav  Enter:edit  Tab:CH  ?:help",
            }
        };

        buf.set_string(area.x + 1, area.y, hint, Style::default().fg(Color::Rgb(60, 60, 60)));

        // Mode indicators on right side
        let mut right_x = area.x + area.width;
        
        if self.pads.active {
            let label = if self.pads.config_mode { "PAD CFG" } else { "PADS" };
            right_x = right_x.saturating_sub(label.len() as u16 + 2);
            buf.set_string(right_x, area.y, label, 
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD));
        }

        if self.state.solo_active {
            right_x = right_x.saturating_sub(6);
            buf.set_string(right_x, area.y, "SOLO", 
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
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
            .border_style(Style::default().fg(Color::White))
            .title(" ? Help ");

        let inner = help_block.inner(help_area);
        help_block.render(help_area, buf);

        let help_text = vec![
            Line::from(Span::styled("Navigation", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  Tab      Switch panes (A/DJ/B/Master)"),
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
            Line::from("  Enter    Assign sample (in DJ center pads)"),
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
                buf.set_string(x, y, " ", Style::default().bg(Color::Rgb(20, 20, 20)));
            }
        }

        let title = match deck {
            Deck::A => " Select Source for Deck A ",
            Deck::B => " Select Source for Deck B ",
        };
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

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

        // Tabs
        let tab_sockets = if picker.tab == SourcePickerTab::MpvSockets {
            Span::styled(" [MPV Sockets] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("  MPV Sockets  ", Style::default().fg(Color::Rgb(100, 100, 100)))
        };
        let tab_files = if picker.tab == SourcePickerTab::AudioFiles {
            Span::styled(" [Audio Files] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("  Audio Files  ", Style::default().fg(Color::Rgb(100, 100, 100)))
        };
        let tab_sc = if picker.tab == SourcePickerTab::SuperCollider {
            Span::styled(" [SuperCollider] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("  SuperCollider  ", Style::default().fg(Color::Rgb(100, 100, 100)))
        };
        buf.set_line(chunks[0].x, chunks[0].y, &Line::from(vec![tab_sockets, tab_files, tab_sc]), chunks[0].width);

        // Search input with mode indicator
        let mode_label = match picker.input_mode {
            PickerInputMode::Normal => Span::styled(" NOR ", Style::default().fg(Color::Black).bg(Color::Rgb(100, 100, 100))),
            PickerInputMode::Insert => Span::styled(" INS ", Style::default().fg(Color::Black).bg(Color::Green)),
        };
        let search_text = format!(" > {}_", picker.query);
        let search_line = Line::from(vec![
            mode_label,
            Span::raw(" "),
            Span::styled(&search_text, Style::default().fg(Color::White)),
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
                    Style::default().fg(Color::Black).bg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
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
            PickerInputMode::Normal => "i:insert  j/k:nav  g/G:top/bottom  Tab:switch  Enter:select  Esc:quit",
            PickerInputMode::Insert => "Esc:normal  Tab:switch  Enter:select  Type to filter",
        };
        buf.set_string(chunks[3].x, chunks[3].y, hint, Style::default().fg(Color::Rgb(80, 80, 80)));
    }
    
    fn render_sample_picker(&self, area: Rect, buf: &mut Buffer, pad_idx: usize, picker: &SourcePickerState) {
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
                buf.set_string(x, y, " ", Style::default().bg(Color::Rgb(20, 20, 20)));
            }
        }

        let pad_num = pad_idx + 1;
        let title = format!(" Select Sample for Pad {} ", pad_num);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        // Layout: path, search, list, hint
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Current path
                Constraint::Length(1),  // Search input
                Constraint::Min(5),     // File list
                Constraint::Length(1),  // Hint
            ])
            .split(inner);

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
        buf.set_string(chunks[0].x, chunks[0].y, &path_truncated, Style::default().fg(Color::Rgb(100, 100, 100)));

        // Search input with mode indicator
        let mode_label = match picker.input_mode {
            PickerInputMode::Normal => Span::styled(" NOR ", Style::default().fg(Color::Black).bg(Color::Rgb(100, 100, 100))),
            PickerInputMode::Insert => Span::styled(" INS ", Style::default().fg(Color::Black).bg(Color::Green)),
        };
        let search_text = format!(" > {}_", picker.query);
        let search_line = Line::from(vec![
            mode_label,
            Span::raw(" "),
            Span::styled(&search_text, Style::default().fg(Color::White)),
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
                    Style::default().fg(Color::Black).bg(Color::Yellow)
                } else if item.is_dir {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                
                // Icon: folder or audio file
                let icon = if item.is_dir { "📁" } else { "♪ " };
                let line = format!("{} {}", icon, item.name);
                let display = if line.chars().count() > list_area.width as usize {
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
        buf.set_string(chunks[3].x, chunks[3].y, hint, Style::default().fg(Color::Rgb(80, 80, 80)));
    }
}
