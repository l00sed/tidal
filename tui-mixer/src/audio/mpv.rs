//! Synchronous MPV IPC client with background audio metering
//!
//! The main connection handles commands (volume, EQ, filters, etc).
//! A background thread on a separate IPC connection polls `af-metadata`
//! at ~60Hz and stores peak/RMS in atomics — same pattern as the
//! ScreenCaptureKit capture for master meters.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::thread;

/// Lock-free meter storage — written by the polling thread, read by the UI.
/// Peaks use CAS to keep the highest value between UI ticks (same as master).
struct AtomicMeter {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
}

impl AtomicMeter {
    fn new() -> Self {
        Self {
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
            rms_l: AtomicU32::new(0),
            rms_r: AtomicU32::new(0),
        }
    }

    /// CAS loop: only update if new value is higher (peak hold).
    fn cas_peak(atom: &AtomicU32, new_val: f32) {
        let new_bits = new_val.to_bits();
        let mut current = atom.load(Ordering::Relaxed);
        loop {
            if new_bits <= current {
                return;
            }
            match atom.compare_exchange_weak(current, new_bits, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn store(&self, peak_l: f32, peak_r: f32, rms_l: f32, rms_r: f32) {
        Self::cas_peak(&self.peak_l, peak_l);
        Self::cas_peak(&self.peak_r, peak_r);
        self.rms_l.store(rms_l.to_bits(), Ordering::Relaxed);
        self.rms_r.store(rms_r.to_bits(), Ordering::Relaxed);
    }

    fn load(&self) -> (f32, f32, f32, f32) {
        (
            f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
            f32::from_bits(self.rms_l.load(Ordering::Relaxed)),
            f32::from_bits(self.rms_r.load(Ordering::Relaxed)),
        )
    }
}

/// Synchronous MPV IPC client
pub struct MpvClient {
    reader: Option<BufReader<UnixStream>>,
    socket_path: String,
    request_id: u64,
    meters: Arc<AtomicMeter>,
    stop_flag: Arc<AtomicBool>,
}

impl MpvClient {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            reader: None,
            socket_path: socket_path.into(),
            request_id: 0,
            meters: Arc::new(AtomicMeter::new()),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Connect to MPV socket
    pub fn connect(&mut self) -> Result<(), String> {
        let path = Path::new(&self.socket_path);
        if !path.exists() {
            return Err(format!("Socket not found: {}", self.socket_path));
        }

        match UnixStream::connect(path) {
            Ok(stream) => {
                stream.set_read_timeout(Some(Duration::from_millis(100))).ok();
                stream.set_write_timeout(Some(Duration::from_millis(100))).ok();
                self.reader = Some(BufReader::new(stream));
                Ok(())
            }
            Err(e) => Err(format!("Failed to connect: {}", e)),
        }
    }

    /// Start the background metering thread (call after `ensure_astats`).
    pub fn start_metering(&self) {
        let socket_path = self.socket_path.clone();
        let meters = self.meters.clone();
        let stop_flag = self.stop_flag.clone();

        thread::Builder::new()
            .name("mpv-meter".into())
            .spawn(move || {
                Self::metering_loop(&socket_path, meters, stop_flag);
            })
            .ok();
    }

    fn metering_loop(socket_path: &str, meters: Arc<AtomicMeter>, stop_flag: Arc<AtomicBool>) {
        let stream = match UnixStream::connect(Path::new(socket_path)) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Metering thread: failed to connect to {}: {}", socket_path, e);
                return;
            }
        };
        stream.set_read_timeout(Some(Duration::from_millis(50))).ok();
        stream.set_write_timeout(Some(Duration::from_millis(50))).ok();
        let mut reader = BufReader::new(stream);
        let mut req_id: u64 = 0;
        let mut line = String::new();

        while !stop_flag.load(Ordering::Relaxed) {
            req_id += 1;
            let cmd = serde_json::json!({
                "command": ["get_property", "af-metadata/astats"],
                "request_id": req_id
            });
            let mut cmd_str = cmd.to_string();
            cmd_str.push('\n');

            if reader.get_mut().write_all(cmd_str.as_bytes()).is_err() {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            let _ = reader.get_mut().flush();

            let mut got_response = false;
            for _ in 0..10 {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(resp) = serde_json::from_str::<serde_json::Value>(&line) {
                            if resp.get("request_id").and_then(|v| v.as_u64()) == Some(req_id) {
                                if let Some(data) = resp.get("data") {
                                    if let Some(obj) = data.as_object() {
                                        let get_dbfs = |key: &str| -> f32 {
                                            obj.get(key)
                                                .and_then(|v| v.as_str())
                                                .and_then(|s| s.parse::<f32>().ok())
                                                .unwrap_or(f32::NEG_INFINITY)
                                        };

                                        let peak_l_db = get_dbfs("lavfi.astats.1.Peak_level");
                                        let peak_r_db = get_dbfs("lavfi.astats.2.Peak_level");
                                        let rms_l_db = get_dbfs("lavfi.astats.1.RMS_level");
                                        let rms_r_db = get_dbfs("lavfi.astats.2.RMS_level");

                                        let dbfs_to_linear = |db: f32| -> f32 {
                                            if db <= f32::NEG_INFINITY {
                                                0.0
                                            } else {
                                                (10.0_f32).powf(db / 20.0).clamp(0.0, 1.0)
                                            }
                                        };

                                        meters.store(
                                            dbfs_to_linear(peak_l_db),
                                            dbfs_to_linear(peak_r_db),
                                            dbfs_to_linear(rms_l_db),
                                            dbfs_to_linear(rms_r_db),
                                        );
                                    }
                                }
                                got_response = true;
                                break;
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }

            if !got_response {
                let (pl, pr, rl, rr) = meters.load();
                meters.store(pl * 0.8, pr * 0.8, rl * 0.85, rr * 0.85);
            }

            thread::sleep(Duration::from_millis(8));
        }
    }

    fn send_command(&mut self, command: Vec<serde_json::Value>) -> Result<Option<serde_json::Value>, String> {
        let reader = self.reader.as_mut().ok_or("Not connected")?;

        self.request_id += 1;
        let cmd = serde_json::json!({
            "command": command,
            "request_id": self.request_id
        });

        let mut cmd_str = cmd.to_string();
        cmd_str.push('\n');

        reader.get_mut().write_all(cmd_str.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        reader.get_mut().flush()
            .map_err(|e| format!("Flush failed: {}", e))?;

        let mut line = String::new();

        for _ in 0..10 {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => return Err("Connection closed".to_string()),
                Ok(_) => {
                    if let Ok(resp) = serde_json::from_str::<serde_json::Value>(&line) {
                        if resp.get("request_id").and_then(|v| v.as_u64()) == Some(self.request_id) {
                            let error = resp.get("error").and_then(|v| v.as_str()).unwrap_or("success");
                            if error != "success" {
                                return Err(format!("MPV error: {}", error));
                            }
                            return Ok(resp.get("data").cloned());
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => return Err(format!("Read failed: {}", e)),
            }
        }

        Ok(None)
    }

    pub fn set_property(&mut self, property: &str, value: serde_json::Value) -> Result<(), String> {
        self.send_command(vec![
            "set_property".into(),
            property.into(),
            value,
        ])?;
        Ok(())
    }

    pub fn get_property(&mut self, property: &str) -> Result<serde_json::Value, String> {
        let result = self.send_command(vec![
            "get_property".into(),
            property.into(),
        ])?;
        result.ok_or_else(|| "No data returned".to_string())
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.set_property("volume", serde_json::json!(volume.clamp(0.0, 100.0)))
    }

    pub fn set_mute(&mut self, muted: bool) -> Result<(), String> {
        self.set_property("mute", serde_json::json!(muted))
    }

    pub fn get_volume(&mut self) -> Result<f32, String> {
        let val = self.get_property("volume")?;
        val.as_f64().map(|v| v as f32).ok_or("Invalid volume value".to_string())
    }

    pub fn get_pause(&mut self) -> Result<bool, String> {
        let val = self.get_property("pause")?;
        val.as_bool().ok_or("Invalid pause value".to_string())
    }

    pub fn set_pause(&mut self, paused: bool) -> Result<(), String> {
        self.set_property("pause", serde_json::json!(paused))
    }

    pub fn set_speed(&mut self, speed: f32) -> Result<(), String> {
        self.set_property("speed", serde_json::json!(speed))
    }

    fn has_filter(&mut self, label: &str) -> bool {
        if let Ok(af) = self.get_property("af") {
            if let Some(arr) = af.as_array() {
                return arr.iter().any(|f| f.get("label").and_then(|v| v.as_str()) == Some(label));
            }
        }
        false
    }

    pub fn set_lpf(&mut self, freq: f32) -> Result<(), String> {
        if freq >= 19000.0 {
            if self.has_filter("lpf") {
                self.send_command(vec!["af".into(), "remove".into(), "@lpf".into()]).ok();
            }
        } else {
            let filter = format!("@lpf:lavfi=[lowpass=f={:.0}]", freq);
            if self.has_filter("lpf") {
                self.send_command(vec!["af".into(), "remove".into(), "@lpf".into()]).ok();
            }
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }
        Ok(())
    }

    pub fn set_hpf(&mut self, freq: f32) -> Result<(), String> {
        if freq <= 25.0 {
            if self.has_filter("hpf") {
                self.send_command(vec!["af".into(), "remove".into(), "@hpf".into()]).ok();
            }
        } else {
            let filter = format!("@hpf:lavfi=[highpass=f={:.0}]", freq);
            if self.has_filter("hpf") {
                self.send_command(vec!["af".into(), "remove".into(), "@hpf".into()]).ok();
            }
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }
        Ok(())
    }

    pub fn set_eq(&mut self, low: f32, mid: f32, high: f32) -> Result<(), String> {
        if self.has_filter("eq") {
            self.send_command(vec!["af".into(), "remove".into(), "@eq".into()]).ok();
        }

        let mut filters = Vec::new();

        if low.abs() > 0.1 {
            filters.push(format!("bass=g={:.1}:f=100", low));
        }

        if mid.abs() > 0.1 {
            filters.push(format!("equalizer=f=1000:t=h:w=500:g={:.1}", mid));
        }

        if high.abs() > 0.1 {
            filters.push(format!("treble=g={:.1}:f=3000", high));
        }

        if !filters.is_empty() {
            let filter = format!("@eq:lavfi=[{}]", filters.join(","));
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }

        Ok(())
    }

    pub fn set_pan(&mut self, pan: f32) -> Result<(), String> {
        let pan_clamped = pan.clamp(-1.0, 1.0);

        if pan_clamped.abs() < 0.01 {
            if self.has_filter("pan") {
                self.send_command(vec!["af".into(), "remove".into(), "@pan".into()]).ok();
            }
        } else {
            let filter = format!("@pan:lavfi=[stereotools=balance_out={:.2}]", pan_clamped);
            if self.has_filter("pan") {
                self.send_command(vec!["af".into(), "remove".into(), "@pan".into()]).ok();
            }
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }
        Ok(())
    }

    pub fn get_path(&mut self) -> Result<String, String> {
        let val = self.get_property("path")?;
        val.as_str().map(|s| s.to_string()).ok_or("Invalid path value".to_string())
    }

    pub fn ensure_astats(&mut self) -> Result<(), String> {
        if let Ok(af) = self.get_property("af") {
            if let Some(arr) = af.as_array() {
                for filter in arr {
                    if filter.get("label").and_then(|v| v.as_str()) == Some("astats") {
                        return Ok(());
                    }
                }
            }
        }

        self.send_command(vec![
            "af".into(),
            "add".into(),
            "@astats:astats=metadata=1:reset=1:measure_overall=Peak_level+RMS_level:measure_perchannel=Peak_level+RMS_level".into(),
        ])?;
        Ok(())
    }

    /// Read audio levels from the background metering thread atomics.
    /// Returns `(peak_l, peak_r, rms_l, rms_r)` in linear 0.0–1.0 scale.
    pub fn get_audio_levels(&self) -> (f32, f32, f32, f32) {
        self.meters.load()
    }
}

impl Drop for MpvClient {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}
