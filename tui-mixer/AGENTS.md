# AGENTS.md — tidal-mixer

## Project Overview

A Rust TUI DJ mixer for live performance with TidalCycles. Controls and mixes audio from MPV players and SuperCollider synthesizers via a terminal interface with vim-like navigation. Real-time DSP with EQ, filtering, crossfading, and sample pads.

## Architecture

```
src/
├── main.rs              # Entry point, CLI parsing, 20fps render loop
├── app.rs               # App state machine (8 modes), event handling, sync logic
├── audio/
│   ├── discovery.rs     # Auto-discover MPV, SC, PulseAudio, PipeWire, JACK sources
│   ├── mpv.rs           # Sync MPV IPC client (Unix socket, lavfi filters)
│   ├── sample_cache.rs  # Rodio-based sample preloader, 16-voice polyphony
│   ├── source.rs        # Async MPV source abstraction (tokio)
│   └── supercollider.rs # OSC client, SynthDef routing, bus mapping
├── state/
│   ├── mixer.rs         # MixerChannel, DjSection, MasterChannel, ChannelControl enum
│   └── sampler.rs       # 4x4 pad grid, play modes (OneShot/Gate/Toggle/Loop)
└── ui/
    ├── colors.rs        # Centralized color constants (use these everywhere)
    ├── channel.rs       # Channel strip + master strip widgets
    ├── mixer.rs         # Main layout (Deck A | DjCenter | Deck B | CUE | Master)
    ├── sampler.rs       # Pad grid widget
    └── widgets.rs       # Fader, Knob, Crossfader, LevelMeter, Button, DeckIndicator
```

## Key Patterns

- **3-level nav**: PaneSelect → ControlSelect → Edit (vim hjkl, Esc to go back)
- **State machine** in `app.rs`: 8 modes handling keyboard + mouse input
- **Channel controls**: 15 variants in `ChannelControl` enum, 9 in `GlobalControl`
- **Crossfader**: 4 curves (Linear, Smooth, Cut, ConstantPower) in `state/mixer.rs`
- **SynthDefs**: Custom SuperCollider synths in `synthdefs/mixerChannel.scd`

## Navigation Model

### Pane Select (default mode)
When no pane is actively selected, you're in **PaneSelect** mode. Navigation keys move the highlight between panes:

- **h / Left** → horizontal left (DeckA↔Master↔DeckB↔Xfader)
- **l / Right** → horizontal right (DeckA↔Xfader↔DeckB↔Master)
- **j / Down** → vertical down (DeckA→DjCenter→Loops→Xfader→DeckB→Master→DjControls)
- **k / Up** → vertical up (reverse of j)
- **Tab / Shift+Tab** → next/prev pane (round-robin)
- **Enter / Space** → enter ControlSelect mode (or Edit for Xfader)
- **p** → toggle Pads mode (DJ Center only)
- **?** → show help

Pane order: `Deck A | DjCenter | Loops | Xfader | Deck B | Master | DjControls`

### Control Select (sub-navigation mode)
Pressing **Enter** on a pane enters **ControlSelect** mode. Navigation keys now move between controls *within* that pane:

- **j / k** → move between controls vertically (faders, knobs, pad rows, rack rows)
- **h / l** → adjust continuous values left/right, or toggle between paired controls (e.g. EQ slider ↔ kill switch)
- **Enter / Space** → toggle a button, or open a picker (pad → sample picker, loop → playback)
- **0** → reset control to default
- **Esc** → return to PaneSelect mode

Control behavior by pane:
| Pane | Controls | h/l behavior |
|------|----------|--------------|
| Deck A/B | Fader, EQ, Filters, Pan | Adjust value |
| DjCenter | Pad grid | Navigate pads |
| Loops | Rack rows | Navigate racks, toggle playback |
| Xfader | Crossfader | Adjust value |
| Master | Fader, Mute, Dim, Mono | Adjust or toggle |
| DjControls | CUE, PH, BT sliders | Adjust value |

### Edit mode
For continuous controls (faders, knobs), pressing **Enter** in ControlSelect enters **Edit** mode:

- **h / j / k / l** → fine adjust the value
- **0** → reset to default
- **Enter / Esc** → return to ControlSelect

### Adding new panes
When adding a new pane:
1. Add variant to `SelectedPane` enum in `app.rs`
2. Update `next()` / `prev()` for Tab navigation
3. Update `sync_pane_to_mixer()` for focus behavior
4. Add pane to `navigate_control_down()` / `navigate_control_up()` for j/k
5. Add pane to `navigate_control_left()` / `navigate_control_right()` for h/l
6. Add pane handling in `handle_control_select_key()` for Enter/Space
7. Add render method in `ui/mixer.rs`
8. Wire up in `render_mixer_full_width()` layout

## Build & Run

```bash
cargo build              # debug
cargo build --release    # release with LTO
cargo run                # auto-discover sources
cargo run -- -s "A" /tmp/mpv-a.sock -s "B" /tmp/mpv-b.sock
DEBUG=1 cargo run        # run with debug panel visible at bottom of screen
```

**Prerequisites**: Rust 2021, [Nerd Fonts](https://www.nerdfonts.com/), MPV, optional SuperCollider.

The `DEBUG=1` environment variable enables a 5-line debug log panel at the bottom of the TUI, showing MPV IPC results, solo state, and errors. Use it to diagnose audio routing or control issues.

## Testing

No test suite currently. Run `cargo check` and `cargo clippy` for validation.

## Conventions

- Rust edition 2021, `anyhow` for errors, `thiserror` for custom types
- Module re-exports via `mod.rs` files
- Doc comments on structs/functions, minimal inline comments
- Follow existing patterns when adding new controls or widgets

## Code Principles

### KISS and DRY
- **Keep it simple** — prefer straightforward code over clever abstractions
- **Don't repeat yourself** — extract shared logic into reusable functions/structs
- **No dead code** — remove unused structs, methods, and imports. If it's not called, delete it
- **No premature generalization** — build what's needed now, refactor when a pattern emerges

### Reuse existing components
- **UI widgets**: Use existing widgets in `ui/widgets.rs` (Fader, LevelMeter, Crossfader, DeckIndicator) before creating new ones
- **State patterns**: Follow `ChannelControl`/`GlobalControl` enum patterns for new controls
- **DSP**: Use existing `Biquad` in `audio/sample_cache.rs` for filtering, not custom implementations
- **Navigation**: Extend existing `navigate_control_*` methods rather than adding parallel nav systems

### Modular structure
- **One responsibility per module** — `state/` for data, `ui/` for rendering, `audio/` for processing
- **State exports via `mod.rs`** — keep re-exports clean, avoid deep imports
- **Widgets are self-contained** — each widget in `ui/` handles its own rendering and area calculation
- **New panes**: Follow the checklist in "Adding new panes" above

### Cross-platform compatibility
- **Keep it cross-platform** — the TUI should work on macOS, Linux, and Windows without platform-specific code in core paths
- **No OS-level input interception** — media keys, global hotkeys, and other OS-grabbed keys are not portable. Use standard keyboard keys that all terminal emulators pass through (letters, numbers, function keys, arrows)
- **No platform-specific dependencies in core** — if a feature requires OS-specific APIs (e.g., macOS `MediaRemote.framework`, Linux `evdev`), gate it behind `#[cfg(target_os)]` with a fallback, or avoid it entirely
- **Terminal emulators vary** — not all terminals pass the same escape sequences. Stick to common keys (F1-F12, letters, modifiers) rather than relying on extended key codes

## UI Conventions

### Colors — always use `src/ui/colors.rs`
All colors are centralized in `src/ui/colors.rs`. **Never** hardcode `Color::Rgb(...)` or named colors directly in widget code. Import from the module instead:

```rust
use crate::ui::colors::*;
```

Color groups:
- **Borders**: `BORDER_DEFAULT`, `BORDER_NAVIGATED`, `BORDER_ACTIVE`
- **Text**: `TEXT_DEFAULT`, `TEXT_DIM`, `TEXT_BRIGHT`, `TEXT_GHOST`, `TEXT_EDITING`
- **Deck accents**: `DECK_A`/`DECK_A_BRIGHT`, `DECK_B`/`DECK_B_BRIGHT`, `DECK_C`
- **Backgrounds**: `BG_DARK`, `BG_DEFAULT`, `BG_LIGHT`
- **Status**: `STATUS_PLAYING`, `STATUS_MUTED`
- **Meters/faders**: `METER_TRACK`, `METER_FILL`, `FADER_FILL`
- **Separators**: `SEPARATOR` (alias for `BORDER_DEFAULT`)
- **Sampler**: `PAD_ACTIVE_LOW`, `PAD_ACTIVE_HIGH`
- **Buttons**: `BTN_DM_PURPLE`
- **Slider**: `SLIDER_MID`
- **Hints**: `HINT_DEFAULT`

To add a new color: define it in `colors.rs` with a descriptive `SCREAMING_SNAKE` name, then use it everywhere.

### Borders and separators
The TUI uses a table-cell aesthetic with connected borders. Two approaches:

#### 1. Automatic merging (preferred for adjacent panes)
Ratatui v0.30+ has built-in border collapsing. Use this when placing blocks adjacent to each other:

```rust
use ratatui::{layout::{Layout, Constraint, Spacing}, symbols::merge::MergeStrategy, widgets::Block};

let [left, right] = Layout::horizontal([Constraint::Length(20); 2])
    .spacing(Spacing::Overlap(1))  // borders share the same cell
    .areas(area);

let block = Block::bordered()
    .merge_borders(MergeStrategy::Exact)  // auto-merge junction characters
    .title("My Pane");
```

This automatically produces correct junctions (├, ┤, ┬, ┴, ┼) where borders meet. **Use this for inter-pane borders** rather than hand-drawing junction characters.

#### 2. Manual separators (for internal controls)
For drawing separator lines *inside* a pane (e.g., between EQ rows, between M/S/C buttons), use the `draw_separator()` pattern from `ChannelStrip`:

```rust
// Horizontal line with junction characters connecting to pane borders
fn draw_separator(&self, area: Rect, buf: &mut Buffer) {
    let style = Style::default().fg(self.border_color());
    let y = area.y + area.height.saturating_sub(1);
    if area.x > 0 {
        buf.set_string(area.x - 1, y, "├", style);  // left junction
    }
    for x in area.x..area.x + area.width {
        buf.set_string(x, y, "─", style);
    }
    buf.set_string(area.x + area.width, y, "┤", style);  // right junction
}
```

For vertical separators between elements (like M/S/C buttons), use `┼` to create cross-junctions.
