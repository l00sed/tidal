//! Custom widget implementations for the mixer UI

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

/// Vertical fader widget that mimics a physical mixer fader
pub struct Fader {
    /// Current value (0.0 to 1.0)
    value: f32,
    /// Is this fader selected
    selected: bool,
    /// Label text
    label: String,
}

impl Fader {
    pub fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            selected: false,
            label: String::new(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl Widget for Fader {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 5 {
            return;
        }

        let track_x = area.x + area.width / 2;
        let track_top = area.y + 1;
        let track_bottom = area.y + area.height - 2;
        let track_height = track_bottom - track_top;

        // Draw the fader track
        let track_style = Style::default().fg(Color::DarkGray);
        for y in track_top..=track_bottom {
            buf.set_string(track_x, y, "│", track_style);
        }

        // Draw tick marks
        let tick_positions = [0.0, 0.25, 0.5, 0.75, 1.0];
        for &pos in &tick_positions {
            let y = track_bottom - (pos * track_height as f32) as u16;
            if y >= track_top && y <= track_bottom {
                if track_x > area.x {
                    buf.set_string(track_x - 1, y, "─", track_style);
                }
                if track_x + 1 < area.x + area.width {
                    buf.set_string(track_x + 1, y, "─", track_style);
                }
            }
        }

        // Draw the fader cap (the part you grab)
        let fader_y = track_bottom - (self.value * track_height as f32) as u16;
        let fader_y = fader_y.clamp(track_top, track_bottom);

        let fader_style = if self.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        // Draw fader cap as a wider element
        let cap_left = track_x.saturating_sub(1);
        let cap_right = (track_x + 1).min(area.x + area.width - 1);

        buf.set_string(cap_left, fader_y, "▐", fader_style);
        buf.set_string(track_x, fader_y, "█", fader_style);
        buf.set_string(cap_right, fader_y, "▌", fader_style);

        // Draw dB label at bottom
        if !self.label.is_empty() && area.height > 3 {
            let label_x = area.x + (area.width.saturating_sub(self.label.len() as u16)) / 2;
            buf.set_string(
                label_x,
                area.y + area.height - 1,
                &self.label,
                Style::default().fg(Color::Cyan),
            );
        }
    }
}

/// Horizontal bar indicator for EQ and filter controls
/// Shows a centered bar that fills green when increased, red when decreased
pub struct HorizontalBar {
    /// Current value (0.0 to 1.0 for unipolar, or normalized from bipolar)
    value: f32,
    /// Is this a bipolar control (centered at 0.5)
    bipolar: bool,
    /// Is this bar selected
    selected: bool,
    /// Label text
    label: String,
    /// Value display text
    value_display: String,
}

impl HorizontalBar {
    pub fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            bipolar: false,
            selected: false,
            label: String::new(),
            value_display: String::new(),
        }
    }

    pub fn bipolar(mut self, bipolar: bool) -> Self {
        self.bipolar = bipolar;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn value_display(mut self, display: impl Into<String>) -> Self {
        self.value_display = display.into();
        self
    }
}

impl Widget for HorizontalBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 3 {
            return;
        }

        // Row 0: Label
        // Row 1: Bar indicator
        // Row 2: Value display

        let bar_y = area.y + 1;
        let bar_width = area.width.saturating_sub(2) as usize;
        let bar_x = area.x + 1;

        // Draw label
        if !self.label.is_empty() {
            let label_style = if self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let label_x = area.x + (area.width.saturating_sub(self.label.len() as u16)) / 2;
            buf.set_string(label_x, area.y, &self.label, label_style);
        }

        // Draw bar background (gray unfilled)
        let bg_style = Style::default().fg(Color::Rgb(60, 60, 60));
        let bg_bar: String = "─".repeat(bar_width);
        buf.set_string(bar_x, bar_y, &bg_bar, bg_style);

        // Draw end caps
        let cap_style = if self.selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        buf.set_string(area.x, bar_y, "├", cap_style);
        buf.set_string(area.x + area.width - 1, bar_y, "┤", cap_style);

        let center_pos = bar_width / 2;

        if self.bipolar {
            // Bipolar: center is neutral (0.5 = center)
            // Below 0.5: red bar extends left from center
            // Above 0.5: green bar extends right from center
            
            // Draw center marker
            buf.set_string(bar_x + center_pos as u16, bar_y, "│", Style::default().fg(Color::White));

            if self.value < 0.5 {
                // Red fill from current position to center
                let fill_start = (self.value * bar_width as f32) as usize;
                let fill_end = center_pos;
                let red_style = Style::default().fg(Color::Red);
                for i in fill_start..fill_end {
                    buf.set_string(bar_x + i as u16, bar_y, "━", red_style);
                }
            } else if self.value > 0.5 {
                // Green fill from center to current position
                let fill_start = center_pos + 1;
                let fill_end = (self.value * bar_width as f32) as usize;
                let green_style = Style::default().fg(Color::Green);
                for i in fill_start..=fill_end.min(bar_width - 1) {
                    buf.set_string(bar_x + i as u16, bar_y, "━", green_style);
                }
            }

            // Draw position indicator
            let indicator_pos = (self.value * bar_width as f32) as u16;
            let indicator_style = if self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            buf.set_string(bar_x + indicator_pos, bar_y, "●", indicator_style);

        } else {
            // Unipolar: fills from left to right, green as it increases
            let fill_amount = (self.value * bar_width as f32) as usize;
            
            // Gradient from dim to bright green based on fill
            let green_style = Style::default().fg(Color::Green);
            for i in 0..fill_amount {
                buf.set_string(bar_x + i as u16, bar_y, "━", green_style);
            }

            // Draw position indicator
            let indicator_pos = (self.value * (bar_width - 1) as f32) as u16;
            let indicator_style = if self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            buf.set_string(bar_x + indicator_pos, bar_y, "●", indicator_style);
        }

        // Draw value display
        if !self.value_display.is_empty() && area.height >= 3 {
            let val_style = if self.selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let val_x = area.x + (area.width.saturating_sub(self.value_display.len() as u16)) / 2;
            buf.set_string(val_x, area.y + 2, &self.value_display, val_style);
        }
    }
}

/// Minimalist deck playback indicator - clean futuristic design
/// Shows playback state with animated waveform/rotation indicator
pub struct DeckIndicator {
    /// Is the deck playing
    playing: bool,
    /// Playback speed (1.0 = normal)
    speed: f32,
    /// Animation frame (0-15 for smooth rotation)
    frame: u8,
    /// Deck label (A or B)
    label: char,
    /// Deck color
    color: Color,
    /// Source name (displayed when connected)
    source_name: Option<String>,
    /// Whether this control is selected
    selected: bool,
    /// Whether source is connected
    connected: bool,
}

impl DeckIndicator {
    pub fn new(label: char) -> Self {
        Self {
            playing: false,
            speed: 1.0,
            frame: 0,
            label,
            color: Color::White,
            source_name: None,
            selected: false,
            connected: false,
        }
    }

    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    pub fn frame(mut self, frame: u8) -> Self {
        self.frame = frame;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    
    pub fn source_name(mut self, name: Option<String>) -> Self {
        self.source_name = name;
        self
    }
    
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    
    pub fn connected(mut self, connected: bool) -> Self {
        self.connected = connected;
        self
    }
}

impl Widget for DeckIndicator {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 5 || area.height < 3 {
            // Ultra-minimal: just show play state
            let symbol = if self.playing { "▶" } else { "⏸" };
            let style = if self.playing {
                Style::default().fg(self.color)
            } else {
                Style::default().fg(Color::Rgb(40, 40, 40))
            };
            buf.set_string(area.x + area.width / 2, area.y + area.height / 2, symbol, style);
            return;
        }

        let cx = area.x + area.width / 2;
        let cy = area.y + area.height / 2;

        // Futuristic ring animation - 8 segments around center
        // When playing, one segment is highlighted and rotates
        
        // Calculate which segment to highlight based on frame and speed
        let highlight_pos = if self.playing {
            ((self.frame as f32 * self.speed) as usize) % 8
        } else {
            8 // No highlight when stopped
        };

        // Dim color for inactive elements
        let dim_style = Style::default().fg(Color::Rgb(35, 35, 35));
        let active_style = Style::default().fg(self.color);
        let glow_style = Style::default().fg(self.color).add_modifier(Modifier::BOLD);
        let selected_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

        // Draw outer ring (positions relative to center)
        // Top-left, top, top-right, right, bottom-right, bottom, bottom-left, left
        let positions: [(i16, i16, &str); 8] = [
            (-2, -1, "╭"),  // 0: top-left
            (0, -1, "─"),   // 1: top
            (2, -1, "╮"),   // 2: top-right
            (2, 0, "│"),    // 3: right
            (2, 1, "╯"),    // 4: bottom-right
            (0, 1, "─"),    // 5: bottom
            (-2, 1, "╰"),   // 6: bottom-left
            (-2, 0, "│"),   // 7: left
        ];

        for (i, (dx, dy, ch)) in positions.iter().enumerate() {
            let x = (cx as i16 + dx) as u16;
            let y = (cy as i16 + dy) as u16;
            
            let style = if self.selected {
                // When selected, show ring in yellow
                if self.playing && i == highlight_pos {
                    selected_style
                } else {
                    Style::default().fg(Color::Yellow)
                }
            } else if self.playing {
                if i == highlight_pos {
                    glow_style
                } else if i == (highlight_pos + 7) % 8 || i == (highlight_pos + 1) % 8 {
                    // Trail effect
                    active_style
                } else {
                    dim_style
                }
            } else {
                dim_style
            };
            
            buf.set_string(x, y, ch, style);
        }

        // Center: play/pause icon (toggleable when source connected)
        let center_char = if self.connected {
            if self.playing { "▶" } else { "⏸" }
        } else {
            // No source - show deck label dimmed
            &self.label.to_string()
        };
        
        let center_style = if self.selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if self.playing {
            Style::default().fg(self.color).add_modifier(Modifier::BOLD)
        } else if self.connected {
            Style::default().fg(Color::Rgb(100, 100, 100))
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
        };
        buf.set_string(cx, cy, center_char, center_style);

        // Speed indicator below (only if not 1.0x)
        if area.height >= 4 && (self.speed - 1.0).abs() > 0.01 {
            let speed_str = format!("{:.1}x", self.speed);
            let speed_x = cx.saturating_sub(speed_str.len() as u16 / 2);
            let speed_style = Style::default().fg(Color::Rgb(80, 80, 80));
            buf.set_string(speed_x, cy + 2, &speed_str, speed_style);
        }
        
        // Source name at bottom of area (with 1 line gap from indicator)
        if let Some(ref name) = self.source_name {
            let max_len = area.width.saturating_sub(2) as usize;
            let display = if name.len() > max_len {
                format!("{}…", &name[..max_len.saturating_sub(1)])
            } else {
                name.clone()
            };
            let name_x = area.x + (area.width.saturating_sub(display.len() as u16)) / 2;
            // Put source name 2 rows below center (1 row gap)
            let name_y = (cy + 3).min(area.y + area.height.saturating_sub(1));
            let name_style = if self.selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(self.color)
            };
            buf.set_string(name_x, name_y, &display, name_style);
        }
    }
}

/// Spinning disk widget for DJ decks - shows playback state
#[allow(dead_code)]
pub struct SpinningDisk {
    /// Is the deck playing
    playing: bool,
    /// Animation frame (0-7)
    frame: u8,
    /// Deck label (A or B)
    label: char,
    /// Deck color
    color: Color,
}

impl SpinningDisk {
    pub fn new(label: char) -> Self {
        Self {
            playing: false,
            frame: 0,
            label,
            color: Color::White,
        }
    }

    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    pub fn frame(mut self, frame: u8) -> Self {
        self.frame = frame % 8;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl Widget for SpinningDisk {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 7 || area.height < 5 {
            // Minimal representation
            let style = if self.playing {
                Style::default().fg(self.color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let symbol = if self.playing { "▶" } else { "◉" };
            buf.set_string(area.x + area.width / 2, area.y + area.height / 2, symbol, style);
            return;
        }

        let cx = area.x + area.width / 2;
        let cy = area.y + area.height / 2;

        // Disk appearance based on playing state and frame
        // Frame rotates the "groove" marker around the disk
        let groove_positions: [(i16, i16); 8] = [
            (0, -2),   // top
            (2, -1),   // top-right
            (2, 0),    // right
            (2, 1),    // bottom-right
            (0, 2),    // bottom
            (-2, 1),   // bottom-left
            (-2, 0),   // left
            (-2, -1),  // top-left
        ];

        let disk_style = if self.playing {
            Style::default().fg(self.color)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let groove_style = if self.playing {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(80, 80, 80))
        };

        // Draw disk outline (circle approximation)
        //    ╭───╮
        //   ╱     ╲
        //  │   A   │
        //   ╲     ╱
        //    ╰───╯

        buf.set_string(cx - 2, cy - 2, "╭───╮", disk_style);
        buf.set_string(cx - 3, cy - 1, "╱     ╲", disk_style);
        buf.set_string(cx - 3, cy,     &format!("│  {}  │", self.label), disk_style);
        buf.set_string(cx - 3, cy + 1, "╲     ╱", disk_style);
        buf.set_string(cx - 2, cy + 2, "╰───╯", disk_style);

        // Draw spinning groove marker when playing
        if self.playing {
            let (gx, gy) = groove_positions[self.frame as usize];
            let marker_x = (cx as i16 + gx) as u16;
            let marker_y = (cy as i16 + gy) as u16;
            buf.set_string(marker_x, marker_y, "◆", groove_style);
        }

        // Draw play indicator below
        let status_y = cy + 3;
        if status_y < area.y + area.height {
            let status = if self.playing { "▶ PLAY" } else { "■ STOP" };
            let status_style = if self.playing {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let status_x = cx.saturating_sub(status.len() as u16 / 2);
            buf.set_string(status_x, status_y, status, status_style);
        }
    }
}

/// Visual style for knobs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KnobStyle {
    /// Standard EQ/pan knob
    #[default]
    Standard,
    /// Filter knob with frequency arc visualization
    Filter,
    /// Large knob for prominent controls
    Large,
}

/// Rotary knob widget for EQ, pan, and filter controls
pub struct Knob {
    /// Current value (0.0 to 1.0, or -1.0 to 1.0 for bipolar)
    value: f32,
    /// Is bipolar (centered at 0)
    bipolar: bool,
    /// Is this knob selected
    selected: bool,
    /// Label above the knob
    label: String,
    /// Value display below
    value_display: String,
    /// Visual style
    style: KnobStyle,
    /// Color for the value arc
    arc_color: Color,
}

impl Knob {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            bipolar: false,
            selected: false,
            label: String::new(),
            value_display: String::new(),
            style: KnobStyle::Standard,
            arc_color: Color::Cyan,
        }
    }

    pub fn bipolar(mut self, bipolar: bool) -> Self {
        self.bipolar = bipolar;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn value_display(mut self, display: impl Into<String>) -> Self {
        self.value_display = display.into();
        self
    }

    pub fn style(mut self, style: KnobStyle) -> Self {
        self.style = style;
        self
    }

    pub fn arc_color(mut self, color: Color) -> Self {
        self.arc_color = color;
        self
    }
}

impl Widget for Knob {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 5 || area.height < 3 {
            return;
        }

        let center_x = area.x + area.width / 2;
        let center_y = area.y + area.height / 2;

        // Draw label above
        if !self.label.is_empty() {
            let label_style = if self.selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let label_x = area.x + area.width.saturating_sub(self.label.len() as u16) / 2;
            buf.set_string(label_x, area.y, &self.label, label_style);
        }

        let knob_style = if self.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        match self.style {
            KnobStyle::Standard => self.render_standard_knob(area, buf, center_x, center_y, knob_style),
            KnobStyle::Filter => self.render_filter_knob(area, buf, center_x, center_y, knob_style),
            KnobStyle::Large => self.render_large_knob(area, buf, center_x, center_y, knob_style),
        }

        // Draw value display below
        if !self.value_display.is_empty() {
            let val_y = center_y + 2;
            if val_y < area.y + area.height {
                let val_style = if self.selected {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Green)
                };
                let val_x = area.x + area.width.saturating_sub(self.value_display.len() as u16) / 2;
                buf.set_string(val_x, val_y, &self.value_display, val_style);
            }
        }
    }
}

impl Knob {
    fn render_standard_knob(&self, _area: Rect, buf: &mut Buffer, cx: u16, cy: u16, style: Style) {
        let knob_y = cy.saturating_sub(1);
        
        buf.set_string(cx.saturating_sub(2), knob_y, "╭───╮", style);
        
        let pointer = self.get_pointer_char();
        let middle = format!("│ {} │", pointer);
        buf.set_string(cx.saturating_sub(2), knob_y + 1, &middle, style);
        
        buf.set_string(cx.saturating_sub(2), knob_y + 2, "╰───╯", style);
        
        self.render_value_arc(buf, cx, knob_y, style);
    }

    fn render_filter_knob(&self, _area: Rect, buf: &mut Buffer, cx: u16, cy: u16, style: Style) {
        let knob_y = cy.saturating_sub(1);
        
        let arc_style = Style::default().fg(self.arc_color);
        
        let fill_width = 5;
        let filled = (self.value * fill_width as f32) as usize;
        
        let mut indicator = String::from("╭");
        for i in 0..fill_width {
            if i < filled {
                indicator.push('━');
            } else {
                indicator.push('─');
            }
        }
        indicator.push('╮');
        
        let indicator_style = if self.selected {
            Style::default().fg(Color::Yellow)
        } else {
            arc_style
        };
        buf.set_string(cx.saturating_sub(3), knob_y.saturating_sub(1), &indicator, indicator_style);
        
        let pointer = self.get_pointer_char();
        buf.set_string(cx.saturating_sub(2), knob_y, "┌───┐", style);
        buf.set_string(cx.saturating_sub(2), knob_y + 1, &format!("│ {} │", pointer), style);
        buf.set_string(cx.saturating_sub(2), knob_y + 2, "└───┘", style);
        
        let marker_style = Style::default().fg(Color::DarkGray);
        buf.set_string(cx.saturating_sub(3), knob_y + 3, "20", marker_style);
        buf.set_string(cx + 2, knob_y + 3, "20k", marker_style);
    }

    fn render_large_knob(&self, _area: Rect, buf: &mut Buffer, cx: u16, cy: u16, style: Style) {
        let knob_y = cy.saturating_sub(2);
        
        let pointer = self.get_pointer_char();
        
        buf.set_string(cx.saturating_sub(3), knob_y, "╭─────╮", style);
        buf.set_string(cx.saturating_sub(4), knob_y + 1, "│       │", style);
        buf.set_string(cx.saturating_sub(4), knob_y + 2, &format!("│   {}   │", pointer), style);
        buf.set_string(cx.saturating_sub(4), knob_y + 3, "│       │", style);
        buf.set_string(cx.saturating_sub(3), knob_y + 4, "╰─────╯", style);
        
        self.render_large_value_arc(buf, cx, knob_y, style);
    }

    fn get_pointer_char(&self) -> char {
        let normalized = if self.bipolar {
            (self.value + 1.0) / 2.0
        } else {
            self.value
        };
        
        let position = (normalized * 8.0) as i32;
        
        match position.clamp(0, 8) {
            0 => '◣',
            1 => '◀',
            2 => '◤',
            3 => '▲',
            4 => '◥',
            5 => '▶',
            6 => '◢',
            7 | 8 => '▼',
            _ => '●',
        }
    }

    fn render_value_arc(&self, buf: &mut Buffer, cx: u16, knob_y: u16, _style: Style) {
        let arc_style = if self.selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(self.arc_color)
        };
        
        let normalized = if self.bipolar {
            (self.value + 1.0) / 2.0
        } else {
            self.value
        };
        
        let positions = 5;
        let active_pos = (normalized * positions as f32) as usize;
        
        let mut arc = String::new();
        for i in 0..positions {
            if i < active_pos {
                arc.push('●');
            } else {
                arc.push('·');
            }
            if i < positions - 1 {
                arc.push(' ');
            }
        }
        
        let arc_x = cx.saturating_sub((arc.len() / 2) as u16);
        if knob_y > 0 {
            buf.set_string(arc_x, knob_y.saturating_sub(1), &arc, arc_style);
        }
    }

    fn render_large_value_arc(&self, buf: &mut Buffer, cx: u16, knob_y: u16, _style: Style) {
        let arc_style = if self.selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(self.arc_color)
        };
        
        let normalized = if self.bipolar {
            (self.value + 1.0) / 2.0
        } else {
            self.value
        };
        
        let positions = 7;
        let active_pos = (normalized * positions as f32) as usize;
        
        let mut arc = String::new();
        for i in 0..positions {
            if self.bipolar {
                let center = positions / 2;
                if (i < center && i >= center - (center - active_pos).min(center)) ||
                   (i > center && i <= center + (active_pos.saturating_sub(center)).min(center)) ||
                   i == center {
                    arc.push(if i == center { '◆' } else { '●' });
                } else {
                    arc.push('·');
                }
            } else if i < active_pos {
                arc.push('●');
            } else {
                arc.push('·');
            }
            if i < positions - 1 {
                arc.push(' ');
            }
        }
        
        let arc_x = cx.saturating_sub((arc.len() / 2) as u16);
        if knob_y > 0 {
            buf.set_string(arc_x, knob_y.saturating_sub(1), &arc, arc_style);
        }
    }
}

/// Horizontal crossfader widget for DJ mixing
pub struct Crossfader {
    /// Current position (-1.0 = A, 0.0 = center, 1.0 = B)
    position: f32,
    /// Is selected
    selected: bool,
    /// Label for deck A
    label_a: String,
    /// Label for deck B
    label_b: String,
}

impl Crossfader {
    pub fn new(position: f32) -> Self {
        Self {
            position: position.clamp(-1.0, 1.0),
            selected: false,
            label_a: "A".to_string(),
            label_b: "B".to_string(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn labels(mut self, a: impl Into<String>, b: impl Into<String>) -> Self {
        self.label_a = a.into();
        self.label_b = b.into();
        self
    }
}

impl Widget for Crossfader {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 3 {
            return;
        }

        let track_y = area.y + area.height / 2;
        let track_left = area.x + 2;
        let track_right = area.x + area.width - 3;
        let track_width = track_right - track_left;

        // Draw track
        let track_style = Style::default().fg(Color::DarkGray);
        buf.set_string(track_left, track_y, &"─".repeat(track_width as usize), track_style);
        
        // Draw end caps
        buf.set_string(track_left - 1, track_y, "├", track_style);
        buf.set_string(track_right, track_y, "┤", track_style);

        // Draw center notch
        let center_x = track_left + track_width / 2;
        buf.set_string(center_x, track_y - 1, "▼", Style::default().fg(Color::DarkGray));

        // Draw deck labels
        let label_style = Style::default().fg(Color::Cyan);
        buf.set_string(area.x, track_y, &self.label_a, label_style);
        buf.set_string(area.x + area.width - self.label_b.len() as u16, track_y, &self.label_b, label_style);

        // Draw fader cap
        let cap_pos = ((self.position + 1.0) / 2.0 * track_width as f32) as u16;
        let cap_x = track_left + cap_pos;

        let cap_style = if self.selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        buf.set_string(cap_x.saturating_sub(1), track_y - 1, "┌─┐", cap_style);
        buf.set_string(cap_x.saturating_sub(1), track_y, "│█│", cap_style);
        buf.set_string(cap_x.saturating_sub(1), track_y + 1, "└─┘", cap_style);
    }
}

/// Level meter widget (VU meter style)
pub struct LevelMeter {
    /// Current level (0.0 to 1.0)
    level: f32,
    /// Peak hold level
    peak: f32,
    /// Is stereo (draw L/R)
    stereo: bool,
    /// Right channel level (for stereo)
    level_r: f32,
    /// Right peak
    peak_r: f32,
}

impl LevelMeter {
    pub fn new(level: f32) -> Self {
        Self {
            level: level.clamp(0.0, 1.0),
            peak: 0.0,
            stereo: false,
            level_r: 0.0,
            peak_r: 0.0,
        }
    }

    pub fn peak(mut self, peak: f32) -> Self {
        self.peak = peak.clamp(0.0, 1.0);
        self
    }

    pub fn stereo(mut self, level_l: f32, level_r: f32) -> Self {
        self.stereo = true;
        self.level = level_l.clamp(0.0, 1.0);
        self.level_r = level_r.clamp(0.0, 1.0);
        self
    }

    pub fn peaks(mut self, peak_l: f32, peak_r: f32) -> Self {
        self.peak = peak_l.clamp(0.0, 1.0);
        self.peak_r = peak_r.clamp(0.0, 1.0);
        self
    }
}

impl Widget for LevelMeter {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 {
            return;
        }

        let draw_single_meter = |buf: &mut Buffer, x: u16, level: f32, peak: f32| {
            let meter_top = area.y;
            let meter_bottom = area.y + area.height - 1;
            let meter_height = meter_bottom - meter_top;

            // Draw background
            for y in meter_top..=meter_bottom {
                buf.set_string(x, y, "░", Style::default().fg(Color::DarkGray));
            }

            // Draw level
            let level_height = (level * meter_height as f32) as u16;
            for i in 0..level_height {
                let y = meter_bottom - i;
                let color = if i > (meter_height * 85 / 100) {
                    Color::Red
                } else if i > (meter_height * 70 / 100) {
                    Color::Yellow
                } else {
                    Color::Green
                };
                buf.set_string(x, y, "█", Style::default().fg(color));
            }

            // Draw peak hold
            if peak > 0.0 {
                let peak_y = meter_bottom - (peak * meter_height as f32) as u16;
                let peak_y = peak_y.clamp(meter_top, meter_bottom);
                let peak_color = if peak > 0.9 { Color::Red } else { Color::White };
                buf.set_string(x, peak_y, "▬", Style::default().fg(peak_color));
            }
        };

        if self.stereo && area.width >= 3 {
            let left_x = area.x + area.width / 2 - 1;
            let right_x = area.x + area.width / 2 + 1;
            draw_single_meter(buf, left_x, self.level, self.peak);
            draw_single_meter(buf, right_x, self.level_r, self.peak_r);
        } else {
            let x = area.x + area.width / 2;
            draw_single_meter(buf, x, self.level, self.peak);
        }
    }
}

/// Button widget for mute/solo/pfl
pub struct Button {
    label: String,
    active: bool,
    selected: bool,
    active_color: Color,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            active: false,
            selected: false,
            active_color: Color::Red,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn active_color(mut self, color: Color) -> Self {
        self.active_color = color;
        self
    }
}

impl Widget for Button {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 1 {
            return;
        }

        let style = if self.active {
            Style::default()
                .fg(Color::Black)
                .bg(self.active_color)
                .add_modifier(Modifier::BOLD)
        } else if self.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        let border_style = if self.selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let center_y = area.y + area.height / 2;

        if area.height >= 3 && center_y > area.y {
            let top = "┌".to_string() + &"─".repeat(area.width.saturating_sub(2) as usize) + "┐";
            buf.set_string(area.x, center_y - 1, &top, border_style);
        }

        let padded_label = format!(
            "│{:^width$}│",
            self.label,
            width = area.width.saturating_sub(2) as usize
        );
        buf.set_string(area.x, center_y, &padded_label, style);

        if area.height >= 3 && center_y + 1 < area.y + area.height {
            let bottom = "└".to_string() + &"─".repeat(area.width.saturating_sub(2) as usize) + "┘";
            buf.set_string(area.x, center_y + 1, &bottom, border_style);
        }
    }
}

/// Source selector dropdown widget
pub struct SourceSelector<'a> {
    sources: &'a [String],
    selected_index: usize,
    open: bool,
    focused: bool,
    highlight_index: usize,
}

impl<'a> SourceSelector<'a> {
    pub fn new(sources: &'a [String]) -> Self {
        Self {
            sources,
            selected_index: 0,
            open: false,
            focused: false,
            highlight_index: 0,
        }
    }

    pub fn selected_index(mut self, index: usize) -> Self {
        self.selected_index = index;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn highlight_index(mut self, index: usize) -> Self {
        self.highlight_index = index;
        self
    }
}

impl<'a> Widget for SourceSelector<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 5 || area.height < 1 {
            return;
        }

        let border_style = if self.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let selected_name = self.sources
            .get(self.selected_index)
            .map(|s| s.as_str())
            .unwrap_or("None");
        
        let display_width = area.width.saturating_sub(4) as usize;
        let truncated = if selected_name.len() > display_width {
            format!("{}…", &selected_name[..display_width.saturating_sub(1)])
        } else {
            selected_name.to_string()
        };

        let arrow = if self.open { "▲" } else { "▼" };
        let selector_text = format!("│{:width$}{}│", truncated, arrow, width = display_width);
        
        buf.set_string(area.x, area.y, &format!("┌{}┐", "─".repeat(area.width.saturating_sub(2) as usize)), border_style);
        buf.set_string(area.x, area.y + 1, &selector_text, if self.focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        });
        buf.set_string(area.x, area.y + 2, &format!("└{}┘", "─".repeat(area.width.saturating_sub(2) as usize)), border_style);

        if self.open && area.height > 3 {
            let dropdown_y = area.y + 3;
            let max_items = (area.height - 3).min(self.sources.len() as u16) as usize;

            for (i, source) in self.sources.iter().take(max_items).enumerate() {
                let y = dropdown_y + i as u16;
                let is_highlighted = i == self.highlight_index;
                
                let item_style = if is_highlighted {
                    Style::default().fg(Color::Black).bg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                };

                let display = if source.len() > display_width {
                    format!("{}…", &source[..display_width.saturating_sub(1)])
                } else {
                    format!("{:width$}", source, width = display_width)
                };

                buf.set_string(area.x, y, &format!("│{}  │", display), item_style);
            }

            let bottom_y = dropdown_y + max_items as u16;
            buf.set_string(area.x, bottom_y, &format!("└{}┘", "─".repeat(area.width.saturating_sub(2) as usize)), border_style);
        }
    }
}
