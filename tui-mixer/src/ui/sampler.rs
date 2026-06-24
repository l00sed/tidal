//! Sample pad grid widget for TUI

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};

use crate::state::{PadControl, Rack, SamplePadGrid, SamplePad};
use crate::ui::colors::*;

/// Configuration pane for a single pad — replaces the pad grid when [c] is pressed
pub struct PadConfigPane<'a> {
    grid: &'a SamplePadGrid,
    editing: bool,
}

impl<'a> PadConfigPane<'a> {
    pub fn new(grid: &'a SamplePadGrid) -> Self {
        Self { grid, editing: false }
    }

    pub fn editing(mut self, editing: bool) -> Self {
        self.editing = editing;
        self
    }
}

impl<'a> Widget for PadConfigPane<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let pad_idx = self.grid.selected_pad;
        let pad = &self.grid.pads[pad_idx];
        let key_char = pad.key_char().to_ascii_uppercase();
        let selected_ctrl = self.grid.selected_control;
        let is_editing = self.editing;

        // Title with pad name
        let title = format!(" Pad Config: [{}] {} ", key_char, pad.name);
        let title_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(ratatui::text::Span::styled(title, title_style));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width < 20 || inner.height < 4 {
            buf.set_string(inner.x, inner.y, "Too small", Style::default().fg(Color::Red));
            return;
        }

        let controls = PadControl::all();
        let row_height = 1u16;
        let mut y = inner.y;

        for &control in controls {
            if y + row_height > inner.y + inner.height {
                break;
            }

            let is_selected = control == selected_ctrl;
            let row_area = Rect::new(inner.x, y, inner.width, row_height);

            render_config_row(row_area, buf, control, pad, is_selected, is_editing);

            y += row_height;
        }

        // Help hint at bottom
        if area.height > inner.height + 2 {
            let hint = if is_editing {
                "hjkl:adjust │ 0:reset │ Esc:back"
            } else {
                "j/k:move │ Enter:edit/open │ SPACE:play │ c:close"
            };
            let hint_y = area.y + area.height - 1;
            buf.set_string(
                area.x + 1,
                hint_y,
                hint,
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

/// Render a single config control row
fn render_config_row(
    area: Rect,
    buf: &mut Buffer,
    control: PadControl,
    pad: &SamplePad,
    selected: bool,
    editing: bool,
) {
    if area.width < 10 {
        return;
    }

    let label_width = 10usize.min(area.width as usize / 3);
    let value_area_width = area.width as usize - label_width - 1;
    let bar_width = (value_area_width as f32 * 0.6) as usize;
    let value_width = value_area_width - bar_width - 1;

    // Label
    let label_style = if selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let label = control.label();
    buf.set_string(area.x, area.y, label, label_style);

    let control_x = area.x + label_width as u16;

    match control {
        PadControl::Sample => {
            let sample_name = pad.sample_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("(none)");
            let name = if sample_name.len() > bar_width + value_width {
                format!("{}…", &sample_name[..bar_width + value_width - 1])
            } else {
                sample_name.to_string()
            };
            let style = if selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            buf.set_string(control_x, area.y, &name, style);
        }
        PadControl::PlayMode => {
            let mode_label = pad.play_mode.label();
            let style = if selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            buf.set_string(control_x, area.y, mode_label, style);
        }
        PadControl::Mute => {
            let (label, color) = if pad.config.mute {
                ("ON", Color::Red)
            } else {
                ("OFF", Color::DarkGray)
            };
            let style = if selected {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            buf.set_string(control_x, area.y, label, style);
        }
        PadControl::FiltersHeader => {
            // Just a heading, not interactive
            let style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
            buf.set_string(control_x, area.y, "Filters", style);
        }
        _ => {
            // Continuous controls: draw a bar + value
            let (value, display) = match control {
                PadControl::Volume => (pad.config.volume, format!("{:.2}", pad.config.volume)),
                PadControl::HighPass => {
                    let norm = (pad.config.high_pass - 20.0) / 19980.0;
                    (norm, format!("{} Hz", format_hz(pad.config.high_pass)))
                }
                PadControl::LowPass => {
                    let norm = (pad.config.low_pass - 20.0) / 19980.0;
                    (norm, format!("{} Hz", format_hz(pad.config.low_pass)))
                }
                PadControl::EqLow => (pad.config.eq_low / 2.0, format!("{:.1}", pad.config.eq_low)),
                PadControl::EqMid => (pad.config.eq_mid / 2.0, format!("{:.1}", pad.config.eq_mid)),
                PadControl::EqHigh => (pad.config.eq_high / 2.0, format!("{:.1}", pad.config.eq_high)),
                PadControl::Reverb => (pad.config.reverb, format!("{:.2}", pad.config.reverb)),
                PadControl::Chorus => (pad.config.chorus, format!("{:.2}", pad.config.chorus)),
                PadControl::Distortion => (pad.config.distortion, format!("{:.2}", pad.config.distortion)),
                _ => return,
            };

            // Bar
            let bar_x = control_x;
            let bar_chars = bar_width.max(2);
            let fill = (value * (bar_chars - 2) as f32) as usize;

            let bar_style = if selected && editing {
                Style::default().fg(Color::Yellow)
            } else if selected {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            buf.set_string(bar_x, area.y, "├", bar_style);
            for i in 0..bar_chars - 2 {
                let ch = if i < fill { "━" } else { "─" };
                buf.set_string(bar_x + 1 + i as u16, area.y, ch, bar_style);
            }
            buf.set_string(bar_x + bar_chars as u16 - 1, area.y, "┤", bar_style);

            // Value text
            let val_x = bar_x + bar_chars as u16 + 1;
            let val_style = if selected && editing {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };
            if val_x + display.len() as u16 <= area.x + area.width {
                buf.set_string(val_x, area.y, &display, val_style);
            }
        }
    }
}

/// Format frequency in Hz to a human-readable string
fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.1}k", hz / 1000.0)
    } else {
        format!("{:.0}", hz)
    }
}

/// A single rack row: name, volume slider, mute, record, blinking indicator
pub struct RackRow<'a> {
    rack: &'a Rack,
    selected: bool,
    frame: u8,
    recording: bool,
    count_in: Option<(u8, u8)>,
}

impl<'a> RackRow<'a> {
    pub fn new(rack: &'a Rack) -> Self {
        Self {
            rack,
            selected: false,
            frame: 0,
            recording: false,
            count_in: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn frame(mut self, frame: u8) -> Self {
        self.frame = frame;
        self
    }

    pub fn recording(mut self, recording: bool) -> Self {
        self.recording = recording;
        self
    }

    pub fn count_in_opt(mut self, count_in: Option<(u8, u8)>) -> Self {
        self.count_in = count_in;
        self
    }
}

impl<'a> Widget for RackRow<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 || area.width < 10 {
            return;
        }

        let y = area.y;

        // Border style
        let border_style = if self.selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };

        // Rack name/number (left-aligned)
        let name = format!(" {} ", self.rack.name);
        let name_style = if self.selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        buf.set_string(area.x, y, &name, name_style);

        // Right-aligned controls (from right edge, leaving room for right border)
        let right_edge = area.x + area.width - 1; // -1 for right border

        // Tempo display (rightmost, 4 chars)
        let tempo_str = format!("{:.0}", self.rack.tempo);
        let tempo_style = if self.selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let tempo_w = tempo_str.len() as u16;
        let tempo_x = right_edge.saturating_sub(tempo_w);
        buf.set_string(tempo_x, y, &tempo_str, tempo_style);

        // Indicator (1 char + 1 space gap before tempo)
        let indicator_x = tempo_x.saturating_sub(2);
        if self.recording {
            buf.set_string(indicator_x, y, "●", 
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
        } else if let Some((step, _total)) = self.count_in {
            let show = self.frame % 4 < 2;
            if show {
                let num = format!("{}", step + 1);
                buf.set_string(indicator_x, y, &num,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            }
        } else if self.rack.playing && !self.rack.mute {
            let on = self.frame % 6 < 4;
            if on {
                buf.set_string(indicator_x, y, "●",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            } else {
                buf.set_string(indicator_x, y, "●",
                    Style::default().fg(PAD_ACTIVE_HIGH));
            }
        } else if self.rack.playing && self.rack.mute {
            buf.set_string(indicator_x, y, "●",
                Style::default().fg(PAD_ACTIVE_LOW));
        } else {
            buf.set_string(indicator_x, y, "○",
                Style::default().fg(Color::DarkGray));
        }

        // Mute button (3 chars + 1 space gap before indicator)
        let mute_x = indicator_x.saturating_sub(4);
        let (label, color) = if self.rack.mute {
            ("[M]", Color::Red)
        } else {
            ("[M]", Color::DarkGray)
        };
        let mute_style = if self.selected {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        buf.set_string(mute_x, y, label, mute_style);

        // Volume slider (right-aligned before mute, 20 chars: ├ + 18 bars + ┤)
        let slider_w = 20u16;
        let slider_x = mute_x.saturating_sub(slider_w + 1);
        if slider_x > area.x + name.len() as u16 {
            let filled = (self.rack.volume * (slider_w as f32 - 2.0)) as usize;
            let bar_style = if self.selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            buf.set_string(slider_x, y, "├", bar_style);
            for i in 0..(slider_w as usize).saturating_sub(2) {
                let ch = if i < filled { "━" } else { "─" };
                buf.set_string(slider_x + 1 + i as u16, y, ch, bar_style);
            }
            buf.set_string(slider_x + slider_w - 1, y, "┤", bar_style);
        }

        // Right border
        if area.width > 1 {
            buf.set_string(area.x + area.width - 1, y, "│", border_style);
        }
    }
}

/// Count-in overlay that shows 1, 2... slow, then 1, 2, 3, 4 fast, then steady
pub struct CountInOverlay {
    step: u8,
    frame: u8,
}

impl CountInOverlay {
    pub fn new(step: u8, frame: u8) -> Self {
        Self { step, frame }
    }
}

impl Widget for CountInOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 3 {
            return;
        }

        // Centered overlay box
        let box_w = 12u16.min(area.width.saturating_sub(2));
        let box_h = 3u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(box_w)) / 2;
        let y = area.y + (area.height.saturating_sub(box_h)) / 2;

        let box_area = Rect::new(x, y, box_w, box_h);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        let inner = block.inner(box_area);
        block.render(box_area, buf);

        // Blink: show number on even frames
        let show = self.frame % 4 < 2;
        if show && inner.height > 0 {
            // Display countdown in reverse: 3, 2, 1
            let num = format!("{}", 3 - self.step);
            let num_style = Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD);
            let nx = inner.x + (inner.width.saturating_sub(num.len() as u16)) / 2;
            buf.set_string(nx, inner.y, &num, num_style);
        }
    }
}
