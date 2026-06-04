//! Channel strip component - represents a single mixer channel

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Widget},
};

use crate::state::{ChannelControl, MixerChannel};
use crate::ui::widgets::{Button, DeckIndicator, Fader, LevelMeter};

/// A complete channel strip widget - minimalist futuristic design
pub struct ChannelStrip<'a> {
    channel: &'a MixerChannel,
    selected: bool,
    selected_control: Option<ChannelControl>,
    deck_label: Option<&'a str>,
    deck_color: Color,
    editing: bool,
    frame: u8,
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
        }
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
            format!("{:.1}k", freq / 1000.0)
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

    fn format_pan(pan: f32) -> String {
        if pan < -0.05 {
            format!("L{:.0}", (-pan * 100.0))
        } else if pan > 0.05 {
            format!("R{:.0}", (pan * 100.0))
        } else {
            "C".to_string()
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

        // Minimalist border - thin line style
        // Connected decks get brighter colors
        let border_color = if self.selected {
            Color::Yellow
        } else if self.deck_label.is_some() {
            if self.channel.connected {
                // Brighter version of deck color when connected
                match self.deck_color {
                    Color::Cyan => Color::Rgb(100, 255, 255),
                    Color::Magenta => Color::Rgb(255, 100, 255),
                    _ => self.deck_color,
                }
            } else {
                // Dimmer when not connected
                Color::Rgb(60, 60, 60)
            }
        } else if self.channel.connected {
            Color::Rgb(0, 180, 0)
        } else {
            Color::Rgb(40, 40, 40)
        };

        let border_style = Style::default().fg(border_color);

        // Clean title - show connection status
        let title = if let Some(deck) = self.deck_label {
            if self.channel.connected {
                if deck.contains('A') { " A ● " } else { " B ● " }
            } else {
                if deck.contains('A') { " A ○ " } else { " B ○ " }
            }
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

        let inner = block.inner(area);
        block.render(area, buf);

        // Minimalist layout
        let has_deck = self.deck_label.is_some();
        
        let constraints = if has_deck {
            vec![
                Constraint::Length(5),  // Deck indicator (with gap for source name)
                Constraint::Length(1),  // BPM slider
                Constraint::Length(2),  // EQ High
                Constraint::Length(2),  // EQ Mid
                Constraint::Length(2),  // EQ Low
                Constraint::Length(2),  // HPF
                Constraint::Length(2),  // LPF
                Constraint::Length(2),  // Pan
                Constraint::Min(3),     // Fader + Meter
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
                Constraint::Min(6),     // Fader + Meter
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
            let deck_char = if self.deck_label.unwrap_or("").contains('A') { 'A' } else { 'B' };
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
                .render(chunks[idx], buf);
            idx += 1;
            
            // BPM/Speed control (only for deck channels)
            self.render_bpm_bar(chunks[idx], buf, self.is_control_selected(ChannelControl::Bpm));
            idx += 1;
        }

        // EQ bars with kill switches - compact single-line style
        self.render_eq_bar_with_kill(chunks[idx], buf, "H", 
            self.channel.eq_high, self.channel.eq_high_kill,
            self.is_control_selected(ChannelControl::EqHigh),
            self.is_control_selected(ChannelControl::EqHighKill));
        idx += 1;

        self.render_eq_bar_with_kill(chunks[idx], buf, "M",
            self.channel.eq_mid, self.channel.eq_mid_kill,
            self.is_control_selected(ChannelControl::EqMid),
            self.is_control_selected(ChannelControl::EqMidKill));
        idx += 1;

        self.render_eq_bar_with_kill(chunks[idx], buf, "L",
            self.channel.eq_low, self.channel.eq_low_kill,
            self.is_control_selected(ChannelControl::EqLow),
            self.is_control_selected(ChannelControl::EqLowKill));
        idx += 1;

        // Filter bars
        self.render_compact_bar(chunks[idx], buf, "↑",
            Self::freq_to_normalized(self.channel.hpf_freq),
            Self::format_freq(self.channel.hpf_freq),
            false, self.is_control_selected(ChannelControl::HighPassFilter));
        idx += 1;

        self.render_compact_bar(chunks[idx], buf, "↓",
            Self::freq_to_normalized(self.channel.lpf_freq),
            Self::format_freq(self.channel.lpf_freq),
            false, self.is_control_selected(ChannelControl::LowPassFilter));
        idx += 1;

        // Pan
        self.render_compact_bar(chunks[idx], buf, "◇",
            (self.channel.pan + 1.0) / 2.0,
            Self::format_pan(self.channel.pan),
            true, self.is_control_selected(ChannelControl::Pan));
        idx += 1;

        // Fader and meter area
        let fader_area = chunks[idx];
        idx += 1;
        
        if fader_area.width >= 4 {
            let fader_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(2), Constraint::Min(2)])
                .split(fader_area);

            LevelMeter::new(self.channel.rms_level)
                .peak(self.channel.peak_level)
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

        // Compact button row: M S C
        let btn_area = chunks[idx];
        self.render_button_row(btn_area, buf);
    }
}

impl<'a> ChannelStrip<'a> {
    /// Render a compact single-line bar: [label][bar][value]
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };
        buf.set_string(area.x, y, label, label_style);

        // Fixed display width to keep bar centered regardless of value
        let display_width = 4u16;
        let bar_start = area.x + 2;
        let bar_width = area.width.saturating_sub(3 + display_width);
        
        if bar_width >= 3 {
            self.draw_mini_bar(buf, bar_start, y, bar_width as usize, value, bipolar, selected, editing);
        }

        // Value - right-aligned in fixed width, white when editing
        let val_x = area.x + area.width - display_width;
        let padded_display = format!("{:>4}", display);
        let val_style = if editing {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Rgb(100, 100, 100))
        };
        buf.set_string(val_x, y, &padded_display, val_style);
    }

    fn draw_mini_bar(&self, buf: &mut Buffer, x: u16, y: u16, width: usize, value: f32, bipolar: bool, selected: bool, editing: bool) {
        let dim = Color::Rgb(30, 30, 30);
        
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
                            Color::White // Brighter when editing
                        } else if fill_pos > center { 
                            Color::Green 
                        } else { 
                            Color::Red 
                        }
                    } else {
                        if editing { Color::Rgb(60, 60, 60) } else { dim }
                    }
                } else {
                    dim
                };
                
                buf.set_string(x + i as u16, y, &ch.to_string(), Style::default().fg(color));
            }
        } else {
            let filled = (value * width as f32) as usize;
            for i in 0..width {
                let color = if i < filled {
                    if editing { 
                        Color::White 
                    } else if selected { 
                        Color::Cyan 
                    } else { 
                        Color::Rgb(60, 60, 60) 
                    }
                } else {
                    if editing { Color::Rgb(50, 50, 50) } else { dim }
                };
                buf.set_string(x + i as u16, y, "─", Style::default().fg(color));
            }
        }
    }
    
    /// Render EQ bar with kill switch: [label][bar][×][value]
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if bar_selected || kill_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };
        buf.set_string(area.x, y, label, label_style);

        // Kill switch (×) - positioned before the value
        let display_width = 4u16;
        let kill_x = area.x + area.width - display_width - 2;
        let kill_char = if killed { "×" } else { "○" };
        let kill_style = if kill_selected {
            if killed {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            }
        } else if killed {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
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
            Style::default().fg(Color::Rgb(60, 60, 60))
        } else if bar_editing {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if bar_selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Rgb(100, 100, 100))
        };
        buf.set_string(val_x, y, &padded_display, val_style);
    }
    
    /// Render BPM/speed control bar
    fn render_bpm_bar(&self, area: Rect, buf: &mut Buffer, selected: bool) {
        if area.width < 4 || area.height < 1 {
            return;
        }
        
        let speed = self.channel.playback_speed;
        // Speed range: 0.5 to 2.0, center at 1.0
        // Normalized: 0.0 to 1.0, center at 0.33 (for 0.5-2.0 range)
        let normalized = ((speed - 0.5) / 1.5).clamp(0.0, 1.0);
        let center_normalized = (1.0 - 0.5) / 1.5; // Where 1.0x sits
        
        let editing = self.is_control_editing(ChannelControl::Bpm);
        
        // Format speed as percentage or multiplier
        let label = format!("{:.0}%", speed * 100.0);
        
        let label_style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };
        
        // Draw label on left
        let label_width = label.len().min(4);
        buf.set_string(area.x, area.y, &label[..label_width], label_style);
        
        // Draw bar on right
        let bar_x = area.x + label_width as u16 + 1;
        let bar_width = area.width.saturating_sub(label_width as u16 + 1) as usize;
        
        if bar_width >= 3 {
            let dim = Color::Rgb(30, 30, 30);
            let center_pos = (center_normalized * bar_width as f32) as usize;
            let fill_pos = (normalized * bar_width as f32) as usize;
            
            for i in 0..bar_width {
                let ch = if i == center_pos { '│' } else { '─' };
                
                let color = if editing || selected {
                    // Highlight filled portion
                    if (fill_pos > center_pos && i > center_pos && i <= fill_pos) ||
                       (fill_pos < center_pos && i < center_pos && i >= fill_pos) ||
                       i == fill_pos {
                        if editing {
                            Color::White
                        } else if fill_pos > center_pos {
                            Color::Cyan  // Faster
                        } else {
                            Color::Magenta  // Slower
                        }
                    } else if i == center_pos {
                        if editing { Color::White } else { Color::Rgb(100, 100, 100) }
                    } else {
                        if editing { Color::Rgb(60, 60, 60) } else { dim }
                    }
                } else {
                    dim
                };
                
                buf.set_string(bar_x + i as u16, area.y, &ch.to_string(), Style::default().fg(color));
            }
        }
    }

    fn render_button_row(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 5 {
            return;
        }

        let btn_width = (area.width / 3).max(1);
        
        // M - Mute
        let m_style = if self.channel.muted {
            Style::default().fg(Color::Black).bg(Color::Red)
        } else if self.is_control_selected(ChannelControl::Mute) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
        };
        buf.set_string(area.x, area.y, "M", m_style);

        // S - Solo
        let s_style = if self.channel.solo {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else if self.is_control_selected(ChannelControl::Solo) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
        };
        buf.set_string(area.x + btn_width, area.y, "S", s_style);

        // C - Cue
        let c_style = if self.channel.pfl {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else if self.is_control_selected(ChannelControl::Pfl) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
        };
        buf.set_string(area.x + btn_width * 2, area.y, "C", c_style);
    }
}

/// Master channel strip widget - minimalist design
pub struct MasterStrip<'a> {
    master: &'a crate::state::MasterChannel,
    pane_selected: bool,
    selected: bool,
}

impl<'a> MasterStrip<'a> {
    pub fn new(master: &'a crate::state::MasterChannel) -> Self {
        Self {
            master,
            pane_selected: false,
            selected: false,
        }
    }

    pub fn pane_selected(mut self, selected: bool) -> Self {
        self.pane_selected = selected;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(80, 0, 80))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                " M ",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // DIM/MONO
                Constraint::Min(6),    // Meters and fader
                Constraint::Length(1), // Mute
            ])
            .split(inner);

        // DIM and MONO inline
        let dm_style = Style::default().fg(Color::Rgb(60, 60, 60));
        let dim_style = if self.master.dim {
            Style::default().fg(Color::Black).bg(Color::Blue)
        } else { dm_style };
        let mono_style = if self.master.mono {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else { dm_style };
        
        buf.set_string(chunks[0].x, chunks[0].y, "D", dim_style);
        buf.set_string(chunks[0].x + 2, chunks[0].y, "M", mono_style);

        // Stereo meter and fader
        let meter_fader = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(chunks[1]);

        LevelMeter::new(0.0)
            .stereo(self.master.peak_left, self.master.peak_right)
            .peaks(self.master.peak_left, self.master.peak_right)
            .render(meter_fader[0], buf);

        let db = self.master.fader_db();
        let db_label = if db <= -60.0 { "∞".to_string() } else { format!("{:+.0}", db) };
        Fader::new(self.master.fader)
            .selected(self.selected)
            .label(db_label)
            .render(meter_fader[1], buf);

        // Mute
        let mute_style = if self.master.muted {
            Style::default().fg(Color::Black).bg(Color::Red)
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
        };
        buf.set_string(chunks[2].x, chunks[2].y, "×", mute_style);
    }
}
