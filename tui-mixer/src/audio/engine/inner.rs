use std::sync::RwLock;

/// Commands sent from the UI thread to the audio callback.
/// State changes (volume, EQ, filters, etc.) go directly through ControlState.
/// These commands are for thread-bound actions only.
#[derive(Debug)]
pub enum AudioCommand {
    LoadFile(usize, String),
    Play(usize),
    Pause(usize),
    Stop(usize),
    Seek(usize, f64),
    Quit,
}

/// Per-deck control state, read by the audio callback.
#[derive(Clone)]
pub struct DeckState {
    pub volume: f32,
    pub muted: bool,
    pub solo: bool,
    pub filter_cutoff: f32,
    pub filter_freq: f32,
    pub lfo_speed: f32,
    pub lfo_shape: f32,
    pub eq_low: f32,
    pub eq_mid: f32,
    pub eq_high: f32,
    pub eq_low_kill: bool,
    pub eq_mid_kill: bool,
    pub eq_high_kill: bool,
    pub pan: f32,
}

impl Default for DeckState {
    fn default() -> Self {
        Self {
            volume: 0.8, filter_cutoff: 0.0, filter_freq: 0.5,
            lfo_speed: 0.0, lfo_shape: 0.0,
            eq_low: 0.0, eq_mid: 0.0, eq_high: 0.0,
            eq_low_kill: false, eq_mid_kill: false, eq_high_kill: false,
            pan: 0.0, muted: false, solo: false,
        }
    }
}

#[derive(Clone)]
pub struct MasterState {
    pub fader: f32,
    pub muted: bool,
    pub crossfader: f32,
    pub solo_active: bool,
}

impl Default for MasterState {
    fn default() -> Self {
        Self { fader: 0.8, muted: false, crossfader: 0.5, solo_active: false }
    }
}

/// Thread-safe control state shared between UI and audio threads.
/// UI thread writes (via send_command), audio thread reads (in callback).
pub struct ControlState {
    inner: RwLock<ControlStateInner>,
}

#[derive(Clone, Default)]
struct ControlStateInner {
    decks: [DeckState; 3],
    master: MasterState,
}

impl ControlState {
    pub fn new() -> Self {
        Self { inner: RwLock::new(ControlStateInner::default()) }
    }

    /// Audio thread: read a snapshot of all control values.
    pub fn read(&self) -> ControlSnapshot {
        let inner = self.inner.read().unwrap();
        ControlSnapshot {
            decks: inner.decks.clone(),
            master: inner.master.clone(),
        }
    }

    // --- UI thread write helpers ---
    //
    // Each setter uses a read-first pattern: check if the value actually changed
    // before acquiring the write lock. This prevents write-lock starvation of the
    // audio callback's read lock during rapid UI updates (e.g. holding a key at min/max).

    pub fn set_volume(&self, ch: usize, v: f32) {
        let clamped = v.clamp(0.0, 1.5);
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if (d.volume - clamped).abs() <= f32::EPSILON { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) { d.volume = clamped; }
        }
    }
    pub fn set_muted(&self, ch: usize, m: bool) {
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) { if d.muted == m { return; } }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) { d.muted = m; }
        }
    }
    pub fn set_solo(&self, ch: usize, s: bool) {
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) { if d.solo == s { return; } }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) { d.solo = s; }
        }
    }
    pub fn set_filter_cutoff(&self, ch: usize, v: f32) {
        let clamped = v.clamp(0.0, 1.0);
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if (d.filter_cutoff - clamped).abs() <= f32::EPSILON { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) { d.filter_cutoff = clamped; }
        }
    }
    pub fn set_filter_freq(&self, ch: usize, v: f32) {
        let clamped = v.clamp(0.0, 1.0);
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if (d.filter_freq - clamped).abs() <= f32::EPSILON { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) { d.filter_freq = clamped; }
        }
    }
    pub fn set_lfo(&self, ch: usize, speed: f32, shape: f32) {
        let sp = speed.clamp(0.0, 1.0);
        let sh = shape.clamp(0.0, 1.0);
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if (d.lfo_speed - sp).abs() <= f32::EPSILON
                    && (d.lfo_shape - sh).abs() <= f32::EPSILON { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) {
                d.lfo_speed = sp; d.lfo_shape = sh;
            }
        }
    }
    pub fn set_eq(&self, ch: usize, lo: f32, mi: f32, hi: f32) {
        let l = lo.clamp(-24.0, 24.0);
        let m = mi.clamp(-24.0, 24.0);
        let h = hi.clamp(-24.0, 24.0);
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if (d.eq_low - l).abs() <= f32::EPSILON
                    && (d.eq_mid - m).abs() <= f32::EPSILON
                    && (d.eq_high - h).abs() <= f32::EPSILON { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) {
                d.eq_low = l; d.eq_mid = m; d.eq_high = h;
            }
        }
    }
    pub fn set_eq_kill(&self, ch: usize, lo: bool, mi: bool, hi: bool) {
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if d.eq_low_kill == lo && d.eq_mid_kill == mi && d.eq_high_kill == hi { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) {
                d.eq_low_kill = lo; d.eq_mid_kill = mi; d.eq_high_kill = hi;
            }
        }
    }
    pub fn set_pan(&self, ch: usize, v: f32) {
        let clamped = v.clamp(-1.0, 1.0);
        if let Ok(inner) = self.inner.read() {
            if let Some(d) = inner.decks.get(ch) {
                if (d.pan - clamped).abs() <= f32::EPSILON { return; }
            }
        }
        if let Ok(mut inner) = self.inner.write() {
            if let Some(d) = inner.decks.get_mut(ch) { d.pan = clamped; }
        }
    }
    pub fn set_master_fader(&self, v: f32) {
        let clamped = v.clamp(0.0, 1.0);
        if let Ok(inner) = self.inner.read() {
            if (inner.master.fader - clamped).abs() <= f32::EPSILON { return; }
        }
        if let Ok(mut inner) = self.inner.write() { inner.master.fader = clamped; }
    }
    pub fn set_master_muted(&self, m: bool) {
        if let Ok(inner) = self.inner.read() {
            if inner.master.muted == m { return; }
        }
        if let Ok(mut inner) = self.inner.write() { inner.master.muted = m; }
    }
    pub fn set_crossfader(&self, v: f32) {
        let clamped = v.clamp(0.0, 1.0);
        if let Ok(inner) = self.inner.read() {
            if (inner.master.crossfader - clamped).abs() <= f32::EPSILON { return; }
        }
        if let Ok(mut inner) = self.inner.write() { inner.master.crossfader = clamped; }
    }
    pub fn set_solo_active(&self, s: bool) {
        if let Ok(inner) = self.inner.read() {
            if inner.master.solo_active == s { return; }
        }
        if let Ok(mut inner) = self.inner.write() { inner.master.solo_active = s; }
    }
}

/// Snapshot of control state, read by the audio callback without holding a lock.
#[derive(Clone)]
pub struct ControlSnapshot {
    pub decks: [DeckState; 3],
    pub master: MasterState,
}

impl Default for ControlState {
    fn default() -> Self { Self::new() }
}
