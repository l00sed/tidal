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
                Constraint::Length(6),  // Deck indicator (ring + padding + marquee)
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // BPM display
                Constraint::Length(1),  // Separator
                Constraint::Length(10),  // EQ section (vertical bars + kill buttons + padding)
                Constraint::Length(1),  // Separator
                Constraint::Length(1),  // Pan
                Constraint::Length(1),  // Separator
                Constraint::Min(5),     // Fader + Meter
                Constraint::Length(1),  // Separator before buttons
                Constraint::Length(1),  // Buttons row
            ]
        } else {
            vec![
                Constraint::Length(10),  // EQ section (vertical bars + kill buttons + padding)
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

        // EQ section with vertical bars
        let eq_sep_x = self.render_eq_section(chunks[idx], buf);
        idx += 1;

        // Separator below EQ
        self.draw_separator(chunks[idx], buf);
        // Bottom junction (┴) where vertical separator meets the horizontal line
        if let Some(sep_x) = eq_sep_x {
            let sep_style = Style::default().fg(SEPARATOR);
            buf.set_string(sep_x, chunks[idx].y, "┴", sep_style);
        }
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

    /// Render pan bar: [L][bar][R]
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
        
        // Left label
        buf.set_string(area.x, y, "L", label_style);

        // Pan bar (bipolar: -1 to +1)
        let bar_start = area.x + 2;
        let bar_width = area.width.saturating_sub(4) as usize;
        if bar_width >= 3 {
            let center = bar_width / 2;
            let normalized = (value + 1.0) / 2.0; // -1..+1 -> 0..1
            let fill_pos = (normalized * bar_width as f32) as usize;
            
            for i in 0..bar_width {
                let ch = if i == center { '│' } else { '─' };
                
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
                
                buf.set_string(bar_start + i as u16, y, ch.to_string(), Style::default().fg(color));
            }
        }

        // Right label
        buf.set_string(area.x + area.width - 1, y, "R", label_style);
    }
    
    /// Render the EQ section with vertical bars (H/M/L) and filters (HPF/LPF)
    /// Returns the x-position of the vertical separator for junction drawing
    fn render_eq_section(&self, area: Rect, buf: &mut Buffer) -> Option<u16> {
        if area.width < 10 || area.height < 8 {
            return None;
        }

        // Layout: [H bar + kill] [M bar + kill] [L bar + kill] [separator] [HPF bar] [LPF bar]
        // Each EQ bar gets ~20% width, separator 1 char, filters share remaining space
        let eq_width = area.width / 5;
        let filter_width = area.width - eq_width * 3 - 1;
        
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(eq_width),         // Low
                Constraint::Length(eq_width),         // Mid
                Constraint::Length(eq_width),         // High
                Constraint::Length(1),                // Separator
                Constraint::Length(filter_width),     // Filters (HPF/LPF stacked)
            ])
            .split(area);

        // Render three EQ bands with vertical bars (L/M/H from left to right)
        self.render_vertical_eq_bar(sections[0], buf, "L",
            self.channel.eq_low, self.channel.eq_low_kill,
            self.is_control_selected(ChannelControl::EqLow),
            self.is_control_selected(ChannelControl::EqLowKill));

        self.render_vertical_eq_bar(sections[1], buf, "M",
            self.channel.eq_mid, self.channel.eq_mid_kill,
            self.is_control_selected(ChannelControl::EqMid),
            self.is_control_selected(ChannelControl::EqMidKill));

        self.render_vertical_eq_bar(sections[2], buf, "H",
            self.channel.eq_high, self.channel.eq_high_kill,
            self.is_control_selected(ChannelControl::EqHigh),
            self.is_control_selected(ChannelControl::EqHighKill));

        // Vertical separator with junctions
        let sep_style = Style::default().fg(SEPARATOR);
        let sep_x = sections[3].x;
        for y in sections[3].y..sections[3].y + sections[3].height {
            buf.set_string(sep_x, y, "│", sep_style);
        }
        // Top junction (┬) connects to separator above (deck channels only)
        if self.deck_label.is_some() && area.y > 0 {
            buf.set_string(sep_x, area.y - 1, "┬", sep_style);
        }

        // Render filter controls stacked vertically (Cutoff on top, Freq on bottom)
        let filter_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),  // Filter Cutoff
                Constraint::Percentage(50),  // Filter Freq
            ])
            .split(sections[4]);

        self.render_filter_bar(filter_chunks[0], buf,
            self.channel.filter_cutoff,
            self.is_control_selected(ChannelControl::FilterCutoff));

        self.render_filter_bar(filter_chunks[1], buf,
            self.channel.filter_freq,
            self.is_control_selected(ChannelControl::FilterFreq));

        Some(sep_x)
    }

    /// Render a single vertical EQ bar with kill button beneath
    #[allow(clippy::too_many_arguments)]
    fn render_vertical_eq_bar(&self, area: Rect, buf: &mut Buffer,
                               label: &str, eq_value: f32, killed: bool,
                               bar_selected: bool, kill_selected: bool) {
        if area.width < 2 || area.height < 4 {
            return;
        }

        let bar_editing = bar_selected && self.editing;
        let kill_editing = kill_selected && self.editing;

        // Top label (slightly left of center for visual balance)
        let label_style = if bar_editing || kill_editing {
            Style::default().fg(TEXT_EDITING).add_modifier(Modifier::BOLD)
        } else if bar_selected || kill_selected {
            Style::default().fg(TEXT_EDITING)
        } else {
            Style::default().fg(TEXT_GHOST)
        };
        // Center based on label length, then shift slightly left
        let label_x = area.x + (area.width.saturating_sub(label.len() as u16)) / 2;
        buf.set_string(label_x, area.y, label, label_style);

        // Vertical bar area (leave room for kill button at bottom)
        let bar_height = area.height.saturating_sub(3); // 1 for label, 1 for kill button, 1 for padding
        
        if bar_height >= 3 {
            // Normalize EQ value: -24dB to +24dB maps to -1.0 to 1.0 (0 = center)
            let normalized = eq_value / 24.0; // -1.0 to 1.0
            let bg_color = Color::Rgb(30, 30, 30);
            
            // tui-slider vertical_blocks style
            let filled_symbol = "█";
            let empty_symbol = "│";
            let handle_symbol = "─";
            let filled_color = if normalized > 0.0 { Color::Green } else { Color::Red };
            let empty_color = Color::DarkGray;
            let handle_color = if bar_editing {
                TEXT_BRIGHT
            } else if bar_selected {
                BORDER_ACTIVE
            } else {
                Color::White
            };
            
            // Bar centered
            let bar_x = area.x + (area.width - 1) / 2;
            
            // Each cell has 2 vertical levels (upper half and lower half)
            let total_half_rows = bar_height * 2;
            let center_half_row = total_half_rows as f32 / 2.0;
            // Max offset so value stays in 0..total_half_rows-1 range
            let max_offset = (total_half_rows as f32 - 1.0) / 2.0;
            
            // Map normalized value (-1.0 to 1.0) to half-row position (0 to total-1)
            let value_half_row = center_half_row - normalized * max_offset;
            
            // Convert to steps
            let value_step = value_half_row as u16;
            let center_step = center_half_row as u16;
            
            for row in 0..bar_height {
                let y = area.y + 1 + row;
                let lower_step = row * 2;
                let upper_step = row * 2 + 1;
                
                // Check if each half is between center and value
                let lower_in_fill = if normalized > 0.0 {
                    lower_step >= value_step && lower_step < center_step
                } else if normalized < 0.0 {
                    lower_step > center_step && lower_step <= value_step
                } else {
                    false
                };
                
                let upper_in_fill = if normalized > 0.0 {
                    upper_step >= value_step && upper_step < center_step
                } else if normalized < 0.0 {
                    upper_step > center_step && upper_step <= value_step
                } else {
                    false
                };
                
                let is_indicator = upper_step == value_step || lower_step == value_step;
                
                // Draw using tui-slider vertical_blocks style
                let (ch, color, use_bg) = if is_indicator {
                    // Moving handle - transparent background
                    (handle_symbol, handle_color, false)
                } else if lower_step == center_step || upper_step == center_step {
                    // Center line - transparent background
                    let center_color = if bar_selected { BORDER_ACTIVE } else { TEXT_DIM };
                    (handle_symbol, center_color, false)
                } else if upper_in_fill && lower_in_fill {
                    // Both halves filled - keep bg for filled area
                    (filled_symbol, filled_color, true)
                } else if upper_in_fill {
                    ("▀", filled_color, true)
                } else if lower_in_fill {
                    ("▄", filled_color, true)
                } else {
                    // Empty - transparent background
                    (empty_symbol, empty_color, false)
                };
                
                if use_bg {
                    buf.set_string(bar_x, y, ch, Style::default().fg(color).bg(bg_color));
                } else {
                    buf.set_string(bar_x, y, ch, Style::default().fg(color));
                }
            }
        }

        // Kill button at bottom (centered)
        let kill_y = area.y + area.height - 1;
        let kill_x = area.x + (area.width - 1) / 2;
        let kill_char = if killed { "󱨦" } else { "󱨥" };
        let kill_style = if kill_selected {
            if killed {
                // Kill active + focused: yellow/gold with bold
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                // Kill inactive + focused: use TEXT_EDITING (yellow)
                Style::default().fg(TEXT_EDITING)
            }
        } else if killed {
            // Kill active + unfocused: use gray (same as filter knob unfocused)
            Style::default().fg(Color::Rgb(100, 100, 100))
        } else {
            // Kill inactive + unfocused: use TEXT_DIM
            Style::default().fg(TEXT_DIM)
        };
        buf.set_string(kill_x, kill_y, kill_char, kill_style);
    }

    /// Render a compact vertical filter bar (HPF/LPF)
    fn render_filter_bar(&self, area: Rect, buf: &mut Buffer,
                         value: f32, selected: bool) {
        if area.width < 1 || area.height < 3 {
            return;
        }

        let editing = selected && self.editing;

        // Knob icon progression (0.0 to 1.0)
        let knob_icons = ["󰄰", "󰪞", "󰪟", "󰪠", "󰪡", "󰪢", "󰪣", "󰪤", "󰪥"];
        let icon_index = (value * (knob_icons.len() - 1) as f32).round() as usize;
        let knob_char = knob_icons[icon_index];

        // Knob style
        let knob_style = if editing {
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(METER_FILL)
        } else {
            Style::default().fg(Color::Rgb(100, 100, 100))
        };

        // Draw the knob centered in the area
        let knob_y = area.y + area.height / 2;
        let knob_x = area.x + (area.width.saturating_sub(1)) / 2;
        buf.set_string(knob_x, knob_y, knob_char, knob_style);
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

    /// Render compact master EQ bars (10-band, no gaps) inside MasterStrip
    fn render_master_eq_bars(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 2 {
            return;
        }

        let bar_area_height = area.height;
        let num_bands = 10u16;

        // Bars with 1-char gap between columns
        let bar_step = 2u16;
        let total_width = num_bands * bar_step;
        let start_x = if area.width > total_width {
            (area.width - total_width + 1) / 2
        } else {
            0
        };

        let eq_selected = self.selected;

        for i in 0..10 {
            let x_offset = start_x + (i as u16) * bar_step;
            if x_offset >= area.width {
                break;
            }

            let band_selected = eq_selected
                && self.selected_control == Some(GlobalControl::all_eq_variants()[i]);

            let peak = self.master.spectrum_peaks[i];
            let db = self.master.master_eq[i];
            let gain_normalized = (db + 12.0) / 24.0;

            let total_steps = bar_area_height * 2;
            let level_steps = (peak * total_steps as f32) as u16;
            let gain_step = (gain_normalized * (total_steps - 1) as f32) as u16;

            let bar_x = area.x + x_offset;

            for row in 0..bar_area_height {
                let y = area.y + bar_area_height - 1 - row;
                let lower_step = row * 2;
                let upper_step = row * 2 + 1;

                let lower_filled = level_steps > lower_step;
                let upper_filled = level_steps > upper_step;

                let gain_in_lower = gain_step == lower_step;
                let gain_in_upper = gain_step == upper_step;

                let color_for_step = |step: u16| -> Color {
                    let pct = step as f32 / total_steps as f32;
                    if pct > 0.90 { Color::Red }
                    else if pct > 0.80 { Color::Rgb(255, 140, 0) }
                    else if pct > 0.65 { Color::Yellow }
                    else { Color::Green }
                };

                if gain_in_upper || gain_in_lower {
                    let gain_color = if band_selected { BORDER_ACTIVE } else { Color::White };
                    let gain_style = Style::default().fg(gain_color).add_modifier(Modifier::BOLD);
                    buf.set_string(bar_x, y, "─", gain_style);
                } else if upper_filled && lower_filled {
                    let c = color_for_step(upper_step);
                    buf.set_string(bar_x, y, "█", Style::default().fg(c));
                } else if upper_filled && !lower_filled {
                    let c = color_for_step(upper_step);
                    buf.set_string(bar_x, y, "▀", Style::default().fg(c));
                } else if !upper_filled && lower_filled {
                    let c = color_for_step(lower_step);
                    buf.set_string(bar_x, y, "▄", Style::default().fg(c));
                }
                // else: transparent (no background)
            }

            // Vertical separator in gap between bands (not after last)
            if i < 9 {
                let sep_x = bar_x + 1;
                for row in 0..bar_area_height {
                    let y = area.y + bar_area_height - 1 - row;
                    buf.set_string(sep_x, y, "│", Style::default().fg(SEPARATOR));
                }
            }
        }
    }
}

impl<'a> Widget for MasterStrip<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 18 {
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
                Constraint::Length(5), // EQ bars
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
        // Junctions: override separator at EQ gap columns
        let eq_area = chunks[2];
        let num_bands = 10u16;
        let bar_step = 2u16;
        let total_width = num_bands * bar_step;
        let start_x = if eq_area.width > total_width {
            (eq_area.width - total_width + 1) / 2
        } else {
            0
        };
        for i in 0..9 {
            let sep_x = eq_area.x + start_x + (i as u16) * bar_step + 1;
            if sep_x < eq_area.x + eq_area.width {
                buf.set_string(sep_x, sep1_y, "┬", Style::default().fg(SEPARATOR));
            }
        }

        // ── Master EQ bars (compact, no gaps) ──────────────────
        self.render_master_eq_bars(chunks[2], buf);

        // ── Separator below EQ ────────────────────────────────
        let sep_eq_y = chunks[3].y;
        for x in chunks[3].x..chunks[3].x + chunks[3].width {
            buf.set_string(x, sep_eq_y, "─", Style::default().fg(SEPARATOR));
        }
        // Junctions: override separator at EQ gap columns
        for i in 0..9 {
            let sep_x = eq_area.x + start_x + (i as u16) * bar_step + 1;
            if sep_x < eq_area.x + eq_area.width {
                buf.set_string(sep_x, sep_eq_y, "┴", Style::default().fg(SEPARATOR));
            }
        }

        // ── Stereo meter and fader ─────────────────────────────
        let meter_fader = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(chunks[4]);

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
        let sep_y = chunks[5].y;
        for x in chunks[5].x..chunks[5].x + chunks[5].width {
            buf.set_string(x, sep_y, "─", Style::default().fg(SEPARATOR));
        }

        // ── M | OUT button row ────────────────────────────────
        let btn_area = chunks[6];
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
