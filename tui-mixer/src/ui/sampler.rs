//! Sample pad grid widget for TUI

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};

use crate::state::{SamplePadGrid, SamplePad, PAD_KEYS};

/// A single pad cell in the grid
pub struct PadCell<'a> {
    pad: &'a SamplePad,
    selected: bool,
    config_mode: bool,
}

impl<'a> PadCell<'a> {
    pub fn new(pad: &'a SamplePad) -> Self {
        Self {
            pad,
            selected: false,
            config_mode: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn config_mode(mut self, config: bool) -> Self {
        self.config_mode = config;
        self
    }
}

impl<'a> Widget for PadCell<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 2 {
            return;
        }

        let (r, g, b) = self.pad.color;
        let pad_color = Color::Rgb(r, g, b);
        let dim_color = Color::Rgb(r / 3, g / 3, b / 3);

        // Determine colors based on state
        let (bg_color, fg_color, border_color) = if self.pad.playing {
            // Bright when playing
            (pad_color, Color::Black, Color::White)
        } else if self.selected {
            // Selected but not playing
            (dim_color, Color::White, pad_color)
        } else if self.pad.has_sample() {
            // Has sample, dim
            (Color::Rgb(r / 4, g / 4, b / 4), Color::Rgb(r, g, b), Color::DarkGray)
        } else {
            // Empty pad
            (Color::Rgb(30, 30, 30), Color::DarkGray, Color::Rgb(50, 50, 50))
        };

        // Draw border
        let border_style = if self.selected && self.config_mode {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if self.selected {
            Style::default().fg(border_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(border_color)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        // Fill background
        for y in inner.y..inner.y + inner.height {
            for x in inner.x..inner.x + inner.width {
                buf.get_mut(x, y).set_bg(bg_color);
            }
        }

        // Draw key hint in top-left
        let key_char = self.pad.key_char().to_ascii_uppercase();
        let key_style = Style::default()
            .fg(if self.pad.playing { Color::Black } else { fg_color })
            .add_modifier(Modifier::BOLD);
        buf.set_string(inner.x, inner.y, &key_char.to_string(), key_style);

        // Draw play mode indicator in top-right
        if self.pad.has_sample() && inner.width > 4 {
            let mode_label = self.pad.play_mode.label();
            let mode_x = inner.x + inner.width.saturating_sub(mode_label.len() as u16);
            let mode_style = Style::default()
                .fg(if self.pad.playing { Color::Black } else { Color::DarkGray })
                .add_modifier(Modifier::DIM);
            buf.set_string(mode_x, inner.y, mode_label, mode_style);
        }

        // Draw sample name or empty indicator
        if inner.height > 1 {
            let name_y = inner.y + 1;
            let max_len = inner.width as usize;
            
            let display = if self.pad.has_sample() {
                self.pad.display_name(max_len)
            } else if self.config_mode && self.selected {
                "ASSIGN".to_string()
            } else {
                "─────".to_string()
            };
            
            let name_style = Style::default()
                .fg(if self.pad.playing { Color::Black } else { fg_color });
            
            // Center the name
            let name_x = inner.x + (inner.width.saturating_sub(display.len() as u16)) / 2;
            buf.set_string(name_x, name_y, &display, name_style);
        }

        // Draw playing indicator (pulsing dot)
        if self.pad.playing && inner.height > 2 {
            let indicator = "▶";
            let ind_x = inner.x + (inner.width.saturating_sub(1)) / 2;
            let ind_y = inner.y + inner.height.saturating_sub(1);
            buf.set_string(ind_x, ind_y, indicator, Style::default().fg(Color::Black));
        }
    }
}

/// The full 4x4 sample pad grid widget
pub struct SamplePadWidget<'a> {
    grid: &'a SamplePadGrid,
}

impl<'a> SamplePadWidget<'a> {
    pub fn new(grid: &'a SamplePadGrid) -> Self {
        Self { grid }
    }
}

impl<'a> Widget for SamplePadWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Outer block
        let title = if self.grid.config_mode {
            " SAMPLE PADS [CONFIG] "
        } else if self.grid.active {
            " SAMPLE PADS [ACTIVE] "
        } else {
            " SAMPLE PADS "
        };

        let title_style = if self.grid.config_mode {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if self.grid.active {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let border_color = if self.grid.config_mode {
            Color::Yellow
        } else if self.grid.active {
            Color::Green
        } else {
            Color::DarkGray
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(ratatui::text::Span::styled(title, title_style));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width < 12 || inner.height < 8 {
            // Too small to render pads
            buf.set_string(
                inner.x,
                inner.y,
                "Too small",
                Style::default().fg(Color::Red),
            );
            return;
        }

        // Create 4x4 grid layout
        let row_constraints = [
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ];

        let col_constraints = [
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ];

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(inner);

        for (row_idx, row_area) in rows.iter().enumerate() {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints)
                .split(*row_area);

            for (col_idx, col_area) in cols.iter().enumerate() {
                let pad_idx = row_idx * 4 + col_idx;
                let pad = &self.grid.pads[pad_idx];
                let selected = pad_idx == self.grid.selected_pad;

                PadCell::new(pad)
                    .selected(selected)
                    .config_mode(self.grid.config_mode)
                    .render(*col_area, buf);
            }
        }

        // Draw help hint at bottom if space allows
        if area.height > inner.height + 3 {
            let hint = if self.grid.config_mode {
                "hjkl:move │ ENTER:assign │ DEL:clear │ P:mode │ ESC:exit config"
            } else if self.grid.active {
                "Keys trigger pads │ P:toggle active │ C:config"
            } else {
                "P:activate pads"
            };
            
            let hint_y = area.y + area.height.saturating_sub(1);
            buf.set_string(
                area.x + 1,
                hint_y,
                hint,
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

/// Compact horizontal pad strip (for embedding in mixer view)
pub struct PadStrip<'a> {
    grid: &'a SamplePadGrid,
    show_row: Option<usize>, // Show specific row, or all if None
}

impl<'a> PadStrip<'a> {
    pub fn new(grid: &'a SamplePadGrid) -> Self {
        Self {
            grid,
            show_row: None,
        }
    }

    pub fn row(mut self, row: usize) -> Self {
        self.show_row = Some(row);
        self
    }
}

impl<'a> Widget for PadStrip<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let pads_to_show: Vec<&SamplePad> = match self.show_row {
            Some(row) => self.grid.pads[row * 4..(row + 1) * 4].iter().collect(),
            None => self.grid.pads.iter().collect(),
        };

        let num_pads = pads_to_show.len();
        if num_pads == 0 {
            return;
        }

        let pad_width = area.width / num_pads as u16;
        
        for (i, pad) in pads_to_show.iter().enumerate() {
            let x = area.x + (i as u16 * pad_width);
            let pad_area = Rect::new(x, area.y, pad_width, area.height);
            
            let selected = pad.index == self.grid.selected_pad;
            PadCell::new(pad)
                .selected(selected)
                .config_mode(self.grid.config_mode)
                .render(pad_area, buf);
        }
    }
}
