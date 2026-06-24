//! Synchronous MPV IPC client

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Synchronous MPV IPC client
pub struct MpvClient {
    reader: Option<BufReader<UnixStream>>,
    socket_path: String,
    request_id: u64,
}

impl MpvClient {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            reader: None,
            socket_path: socket_path.into(),
            request_id: 0,
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

    fn send_command(&mut self, command: Vec<serde_json::Value>) -> Result<Option<serde_json::Value>, String> {
        let reader = self.reader.as_mut().ok_or("Not connected")?;

        self.request_id += 1;
        let cmd = serde_json::json!({
            "command": command,
            "request_id": self.request_id
        });

        let mut cmd_str = cmd.to_string();
        cmd_str.push('\n');

        // Write via the inner stream (BufReader wraps the stream)
        reader.get_mut().write_all(cmd_str.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        reader.get_mut().flush()
            .map_err(|e| format!("Flush failed: {}", e))?;

        // Read response (may need multiple attempts due to async events from MPV)
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

    /// Set a property
    pub fn set_property(&mut self, property: &str, value: serde_json::Value) -> Result<(), String> {
        self.send_command(vec![
            "set_property".into(),
            property.into(),
            value,
        ])?;
        Ok(())
    }

    /// Get a property
    pub fn get_property(&mut self, property: &str) -> Result<serde_json::Value, String> {
        let result = self.send_command(vec![
            "get_property".into(),
            property.into(),
        ])?;
        result.ok_or_else(|| "No data returned".to_string())
    }

    /// Set volume (0-100)
    pub fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.set_property("volume", serde_json::json!(volume.clamp(0.0, 100.0)))
    }

    /// Set mute
    pub fn set_mute(&mut self, muted: bool) -> Result<(), String> {
        self.set_property("mute", serde_json::json!(muted))
    }

    /// Get current volume
    pub fn get_volume(&mut self) -> Result<f32, String> {
        let val = self.get_property("volume")?;
        val.as_f64().map(|v| v as f32).ok_or("Invalid volume value".to_string())
    }

    /// Get pause state
    pub fn get_pause(&mut self) -> Result<bool, String> {
        let val = self.get_property("pause")?;
        val.as_bool().ok_or("Invalid pause value".to_string())
    }

    /// Set pause state
    pub fn set_pause(&mut self, paused: bool) -> Result<(), String> {
        self.set_property("pause", serde_json::json!(paused))
    }
    
    /// Set playback speed (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
    pub fn set_speed(&mut self, speed: f32) -> Result<(), String> {
        self.set_property("speed", serde_json::json!(speed))
    }
    
    /// Set just the low-pass filter
    pub fn set_lpf(&mut self, freq: f32) -> Result<(), String> {
        if freq >= 19000.0 {
            self.send_command(vec!["af".into(), "del".into(), "@lpf".into()]).ok();
        } else {
            let filter = format!("@lpf:lavfi=[lowpass=f={:.0}]", freq);
            self.send_command(vec!["af".into(), "del".into(), "@lpf".into()]).ok();
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }
        Ok(())
    }
    
    /// Set just the high-pass filter  
    pub fn set_hpf(&mut self, freq: f32) -> Result<(), String> {
        if freq <= 25.0 {
            self.send_command(vec!["af".into(), "del".into(), "@hpf".into()]).ok();
        } else {
            let filter = format!("@hpf:lavfi=[highpass=f={:.0}]", freq);
            self.send_command(vec!["af".into(), "del".into(), "@hpf".into()]).ok();
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }
        Ok(())
    }
    
    /// Set EQ (bass/mid/treble adjustment using equalizer filter)
    pub fn set_eq(&mut self, low: f32, mid: f32, high: f32) -> Result<(), String> {
        let mut filters = Vec::new();
        
        // Only add filters that are non-zero
        if low.abs() > 0.1 {
            filters.push(format!("bass=g={:.1}:f=100", low));
        }
        
        if mid.abs() > 0.1 {
            filters.push(format!("equalizer=f=1000:t=h:w=500:g={:.1}", mid));
        }
        
        if high.abs() > 0.1 {
            filters.push(format!("treble=g={:.1}:f=3000", high));
        }
        
        // Build the complete filter string
        let filter_value = if filters.is_empty() {
            // No EQ active - clear all filters
            String::new()
        } else {
            // Wrap in lavfi
            format!("lavfi=[{}]", filters.join(","))
        };
        
        // Set the af property directly (replaces all audio filters)
        self.set_property("af", serde_json::json!(filter_value))?;
        
        Ok(())
    }
    /// Set pan/balance (-1.0 = full left, 0.0 = center, 1.0 = full right)
    pub fn set_pan(&mut self, pan: f32) -> Result<(), String> {
        let pan_clamped = pan.clamp(-1.0, 1.0);
        
        if pan_clamped.abs() < 0.01 {
            self.send_command(vec!["af".into(), "del".into(), "@pan".into()]).ok();
        } else {
            let filter = format!("@pan:lavfi=[stereotools=balance_out={:.2}]", pan_clamped);
            self.send_command(vec!["af".into(), "del".into(), "@pan".into()]).ok();
            self.send_command(vec!["af".into(), "add".into(), filter.into()])?;
        }
        Ok(())
    }

    /// Get the path of the currently playing file
    pub fn get_path(&mut self) -> Result<String, String> {
        let val = self.get_property("path")?;
        val.as_str().map(|s| s.to_string()).ok_or("Invalid path value".to_string())
    }
}
