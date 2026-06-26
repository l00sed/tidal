//! Channel strip component - represents a single mixer channel

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Widget},
};

use crate::state::{ChannelControl, GlobalControl, MixerChannel};
use crate::ui::colors::*;
use crate::ui::widgets::{DeckIndicator, Fader, LevelMeter};

/// A complete channel strip widget - minimalist futuristic design
pub struct ChannelStrip<'a> {
    channel: &'a MixerChannel,
    selected: bool,
    selected_control: Option<ChannelControl>,
    deck_label: Option<&'a str>,
    deck_color: Color,
    editing: bool,
    frame: u8,
    show_border: bool,
}

impl<'a> ChannelStrip<'a> {
    pub fn new(channel: &'a MixerChannel) -> Self {
        Self {
            channel,
            selected: false,
            selected_control: None,
            deck_label: None,
            deck_color: Color::White,
            editing: false,
            frame: 0,
            show_border: true,
        }
    }

    pub fn show_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    pub fn selected(mut self, selected: bool, control: Option<ChannelControl>) -> Self {
        self.selected = selected;
        if selected {
            self.selected_control = control;
        }
        self
    }

    pub fn deck_label(mut self, label: Option<&'a str>) -> Self {
        self.deck_label = label;
        self
    }

    pub fn deck_color(mut self, color: Color) -> Self {
        self.deck_color = color;
        self
    }

    #[allow(dead_code)]
    pub fn playing(self, _playing: bool) -> Self {
        self // Now using channel.playing directly
    }

    pub fn editing(mut self, editing: bool) -> Self {
        self.editing = editing;
        self
    }

    pub fn frame(mut self, frame: u8) -> Self {
        self.frame = frame;
        self
    }

    fn is_control_selected(&self, control: ChannelControl) -> bool {
        self.selected && self.selected_control == Some(control)
    }

    fn is_control_editing(&self, control: ChannelControl) -> bool {
        self.editing && self.is_control_selected(control)
    }

    fn format_freq(freq: f32) -> String {
        if freq >= 1000.0 {
            format!("{:.0}k", freq / 1000.0)
        } else {
            format!("{:.0}", freq)
        }
    }

    fn format_db(db: f32) -> String {
        if db > 0.0 {
            format!("+{:.0}", db)
        } else if db < 0.0 {
            format!("{:.0}", db)
        } else {
            "0".to_string()
        }
    }

    fn freq_to_normalized(freq: f32) -> f32 {
        let log_min = 20f32.log10();
        let log_max = 20000f32.log10();
        let log_freq = freq.clamp(20.0, 20000.0).log10();
        (log_freq - log_min) / (log_max - log_min)
    }
}

impl<'a> Widget for ChannelStrip<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 18 {
            let style = Style::default().fg(Color::Rgb(40, 40, 40));
            buf.set_string(area.x + 1, area.y + area.height / 2, "···", style);
            return;
        }

        // Determine inner area and render border if enabled
        let inner = if self.show_border {
            // Minimalist border - thin line style
            let border_color = if self.selected {
                if self.editing { Color::Yellow } else { Color::Rgb(120, 100, 0) }
            } else if self.deck_label.is_some() {
                if self.channel.connected {
                    match self.deck_color {
                        Color::Cyan => DECK_A_BRIGHT,
                        Color::Magenta => Color::Rgb(255, 100, 255),
                        _ => self.deck_color,
                    }
                } else {
                    Color::Rgb(60, 60, 60)
                }
            } else if self.channel.connected {
                FADER_FILL
            } else {
                Color::Rgb(40, 40, 40)
            };

            let border_style = Style::default().fg(border_color);

            let title = if let Some(deck) = self.deck_label {
                if self.channel.connected {
                    if deck.contains('A') { " A ● " } else if deck.contains('B') { " B ● " } else { " C ● " }
                } else if deck.contains('A') { " A ○ " } else if deck.contains('B') { " B ○ " } else { " C ○ " }
            } else {
                ""
            };

            let title_style = if self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if self.channel.connected {
                Style::default().fg(border_color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(80, 80, 80))
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(title, title_style));

            let inner_area = block.inner(area);
            block.render(area, buf);
            inner_area
        } else {
            area
        };

        // Minimalist layout
        let has_deck = self.deck_label.is_some();
        
        let constraints = if has_deck {
            vec![
                Constraint::Length(1),  // Scrubber (hidden until track loaded)
                Constraint::Length(7),  // Deck indicator (ring + padding + marquee)
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // BPM display
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // EQ High
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // EQ Mid
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // EQ Low
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // HPF
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // LPF
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // Pan
                Constraint::Length(1),  // Separator
                Constraint::Min(5),     // Fader + Meter
                Constraint::Length(1),  // Separator before buttons
                Constraint::Length(1),  // Buttons row
            ]
        } else {
            vec![
                Constraint::Length(2),  // EQ High
                Constraint::Length(2),  // EQ Mid
                Constraint::Length(2),  // EQ Low
                Constraint::Length(2),  // HPF
                Constraint::Length(2),  // LPF
                Constraint::Length(2),  // Pan
                Constraint::Min(7),     // Fader + Meter
                Constraint::Length(1),  // Separator before buttons
                Constraint::Length(1),  // Buttons row
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        let mut idx = 0;

        // Deck indicator (only for deck channels)
        if has_deck {
            // Scrubber (only shown when track loaded)
            let has_track = self.channel.connected && self.channel.duration > 0.0;
            if has_track {
                self.render_scrub_bar(chunks[idx], buf, self.is_control_selected(ChannelControl::Scrub));
            }
            idx += 1;

            let deck_label = self.deck_label.unwrap_or("");
            let deck_char = if deck_label.contains('A') { 'A' } else if deck_label.contains('B') { 'B' } else { 'C' };
            let source_name = if self.channel.connected {
                Some(self.channel.name.clone())
            } else {
                None
            };
            DeckIndicator::new(deck_char)
                .playing(self.channel.playing)
                .speed(self.channel.playback_speed)
                .frame(self.frame)
                .color(self.deck_color)
                .source_name(source_name)
                .connected(self.channel.connected)
                .selected(self.is_control_selected(ChannelControl::PlayPause))
                .scrub(self.channel.scrub_direction, self.channel.scrub_speed)
                .render(chunks[idx], buf);
            idx += 1;

            // Separator
            self.draw_separator(chunks[idx], buf);
            idx += 1;

            // BPM display (always shown) - label left, value right, highlighted when focused
            let bpm_selected = self.is_control_selected(ChannelControl::Bpm);
            let bpm_editing = self.is_control_editing(ChannelControl::Bpm);
            let bpm_value = if let Some(bpm) = self.channel.bpm {
                format!("{:.0}", bpm)
            } else {
                "---".to_string()
            };
            let label_style = if bpm_editing {
                Style::default().fg(TEXT_EDITING).add_modifier(Modifier::BOLD)
            } else if bpm_selected {
                Style::default().fg(TEXT_EDITING)
            } else {
                Style::default().fg(TEXT_DIM)
            };
            let value_style = if bpm_editing {
                Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
            } else if bpm_selected {
                Style::default().fg(TEXT_BRIGHT)
            } else {
                Style::default().fg(TEXT_DEFAULT)
            };
            // Label left-aligned, value right-aligned
            let bpm_area = chunks[idx];
            buf.set_string(bpm_area.x, bpm_area.y, "BPM", label_style);
            // Show speed factor when selected (e.g. x0.85)
            if bpm_selected || bpm_editing {
                let base = self.channel.base_bpm;
                let speed = if base > 0.0 { self.channel.target_bpm / base } else { 1.0 };
                let factor_str = format!("x{:.2}", speed);
                buf.set_string(bpm_area.x + 4, bpm_area.y, &factor_str, Style::default().fg(TEXT_DIM));
            }
            let val_right = bpm_area.x + bpm_area.width;
            buf.set_string(val_right.saturating_sub(4), bpm_area.y, &format!("{:>4}", bpm_value), value_style);
            idx += 1;

            // Separator
            self.draw_separator(chunks[idx], buf);
            idx += 1;
        }

        // EQ bars with kill switches - compact single-line style
        self.render_eq_bar_with_kill(chunks[idx], buf, "H", 
            self.channel.eq_high, self.channel.eq_high_kill,
            self.is_control_selected(ChannelControl::EqHigh),
            self.is_control_selected(ChannelControl::EqHighKill));
        idx += 1;

        // Separator
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        self.render_eq_bar_with_kill(chunks[idx], buf, "M",
            self.channel.eq_mid, self.channel.eq_mid_kill,
            self.is_control_selected(ChannelControl::EqMid),
            self.is_control_selected(ChannelControl::EqMidKill));
        idx += 1;

        // Separator
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        self.render_eq_bar_with_kill(chunks[idx], buf, "L",
            self.channel.eq_low, self.channel.eq_low_kill,
            self.is_control_selected(ChannelControl::EqLow),
            self.is_control_selected(ChannelControl::EqLowKill));
        idx += 1;

        // Separator
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        // Filter bars
        self.render_compact_bar(chunks[idx], buf, "↑",
            Self::freq_to_normalized(self.channel.hpf_freq),
            Self::format_freq(self.channel.hpf_freq),
            false, self.is_control_selected(ChannelControl::HighPassFilter));
        idx += 1;

        // Separator
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        self.render_compact_bar(chunks[idx], buf, "↓",
            Self::freq_to_normalized(self.channel.lpf_freq),
            Self::format_freq(self.channel.lpf_freq),
            false, self.is_control_selected(ChannelControl::LowPassFilter));
        idx += 1;

        // Separator
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        // Pan
        self.render_pan_bar(chunks[idx], buf, self.channel.pan,
            self.is_control_selected(ChannelControl::Pan), false);
        idx += 1;

        // Separator between pan and fader
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        // Fader and meter area
        let fader_area = chunks[idx];
        idx += 1;

        if fader_area.width >= 5 {
            let fader_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(3), Constraint::Min(2)])
                .split(fader_area);

            LevelMeter::new(0.0)
                .stereo(self.channel.rms_left, self.channel.rms_right)
                .peaks(self.channel.peak_left, self.channel.peak_right)
                .render(fader_chunks[0], buf);

            let db = self.channel.fader_db();
            let db_label = if self.channel.muted {
                "×".to_string()
            } else if db <= -60.0 {
                "∞".to_string()
            } else {
                format!("{:+.0}", db)
            };
            Fader::new(self.channel.fader)
                .selected(self.is_control_selected(ChannelControl::Fader))
                .label(db_label)
                .render(fader_chunks[1], buf);
        }

        // Separator before buttons
        self.draw_separator(chunks[idx], buf);
        idx += 1;

        // Compact button row: M S C
        let btn_area = chunks[idx];
        let is_cue = self.deck_label == Some("C");
        self.render_button_row(btn_area, buf, is_cue);
    }
}

impl<'a> ChannelStrip<'a> {
    /// Draw a plain horizontal separator line (no junction characters).
    fn draw_separator(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 1 || area.height < 1 {
            return;
        }
        let style = Style::default().fg(SEPARATOR);
        let y = area.y + area.height.saturating_sub(1);
        
        // Plain horizontal line - no junction characters
        for x in area.x..area.x + area.width {
            buf.set_string(x, y, "─", style);
        }
    }

    /// Compute the border color for this channel strip
    #[allow(dead_code)]
    fn border_color(&self) -> Color {
        if self.selected {
            if self.editing { Color::Yellow } else { Color::Rgb(120, 100, 0) }
        } else if self.deck_label.is_some() {
            if self.channel.connected {
                match self.deck_color {
                    Color::Cyan => DECK_A_BRIGHT,
                    Color::Magenta => Color::Rgb(255, 100, 255),
                    _ => self.deck_color,
                }
            } else {
                Color::Rgb(60, 60, 60)
            }
        } else if self.channel.connected {
            FADER_FILL
        } else {
            Color::Rgb(40, 40, 40)
        }
    }

    /// Render a compact single-line bar: [label][bar][value]
    #[allow(clippy::too_many_arguments)]
    fn render_compact_bar(&self, area: Rect, buf: &mut Buffer, 
                          label: &str, value: f32, display: String, 
                          bipolar: bool, selected: bool) {
        if area.width < 6 || area.height < 1 {
            return;
        }

        let y = area.y;
        let editing = selected && self.editing;
        
        // Label - brighter when editing
        let label_style = if editing {
            Style::default().fg(TEXT_EDITING).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(TEXT_EDITING)
        } else {
            Style::default().fg(TEXT_GHOST)
        };
        buf.set_string(area.x, y, label, label_style);

        // Fixed display width to keep bar centered regardless of value
        let display_width = 4u16;
        let bar_start = area.x + 2;
        let bar_width = area.width.saturating_sub(2 + display_width);
        
        if bar_width >= 3 {
            self.draw_mini_bar(buf, bar_start, y, bar_width as usize, value, bipolar, selected, editing);
        }

        // Value - right-aligned in fixed width, white when editing
        let val_x = area.x + area.width - display_width;
        let padded_display = format!("{:>4}", display);
        let val_style = if editing {
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(TEXT_BRIGHT)
        } else {
            Style::default().fg(TEXT_DEFAULT)
        };
        buf.set_string(val_x, y, &padded_display, val_style);
    }

    fn render_pan_bar(&self, area: Rect, buf: &mut Buffer, value: f32, selected: bool, editing: bool) {
        if area.width < 7 || area.height < 1 {
            return;
        }
        let y = area.y;

        let label_style = if editing {
            Style::default().fg(TEXT_EDITING).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(TEXT_EDITING)
        } else {
            Style::default().fg(TEXT_GHOST)
        };
        buf.set_string(area.x, y, "L", label_style);

        let bar_width = area.width.saturating_sub(4) as usize;
        if bar_width >= 3 {
            self.draw_mini_bar(buf, area.x + 2, y, bar_width, (value + 1.0) / 2.0, true, selected, editing);
        }

        buf.set_string(area.x + area.width - 1, y, "R", label_style);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_mini_bar(&self, buf: &mut Buffer, x: u16, y: u16, width: usize, value: f32, bipolar: bool, selected: bool, editing: bool) {
        if bipolar {
            let center = width / 2;
            let fill_pos = (value * width as f32) as usize;
            
            for i in 0..width {
                let ch = if i == center {
                    '│'
                } else {
                    '─'
                };
                
                let color = if editing || selected {
                    if (fill_pos > center && i > center && i <= fill_pos) ||
                       (fill_pos < center && i < center && i >= fill_pos) {
                        if editing {
                            TEXT_BRIGHT
                        } else if fill_pos > center { 
                            STATUS_PLAYING 
                        } else { 
                            STATUS_MUTED 
                        }
                    } else if editing { TEXT_DIM } else { METER_TRACK }
                } else {
                    METER_TRACK
                };
                
                buf.set_string(x + i as u16, y, ch.to_string(), Style::default().fg(color));
            }
        } else {
            let filled = (value * width as f32) as usize;
            for i in 0..width {
                let color = if i < filled {
                    if editing { 
                        TEXT_BRIGHT 
                    } else if selected { 
                        METER_FILL 
                    } else { 
                        METER_TRACK 
                    }
                } else { METER_TRACK };
                buf.set_string(x + i as u16, y, "─", Style::default().fg(color));
            }
        }
    }
    
    /// Render EQ bar with kill switch: [label][bar][×][value]
    #[allow(clippy::too_many_arguments)]
    fn render_eq_bar_with_kill(&self, area: Rect, buf: &mut Buffer, 
                                label: &str, eq_value: f32, killed: bool,
                                bar_selected: bool, kill_selected: bool) {
        if area.width < 8 || area.height < 1 {
            return;
        }

        let y = area.y;
        let bar_editing = bar_selected && self.editing;
        let kill_editing = kill_selected && self.editing;
        
        // Label
        let label_style = if bar_editing || kill_editing {
            Style::default().fg(TEXT_EDITING).add_modifier(Modifier::BOLD)
        } else if bar_selected || kill_selected {
            Style::default().fg(TEXT_EDITING)
        } else {
            Style::default().fg(TEXT_GHOST)
        };
        buf.set_string(area.x, y, label, label_style);

        // Kill switch (×) - positioned before the value
        let display_width = 4u16;
        let kill_x = area.x + area.width - display_width - 2;
        let kill_char = if killed { "×" } else { "○" };
        let kill_style = if kill_selected {
            if killed {
                Style::default().fg(STATUS_MUTED).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_EDITING)
            }
        } else if killed {
            Style::default().fg(STATUS_MUTED)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        buf.set_string(kill_x, y, kill_char, kill_style);

        // Bar area
        let bar_start = area.x + 2;
        let bar_width = (kill_x - bar_start).saturating_sub(1) as usize;
        
        if bar_width >= 3 {
            let value = (eq_value + 24.0) / 48.0;  // ±24dB range
            self.draw_mini_bar(buf, bar_start, y, bar_width, value, true, bar_selected, bar_editing);
        }

        // Value - right-aligned, dimmed if killed
        let val_x = area.x + area.width - display_width;
        let display = Self::format_db(eq_value);
        let padded_display = format!("{:>4}", display);
        let val_style = if killed {
            Style::default().fg(TEXT_DIM)
        } else if bar_editing {
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
        } else if bar_selected {
            Style::default().fg(TEXT_BRIGHT)
        } else {
            Style::default().fg(TEXT_DEFAULT)
        };
        buf.set_string(val_x, y, &padded_display, val_style);
    }
    
    /// Render scrub slider showing elapsed/total time with position bar
    fn render_scrub_bar(&self, area: Rect, buf: &mut Buffer, selected: bool) {
        if area.width < 4 || area.height < 1 {
            return;
        }

        let y = area.y;
        let editing = self.is_control_editing(ChannelControl::Scrub);

        let format_time = |secs: f32| -> String {
            let total = secs.max(0.0) as u32;
            let m = total / 60;
            let s = total % 60;
            format!("{}:{:02}", m, s)
        };

        let elapsed = format_time(self.channel.time_pos);
        let total = format_time(self.channel.duration);

        let position = if self.channel.duration > 0.0 {
            (self.channel.time_pos / self.channel.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let time_str = format!("{} / {}", elapsed, total);
        let time_x = area.x + area.width.saturating_sub(time_str.len() as u16);
        let time_style = if editing {
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(TEXT_BRIGHT)
        } else {
            Style::default().fg(TEXT_DEFAULT)
        };
        buf.set_string(time_x, y, &time_str, time_style);

        let bar_start = area.x;
        let bar_end = time_x.saturating_sub(1);
        if bar_end > bar_start + 2 {
            let bar_width = (bar_end - bar_start) as usize;
            let filled = (position * bar_width as f32) as usize;
            for i in 0..bar_width {
                let ch = if i == filled { "◆" } else { "─" };
                let color = if i < filled {
                    if editing { TEXT_BRIGHT } else { METER_FILL }
                } else {
                    METER_TRACK
                };
                buf.set_string(bar_start + i as u16, y, ch, Style::default().fg(color));
            }
        }
    }

    fn render_button_row(&self, area: Rect, buf: &mut Buffer, is_cue: bool) {
        if area.width < 3 {
            return;
        }

        // Two equal columns: M │ S (or M │ -> A for CUE)
        let sep_x = area.x + area.width / 2;
        let left_w = sep_x - area.x;
        let right_w = area.x + area.width - sep_x - 1;

        // Center each letter within its half (with 1-cell padding on each side)
        let m_x = area.x + 1 + (left_w - 2) / 2;
        
        // For CUE deck, right side shows "-> A" instead of "S"
        let (right_label, right_control, right_active) = if is_cue {
            ("-> A", ChannelControl::CueSendToA, false)
        } else {
            ("S", ChannelControl::Solo, self.channel.solo)
        };
        
        // Calculate position for right label
        let s_x = if is_cue {
            // "-> A" is 4 chars, shift left by 1 (one less padding on left)
            sep_x + 1 + (right_w.saturating_sub(4)) / 2
        } else {
            // "S" is 1 char, centered
            sep_x + 2 + (right_w - 2) / 2
        };
        
        let sep_style = Style::default().fg(SEPARATOR);
        // ┬ on the separator line above, connecting downward into the M|S split
        // │ on the button row between M and S/-> A
        // Note: bottom junction for CUE is handled by render_cue_pane separator
        if area.y > 0 {
            buf.set_string(sep_x, area.y - 1, "┬", sep_style);
        }
        buf.set_string(sep_x, area.y, "│", sep_style);
        
        // Highlight background for active toggles (with 1-cell padding on edges)
        let active_m_bg = if self.channel.muted { Some(STATUS_MUTED) } else { None };
        let active_s_bg = if right_active { Some(BORDER_ACTIVE) } else { None };

        // Fill M column background if active (skip first and last cell)
        if let Some(bg) = active_m_bg {
            for x in area.x + 1..sep_x.saturating_sub(1) {
                buf.set_string(x, area.y, " ", Style::default().bg(bg));
            }
        }
        // Fill S/-> A column background if active (skip first and last cell)
        if let Some(bg) = active_s_bg {
            for x in sep_x + 2..area.x + area.width - 1 {
                buf.set_string(x, area.y, " ", Style::default().bg(bg));
            }
        }
        
        // M - Mute
        let m_style = if self.channel.muted {
            Style::default().fg(Color::Black).bg(STATUS_MUTED)
        } else if self.is_control_selected(ChannelControl::Mute) {
            Style::default().fg(TEXT_EDITING)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        buf.set_string(m_x, area.y, "M", m_style);

        // S - Solo (or -> A for CUE)
        let s_style = if right_active {
            Style::default().fg(Color::Black).bg(BORDER_ACTIVE)
        } else if self.is_control_selected(right_control) {
            Style::default().fg(TEXT_EDITING)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        buf.set_string(s_x, area.y, right_label, s_style);
    }
}

/// Master channel strip widget - minimalist design
pub struct MasterStrip<'a> {
    master: &'a crate::state::MasterChannel,
    pane_selected: bool,
    selected: bool,
    selected_control: Option<GlobalControl>,
    editing: bool,
    frame: u8,
    /// True if any deck or CUE channel has playing == true
    any_channel_playing: bool,
}

impl<'a> MasterStrip<'a> {
    pub fn new(master: &'a crate::state::MasterChannel) -> Self {
        Self {
            master,
            pane_selected: false,
            selected: false,
            selected_control: None,
            editing: false,
            frame: 0,
            any_channel_playing: false,
        }
    }

    pub fn pane_selected(mut self, selected: bool) -> Self {
        self.pane_selected = selected;
        self
    }

    pub fn selected(mut self, selected: bool, control: Option<GlobalControl>) -> Self {
        self.selected = selected;
        if selected {
            self.selected_control = control;
        }
        self
    }

    pub fn editing(mut self, editing: bool) -> Self {
        self.editing = editing;
        self
    }

    pub fn frame(mut self, frame: u8) -> Self {
        self.frame = frame;
        self
    }

    pub fn any_channel_playing(mut self, v: bool) -> Self {
        self.any_channel_playing = v;
        self
    }
}

impl<'a> Widget for MasterStrip<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 12 {
            return;
        }

        // Border: yellow if pane selected, dim otherwise
        let border_style = if self.pane_selected {
            if self.selected { 
                Style::default().fg(BORDER_ACTIVE) 
            } else { 
                Style::default().fg(BORDER_NAVIGATED) 
            }
        } else {
            Style::default().fg(BTN_DM_PURPLE)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                " M ",
                border_style.add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Play/Pause spinner (3×3 ring)
                Constraint::Length(1), // Separator
                Constraint::Min(6),    // Meters and fader
                Constraint::Length(1), // Separator
                Constraint::Length(1), // M | OUT
            ])
            .split(inner);

        // ── Play/Pause spinner (full-width ring) ──
        let pp_area = chunks[0];
        let pp_selected = self.selected_control == Some(GlobalControl::MasterPlayPause);
        let playing = self.master.playing && self.any_channel_playing;

        // Background highlight when paused
        if !playing {
            for y in pp_area.y..pp_area.y + pp_area.height {
                for x in pp_area.x..pp_area.x + pp_area.width {
                    buf.set_string(x, y, " ", Style::default().bg(Color::Rgb(20, 20, 20)));
                }
            }
        }

        let cx = pp_area.x + pp_area.width / 2;
        let cy = pp_area.y + pp_area.height / 2;
        let hw = pp_area.width as i16 / 2 - 1; // 1 col padding each side

        // Build full-width ring segments clockwise: top → right → bottom → left
        // Total segments: 2*(2*hw+1) + 2 = 4*hw + 4
        let num_segments = (4 * hw + 4) as usize;
        let highlight_pos = if playing {
            (self.frame as usize) % num_segments
        } else {
            num_segments
        };

        let dim = Style::default().fg(Color::Rgb(35, 35, 35));
        let glow = Style::default().fg(STATUS_PLAYING).add_modifier(Modifier::BOLD);
        let trail = Style::default().fg(STATUS_PLAYING);
        let sel_glow = Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD);
        let sel_ring = Style::default().fg(BORDER_ACTIVE);

        // Generate ring positions: top-left corner, top edge, top-right corner,
        // right edge, bottom-right corner, bottom edge, bottom-left corner, left edge
        let mut ring: Vec<(i16, i16, &str)> = Vec::with_capacity(num_segments);
        // Top-left corner
        ring.push((-hw, -1, "╭"));
        // Top horizontal (left to right)
        for x in (-hw + 1)..hw {
            ring.push((x, -1, "─"));
        }
        // Top-right corner
        ring.push((hw, -1, "╮"));
        // Right edge
        ring.push((hw, 0, "│"));
        // Bottom-right corner
        ring.push((hw, 1, "╯"));
        // Bottom horizontal (right to left)
        for x in (1 - hw..hw).rev() {
            ring.push((x, 1, "─"));
        }
        // Bottom-left corner
        ring.push((-hw, 1, "╰"));
        // Left edge
        ring.push((-hw, 0, "│"));

        for (i, (dx, dy, ch)) in ring.iter().enumerate() {
            let x = (cx as i16 + dx) as u16;
            let y = (cy as i16 + dy) as u16;
            let prev = (i + num_segments - 1) % num_segments;
            let next = (i + 1) % num_segments;
            let style = if pp_selected {
                if playing && i == highlight_pos { sel_glow } else { sel_ring }
            } else if playing {
                if i == highlight_pos { glow }
                else if i == prev || i == next { trail }
                else { dim }
            } else {
                dim
            };
            buf.set_string(x, y, ch, style);
        }

        // Center: play / pause icon
        let center_char = if playing { "▶" } else { "⏸" };
        let center_style = if pp_selected {
            Style::default().fg(BORDER_ACTIVE).add_modifier(Modifier::BOLD)
        } else if playing {
            Style::default().fg(STATUS_PLAYING).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        buf.set_string(cx, cy, center_char, center_style);

        // ── Separator below play/pause ────────────────────────
        let sep1_y = chunks[1].y;
        for x in chunks[1].x..chunks[1].x + chunks[1].width {
            buf.set_string(x, sep1_y, "─", Style::default().fg(SEPARATOR));
        }

        // ── Stereo meter and fader ─────────────────────────────
        let meter_fader = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(chunks[2]);

        LevelMeter::new(0.0)
            .stereo(self.master.rms_left, self.master.rms_right)
            .peaks(self.master.peak_left, self.master.peak_right)
            .render(meter_fader[0], buf);

        let db = self.master.fader_db();
        let db_label = if db <= -60.0 { "∞".to_string() } else { format!("{:+.0}", db) };
        let fader_selected = self.selected_control == Some(GlobalControl::MasterFader);
        Fader::new(self.master.fader)
            .selected(fader_selected)
            .label(db_label)
            .render(meter_fader[1], buf);

        // ── Separator before buttons ──────────────────────────
        let sep_y = chunks[3].y;
        for x in chunks[3].x..chunks[3].x + chunks[3].width {
            buf.set_string(x, sep_y, "─", Style::default().fg(SEPARATOR));
        }

        // ── M | OUT button row ────────────────────────────────
        let btn_area = chunks[4];
        if btn_area.width >= 3 {
            let sep_x = btn_area.x + btn_area.width / 2;
            let left_w = sep_x - btn_area.x;
            let right_w = btn_area.x + btn_area.width - sep_x - 1;

            let m_x = btn_area.x + 1 + (left_w.saturating_sub(2)) / 2;
            let out_x = sep_x + 1 + (right_w.saturating_sub(3)) / 2;

            let sep_style = Style::default().fg(SEPARATOR);
            // Connect separator to borders: ┬ above, │ in row (no ┴ below - shares pane border)
            if btn_area.y > 0 {
                buf.set_string(sep_x, btn_area.y - 1, "┬", sep_style);
            }
            buf.set_string(sep_x, btn_area.y, "│", sep_style);

            // Fill M column background if muted
            if self.master.muted {
                for x in btn_area.x + 1..sep_x.saturating_sub(1) {
                    buf.set_string(x, btn_area.y, " ", Style::default().bg(STATUS_MUTED));
                }
            }

            // M - Mute
            let m_selected = self.selected_control == Some(GlobalControl::MasterMute);
            let m_style = if self.master.muted {
                Style::default().fg(Color::Black).bg(STATUS_MUTED)
            } else if m_selected {
                Style::default().fg(TEXT_EDITING)
            } else {
                Style::default().fg(TEXT_DIM)
            };
            buf.set_string(m_x, btn_area.y, "M", m_style);

            // OUT - Output Select
            let out_selected = self.selected_control == Some(GlobalControl::MasterOutputSelect);
            let out_style = if out_selected {
                Style::default().fg(TEXT_EDITING)
            } else {
                Style::default().fg(TEXT_DIM)
            };
            buf.set_string(out_x, btn_area.y, "OUT", out_style);
        }
    }
}
