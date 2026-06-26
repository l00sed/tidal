//! Tidal Mixer - A TUI audio mixer with LPF/HPF controls
//!
//! A professional-looking terminal-based audio mixer for controlling
//! multiple audio sources (like MPV instances) with vim-like navigation.

mod app;
mod audio;
mod state;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, prelude::Backend, Terminal};
use std::io::stdout;
use std::path::PathBuf;

use crate::audio::{SourceDiscovery, SourceType};
use ui::MixerView;

fn main() -> Result<()> {
    // Logging disabled — TUI renders to stdout so tracing output would corrupt it.
    // Use the DEBUG=1 env var for the built-in debug pane instead.

    // Parse command line arguments for MPV socket paths
    let args: Vec<String> = std::env::args().collect();
    let cli = parse_args(&args);

    // Create application
    let mut app = App::new(2); // Start with 2 channels
    
    // Set music directory if provided
    if let Some(dir) = cli.music_dir {
        app.set_music_dir(dir);
    }
    
    // Set samples directory if provided
    if let Some(dir) = cli.samples_dir {
        app.set_samples_dir(dir);
    }

    if !cli.sources.is_empty() {
        app.configure_sources(cli.sources);
    } else if cli.auto_discover {
        // Auto-discover audio sources
        let discovered = discover_sources();
        if !discovered.is_empty() {
            app.configure_sources(discovered);
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // When DEBUG is not set, redirect stderr to /dev/null to prevent
    // library warnings from corrupting the TUI
    let _stderr_guard = if std::env::var("DEBUG").is_err() {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let devnull = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .ok();
            devnull.map(|f| {
                let fd = f.as_raw_fd();
                unsafe { libc::dup2(fd, 2); }
                f  // Return file to keep it alive
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    } else {
        None
    };

    // Run the application
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    Ok(())
}

struct CliArgs {
    sources: Vec<(String, String)>,
    auto_discover: bool,
    music_dir: Option<PathBuf>,
    samples_dir: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> CliArgs {
    let mut sources = Vec::new();
    let mut auto_discover = false;
    let mut music_dir = None;
    let mut samples_dir = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--source" | "-s" => {
                if i + 2 < args.len() {
                    let name = args[i + 1].clone();
                    let socket = args[i + 2].clone();
                    sources.push((name, socket));
                    i += 3;
                } else {
                    eprintln!("--source requires NAME and SOCKET_PATH arguments");
                    i += 1;
                }
            }
            "--music-dir" | "-m" => {
                if i + 1 < args.len() {
                    music_dir = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("--music-dir requires a PATH argument");
                    i += 1;
                }
            }
            "--samples-dir" | "-S" => {
                if i + 1 < args.len() {
                    samples_dir = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("--samples-dir requires a PATH argument");
                    i += 1;
                }
            }
            "--discover" | "-d" => {
                auto_discover = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                i += 1;
            }
        }
    }

    // Auto-discover if no sources specified
    if sources.is_empty() {
        auto_discover = true;
    }

    CliArgs { sources, auto_discover, music_dir, samples_dir }
}

/// Discover audio sources (MPV sockets, PulseAudio, etc.)
fn discover_sources() -> Vec<(String, String)> {
    let mut discovery = SourceDiscovery::new();
    let sources = discovery.discover_all();
    
    sources
        .iter()
        .filter(|s| s.source_type == SourceType::Mpv)  // Start with MPV only
        .map(|s| (s.name.clone(), s.identifier.clone()))
        .collect()
}

fn print_help() {
    println!(
        r#"Tidal Mixer - TUI Audio Mixer

USAGE:
    tidal-mixer [OPTIONS]

OPTIONS:
    -s, --source NAME SOCKET    Add an audio source (MPV IPC socket)
    -m, --music-dir PATH        Directory for audio file browser (default: cwd)
    -S, --samples-dir PATH      Directory for sample pad files (default: ~/Library/Application Support/SuperCollider/downloaded-quarks/Dirt-Samples)
    -d, --discover              Auto-discover audio sources (default if no -s)
    -h, --help                  Show this help message

EXAMPLES:
    # Start with two MPV instances
    tidal-mixer -s "Music" /tmp/mpv-music.sock -s "Effects" /tmp/mpv-fx.sock
    
    # Start with music and samples directories
    tidal-mixer -m ~/Music -S ~/Samples

    # Start MPV with IPC socket:
    mpv --input-ipc-server=/tmp/mpv-music.sock music.mp3

KEYBOARD CONTROLS:
    Tab, h/l     Navigate between panes (Deck A, DJ, Deck B, Master)
    Enter        Enter control select / edit mode
    Esc          Go back one mode level
    A            Open source picker for Deck A
    B            Open source picker for Deck B
    P            Toggle sample pads mode
    ?            Show help
    q            Quit
    J/K          Coarse adjustment
    +/-          Fine adjustment
    SPACE/ENTER  Toggle mute/solo
    0            Reset control to default
    c            Center pan
    m            Toggle mute
    s            Toggle solo
    ?            Show help
    q, ESC       Quit

MOUSE:
    Click        Select control
    Drag         Adjust fader/knob value
    Scroll       Fine adjustment
"#
    );
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    let mut frame_counter: u8 = 0;
    
    loop {
        // Update terminal height for scroll calculations
        app.terminal_height = terminal.size()?.height;
        app.mixer.terminal_height = app.terminal_height;

        // Draw
        terminal.draw(|frame| {
            let control_select = matches!(app.mode, app::AppMode::ControlSelect | app::AppMode::Edit);
            let pad_config = app.mode == app::AppMode::SamplePadConfig;
            
            // Bind device lists to extend lifetime
            let master_devices = app.master_output.devices();
            let cue_devices = app.cue_output.devices();
            
            let mut view = MixerView::new(&app.mixer, &app.sample_pads)
                .show_help(app.show_help())
                .editing(app.is_editing())
                .control_select(control_select)
                .frame(frame_counter)
                .selected_pane(app.selected_pane)
                .selected_pad_idx(app.selected_pad_idx)
                .pad_config_mode(pad_config)
                .pad_config_editing(app.sample_pads.editing_control)
                .racks(&app.rack_state)
                .scroll_offset(app.rack_scroll_offset)
                .master_output_device(app.master_output.selected_device())
                .cue_output_device(app.cue_output.selected_device())
                .output_picker_active(app.output_picker_active)
                .output_picker_target(app.output_picker_target)
                .master_output_devices(&master_devices)
                .cue_output_devices(&cue_devices)
                .selected_master_output_idx(app.selected_master_output_idx)
                .selected_cue_output_idx(app.selected_cue_output_idx)
                .debug_log(&app.debug_log)
                .samples_dir(Some(&app.samples_dir));
            
            // Add source picker if active
            if let app::AppMode::SourcePicker(deck) = app.mode {
                view = view.source_picker(deck, &app.source_picker);
            }
            
            // Add sample picker if active
            if let app::AppMode::SamplePicker(pad_idx) = app.mode {
                view = view.sample_picker(pad_idx, &app.source_picker);
            }
            
            frame.render_widget(view, frame.area());

            // Update areas for mouse hit testing
            let (channel_areas, xfader_area, master_area, cue_area, loops_area, pad_areas) =
                calculate_all_areas(frame.area(), app.mixer.channels.len());
            app.update_channel_areas(channel_areas);
            app.update_pane_areas(xfader_area, master_area, cue_area, loops_area, pad_areas);
        })?;

        // Handle events
        let timeout = app.tick_rate.saturating_sub(app.last_tick.elapsed());

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    // Ctrl+c always quits
                    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        return Ok(());
                    }
                    app.handle_key(key);
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse);
                }
                Event::Resize(_, _) => {
                    // Terminal will redraw automatically
                }
                _ => {}
            }
        }

        // Check if we should quit
        if app.should_quit {
            return Ok(());
        }

        // Tick for animations/meters
        if app.last_tick.elapsed() >= app.tick_rate {
            app.elapsed_ms += 50; // 50ms per tick at 20fps
            app.update_racks();
            app.tick();
            app.last_tick = std::time::Instant::now();
            frame_counter = frame_counter.wrapping_add(1);
        }
    }
}

/// Calculate screen areas for all panes for mouse hit testing.
/// Returns (channel_areas, xfader_area, master_area, cue_area, loops_area, pad_areas).
fn calculate_all_areas(
    area: Rect,
    num_channels: usize,
) -> (
    Vec<app::ChannelArea>,
    Option<app::PaneArea>,
    Option<app::PaneArea>,
    Option<app::PaneArea>,
    Option<app::PaneArea>,
    Vec<(usize, u16, u16, u16, u16)>,
) {
    use crate::state::ChannelControl;

    if num_channels == 0 {
        return (Vec::new(), None, None, None, None, Vec::new());
    }

    // Mirror the layout from MixerView::render_mixer_full_width
    let header_height = 3u16;
    let footer_height = 3u16;
    let mixer_area = Rect::new(
        area.x,
        area.y + header_height,
        area.width,
        area.height.saturating_sub(header_height + footer_height),
    );

    let deck_max_width = 21u16;
    let master_width = 21u16;
    let min_dj_width = 20u16;

    let total_fixed = deck_max_width * 2 + master_width + min_dj_width;
    let deck_width = if mixer_area.width >= total_fixed {
        deck_max_width
    } else {
        ((mixer_area.width.saturating_sub(master_width + min_dj_width)) / 2).max(10)
    };

    let dj_center_width = mixer_area.width.saturating_sub(deck_width * 2 + master_width);

    // Horizontal chunks: [Deck A] [DJ center] [Deck B] [Master column]
    let chunk_a_x = mixer_area.x;
    let chunk_dj_x = chunk_a_x + deck_width;
    let chunk_b_x = chunk_dj_x + dj_center_width;
    let chunk_m_x = chunk_b_x + deck_width;

    // DJ center vertical: [Pads] [Loops] [Xfader]
    let loops_height = (mixer_area.height as f32 * 0.20) as u16;
    let loops_height = loops_height.max(3);
    let xfader_height = 5u16;

    let pads_y = mixer_area.y;
    let pads_h = mixer_area.height.saturating_sub(loops_height + xfader_height);
    let loops_y = pads_y + pads_h;
    let xfader_y = loops_y + loops_height;

    // Master column vertical: [CUE] [Master]
    let cue_h = (mixer_area.height as f32 * 0.66) as u16;
    let master_h = mixer_area.height.saturating_sub(cue_h);
    let cue_y = mixer_area.y;
    let master_y = cue_y + cue_h;

    // ── Channel strip areas ──
    let channel_width = (dj_center_width / num_channels as u16).max(10);
    let mut channel_areas = Vec::with_capacity(num_channels);
    for i in 0..num_channels {
        let x = chunk_a_x + i as u16 * channel_width;
        let w = channel_width.min(mixer_area.width.saturating_sub(x - mixer_area.x));
        let inner_y = mixer_area.y + 1;
        let inner_h = mixer_area.height.saturating_sub(2);
        let row_heights = [4, 4, 4, 3, 3, 4, 8, 3, 3, 3];
        let total: u16 = row_heights.iter().sum();
        let scale = inner_h as f32 / total as f32;
        let controls = [
            ChannelControl::EqHigh,
            ChannelControl::EqMid,
            ChannelControl::EqLow,
            ChannelControl::HighPassFilter,
            ChannelControl::LowPassFilter,
            ChannelControl::Pan,
            ChannelControl::Fader,
            ChannelControl::Mute,
            ChannelControl::Solo,
        ];
        let mut control_rows = Vec::new();
        let mut current_y = inner_y;
        for (j, &control) in controls.iter().enumerate() {
            let row_h = (row_heights[j] as f32 * scale) as u16;
            let y_end = current_y + row_h;
            control_rows.push((control, current_y, y_end));
            current_y = y_end;
        }
        channel_areas.push(app::ChannelArea {
            bounds: (x, mixer_area.y, w, mixer_area.height),
            control_rows,
        });
    }

    // ── Crossfader area ──
    let xfader_area = app::PaneArea {
        x: chunk_dj_x,
        y: xfader_y,
        w: dj_center_width,
        h: xfader_height,
    };

    // ── Master area ──
    let master_area = app::PaneArea {
        x: chunk_m_x,
        y: master_y,
        w: master_width.min(mixer_area.width.saturating_sub(chunk_m_x - mixer_area.x)),
        h: master_h,
    };

    // ── CUE area ──
    let cue_area = app::PaneArea {
        x: chunk_m_x,
        y: cue_y,
        w: master_width.min(mixer_area.width.saturating_sub(chunk_m_x - mixer_area.x)),
        h: cue_h,
    };

    // ── Loops area ──
    let loops_area = app::PaneArea {
        x: chunk_dj_x,
        y: loops_y,
        w: dj_center_width,
        h: loops_height,
    };

    // ── Pad areas (4x4 grid centered in pads area) ──
    let mut pad_areas = Vec::new();
    if pads_h >= 6 && dj_center_width >= 12 {
        let cell_w = 5u16;
        let cell_h = 3u16;
        let gap_x = 1u16;
        let grid_w = cell_w * 4 + gap_x * 3;
        let grid_h = cell_h * 4;
        let offset_x = (dj_center_width.saturating_sub(grid_w)) / 2;
        let offset_y = (pads_h.saturating_sub(grid_h)) / 2;
        for row in 0..4u16 {
            for col in 0..4u16 {
                let pad_idx = (row * 4 + col) as usize;
                let x = chunk_dj_x + offset_x + col * (cell_w + gap_x);
                let y = pads_y + offset_y + row * cell_h;
                if x + cell_w <= chunk_dj_x + dj_center_width && y + cell_h <= pads_y + pads_h {
                    pad_areas.push((pad_idx, x, y, cell_w, cell_h));
                }
            }
        }
    }

    (
        channel_areas,
        Some(xfader_area),
        Some(master_area),
        Some(cue_area),
        Some(loops_area),
        pad_areas,
    )
}
