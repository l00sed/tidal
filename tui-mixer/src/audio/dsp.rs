use std::f32::consts::TAU;

/// Biquad filter (Direct Form 1) with RBJ cookbook coefficient calculation.
/// Smooth parameter interpolation via per-buffer coefficient updates.
pub struct Biquad {
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
    x1: f32, x2: f32,
    y1: f32, y2: f32,
    sample_rate: f32,
}

#[derive(Clone, Copy)]
pub enum FilterType {
    LowPass,
    HighPass,
    Peaking,
}

impl Biquad {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            b0: 1.0, b1: 0.0, b2: 0.0,
            a1: 0.0, a2: 0.0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
            sample_rate,
        }
    }

    /// Set coefficients using RBJ cookbook formulas (pre-warped bilinear transform).
    pub fn set_params(&mut self, filter_type: FilterType, freq: f32, q: f32, gain_db: f32) {
        let fs = self.sample_rate;
        let freq = freq.clamp(1.0, fs * 0.49);
        let w0 = TAU * (freq / fs);
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.max(0.001));
        let a = 10.0_f32.powf(gain_db.clamp(-48.0, 48.0) / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match filter_type {
            FilterType::LowPass => {
                let b0 = (1.0 - cos_w0) / 2.0;
                let b1 = 1.0 - cos_w0;
                let b2 = (1.0 - cos_w0) / 2.0;
                (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterType::HighPass => {
                let b0 = (1.0 + cos_w0) / 2.0;
                let b1 = -(1.0 + cos_w0);
                let b2 = (1.0 + cos_w0) / 2.0;
                (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterType::Peaking => {
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a;
                (b0, b1, b2, 1.0 + alpha / a, -2.0 * cos_w0, 1.0 - alpha / a)
            }
        };

        let inv_a0 = 1.0 / a0;
        self.b0 = b0 * inv_a0;
        self.b1 = b1 * inv_a0;
        self.b2 = b2 * inv_a0;
        self.a1 = a1 * inv_a0;
        self.a2 = a2 * inv_a0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
              - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        y
    }
}

/// Per-sample LFO oscillator with cubic speed curve and shape morphing.
pub struct LfoOsc {
    pub phase: f32,
    sample_rate: f32,
    pub speed: f32,
    pub shape: f32,
    freq_hz: f32,
    prev_speed: f32,
    prev_output: f32,
}

impl LfoOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self { phase: 0.0, sample_rate, speed: 0.0, shape: 0.0, freq_hz: 0.0, prev_speed: 0.0, prev_output: 0.0 }
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
        if speed <= 0.001 {
            self.freq_hz = 0.0;
        } else {
            self.freq_hz = 0.05 + speed.powf(3.0) * 29.95;
            // Start at peak (phase 0.25) when LFO activates from idle
            if self.prev_speed <= 0.001 {
                self.phase = 0.25;
            }
        }
        self.prev_speed = speed;
    }

    pub fn set_shape(&mut self, shape: f32) {
        self.shape = shape.clamp(0.0, 1.0);
    }

    #[inline]
    pub fn tick(&mut self) -> Option<f32> {
        if self.freq_hz <= 0.001 {
            self.phase = 0.0;
            self.prev_output = 0.0;
            return None;
        }
        self.phase = (self.phase + self.freq_hz / self.sample_rate) % 1.0;
        let raw = (self.phase * TAU).sin();
        let sq = if raw > 0.0 { 1.0 } else { 0.0 };
        let sine = raw * 0.5 + 0.5;
        let out = sq * (1.0 - self.shape) + sine * self.shape;
        // One-pole smooth to prevent clicks on activation
        self.prev_output += (out - self.prev_output) * 0.01;
        Some(self.prev_output)
    }
}

/// Stereo panning using sine/cosine law.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = ((pan + 1.0) * 0.25 * TAU).clamp(0.0, TAU * 0.5);
    (angle.cos(), angle.sin())
}

/// Per-buffer level meter.
pub struct LevelMeter {
    peak_l: f32,
    peak_r: f32,
    rms_ll: f64,
    rms_rr: f64,
    count: usize,
}

impl LevelMeter {
    pub fn new() -> Self {
        Self { peak_l: 0.0, peak_r: 0.0, rms_ll: 0.0, rms_rr: 0.0, count: 0 }
    }

    pub fn push_stereo(&mut self, l: f32, r: f32) {
        self.peak_l = self.peak_l.max(l.abs());
        self.peak_r = self.peak_r.max(r.abs());
        self.rms_ll += (l as f64) * (l as f64);
        self.rms_rr += (r as f64) * (r as f64);
        self.count += 1;
    }

    pub fn read(&mut self) -> (f32, f32, f32, f32) {
        let n = self.count.max(1) as f64;
        let rms_l = (self.rms_ll / n).sqrt() as f32;
        let rms_r = (self.rms_rr / n).sqrt() as f32;
        let peak_l = self.peak_l;
        let peak_r = self.peak_r;
        self.peak_l = 0.0; self.peak_r = 0.0;
        self.rms_ll = 0.0; self.rms_rr = 0.0;
        self.count = 0;
        (peak_l, peak_r, rms_l, rms_r)
    }
}

/// Lock-free meter values for audio→UI thread handoff.
use std::sync::atomic::{AtomicU32, Ordering};

pub struct AtomicMeter {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
}

impl AtomicMeter {
    pub fn new() -> Self {
        Self {
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
            rms_l: AtomicU32::new(0),
            rms_r: AtomicU32::new(0),
        }
    }

    pub fn store(&self, peak_l: f32, peak_r: f32, rms_l: f32, rms_r: f32) {
        self.peak_l.store(peak_l.to_bits(), Ordering::Release);
        self.peak_r.store(peak_r.to_bits(), Ordering::Release);
        self.rms_l.store(rms_l.to_bits(), Ordering::Release);
        self.rms_r.store(rms_r.to_bits(), Ordering::Release);
    }

    pub fn load(&self) -> (f32, f32, f32, f32) {
        let peak_l = f32::from_bits(self.peak_l.load(Ordering::Acquire));
        let peak_r = f32::from_bits(self.peak_r.load(Ordering::Acquire));
        let rms_l = f32::from_bits(self.rms_l.load(Ordering::Acquire));
        let rms_r = f32::from_bits(self.rms_r.load(Ordering::Acquire));
        (peak_l, peak_r, rms_l, rms_r)
    }
}
