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
    // Initialize logging
    tracing_subscriber::fmt::init();

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
    -S, --samples-dir PATH      Directory for sample pad files (default: cwd)
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
    SPACE/ENTER  Toggle mute/solo/pfl
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
        // Draw
        terminal.draw(|frame| {
            let control_select = matches!(app.mode, app::AppMode::ControlSelect | app::AppMode::Edit);
            let mut view = MixerView::new(&app.mixer, &app.sample_pads)
                .show_help(app.show_help())
                .editing(app.is_editing())
                .control_select(control_select)
                .frame(frame_counter)
                .selected_pane(app.selected_pane)
                .selected_pad_idx(app.selected_pad_idx);
            
            // Add source picker if active
            if let app::AppMode::SourcePicker(deck) = app.mode {
                view = view.source_picker(deck, &app.source_picker);
            }
            
            // Add sample picker if active
            if let app::AppMode::SamplePicker(pad_idx) = app.mode {
                view = view.sample_picker(pad_idx, &app.source_picker);
            }
            
            frame.render_widget(view, frame.area());

            // Update channel areas for mouse hit testing
            let areas = calculate_channel_areas(frame.area(), app.mixer.channels.len());
            app.update_channel_areas(areas);
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
            app.tick();
            app.last_tick = std::time::Instant::now();
            frame_counter = frame_counter.wrapping_add(1);
        }
    }
}

/// Calculate the screen areas for each channel strip for mouse hit testing
fn calculate_channel_areas(area: Rect, num_channels: usize) -> Vec<app::ChannelArea> {
    use crate::state::ChannelControl;

    if num_channels == 0 {
        return Vec::new();
    }

    // Mirror the layout calculations from MixerView
    let header_height = 3u16;
    let footer_height = 3u16;
    let mixer_area = Rect::new(
        area.x,
        area.y + header_height,
        area.width,
        area.height.saturating_sub(header_height + footer_height),
    );

    let master_width = 14u16;
    let available_for_channels = mixer_area.width.saturating_sub(master_width + 2);
    let min_channel_width = 12u16;
    let max_channel_width = 20u16;
    let channel_width = (available_for_channels / num_channels as u16)
        .clamp(min_channel_width, max_channel_width);

    let mut areas = Vec::with_capacity(num_channels);

    for i in 0..num_channels {
        let x = mixer_area.x + (i as u16 * channel_width);
        let y = mixer_area.y;
        let w = channel_width;
        let h = mixer_area.height;

        // Calculate control row positions (approximate based on layout constraints)
        let inner_y = y + 1; // Account for border
        let inner_h = h.saturating_sub(2);

        // These ratios match the Layout constraints in ChannelStrip
        let row_heights = [4, 4, 4, 3, 3, 4, 8, 3, 3, 3]; // Approximate
        let total: u16 = row_heights.iter().sum();
        let scale = inner_h as f32 / total as f32;

        let mut control_rows = Vec::new();
        let mut current_y = inner_y;

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
            ChannelControl::Pfl,
        ];

        for (j, &control) in controls.iter().enumerate() {
            let row_h = (row_heights[j] as f32 * scale) as u16;
            let y_end = current_y + row_h;
            control_rows.push((control, current_y, y_end));
            current_y = y_end;
        }

        areas.push(app::ChannelArea {
            bounds: (x, y, w, h),
            control_rows,
        });
    }

    areas
}
