//! SuperCollider OSC client with SynthDef-based audio routing
//!
//! Instead of controlling SC's global output, we create a SynthDef that wraps
//! the audio chain with per-deck volume, LPF, HPF, EQ, and pan controls.
//! Tidal's synths route through this chain before reaching the speakers.

use std::net::UdpSocket;
use std::time::Duration;

/// Synchronous SuperCollider OSC client with SynthDef support
pub struct SuperColliderClient {
    socket: Option<UdpSocket>,
    addr: String,
    connected: bool,
    base_node_id: i32,
    synth_def_sent: bool,
    group_id: Option<i32>,
    synth_id: Option<i32>,
    monitor_synth_id: Option<i32>,
}

impl SuperColliderClient {
    pub fn new(addr: impl Into<String>, base_node_id: i32) -> Self {
        Self {
            socket: None,
            addr: addr.into(),
            connected: false,
            base_node_id,
            synth_def_sent: false,
            group_id: None,
            synth_id: None,
            monitor_synth_id: None,
        }
    }

    pub fn connect(&mut self) -> Result<(), String> {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("Failed to bind UDP socket: {}", e))?;
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok();
        socket
            .set_write_timeout(Some(Duration::from_millis(100)))
            .ok();
        self.socket = Some(socket);
        self.connected = true;
        Ok(())
    }

    fn send_raw(&self, msg: &[u8]) -> Result<(), String> {
        let socket = self.socket.as_ref().ok_or("Not connected")?;
        socket
            .send_to(msg, &self.addr)
            .map_err(|e| format!("Send failed: {}", e))?;
        Ok(())
    }

    /// Send the mixerChannel SynthDef to SC
    ///
    /// The SynthDef reads stereo audio from a bus, applies EQ, LPF, HPF,
    /// volume, and pan, then outputs to the hardware.
    ///
    /// After sending, call `create_monitor_synth()`, `create_group()`, and `create_synth()` to activate.
    pub fn send_synth_def(&mut self) -> Result<(), String> {
        // Send both SynthDefs: monitorChannel (bus 0→2) and mixerChannel (bus 2→0 with effects)
        let synthdef_files = ["monitorChannel.scsyndef", "mixerChannel.scsyndef"];
        
        for filename in &synthdef_files {
            let paths = [
                format!("synthdefs/{}", filename),
                format!("../synthdefs/{}", filename),
                format!("../../synthdefs/{}", filename),
            ];
            
            let def_bytes = paths.iter()
                .find_map(|p| std::fs::read(p).ok())
                .ok_or_else(|| format!("{}.scsyndef not found. Run: sclang synthdefs/mixerChannel.scd", filename))?;
            
            let mut msg = Vec::new();
            Self::write_osc_string(&mut msg, "/d_recv");
            Self::write_osc_string(&mut msg, ",b");
            let len = def_bytes.len() as u32;
            msg.extend_from_slice(&len.to_be_bytes());
            msg.extend_from_slice(&def_bytes);
            while msg.len() % 4 != 0 {
                msg.push(0);
            }
            self.send_raw(&msg)?;
        }
        
        self.synth_def_sent = true;
        Ok(())
    }

    /// Create a group for this deck's effects chain
    /// The group is placed at the tail of the default group (node 0)
    pub fn create_group(&mut self) -> Result<(), String> {
        let group_id = self.base_node_id + 100;
        self.group_id = Some(group_id);

        let msg = Self::encode_osc_message(
            "/g_new",
            &[
                OscType::Int(group_id),
                OscType::Int(1), // addToTail
                OscType::Int(0), // target: default group
            ],
        );
        self.send_raw(&msg)
    }

    /// Create the mixer synth in this deck's group
    /// Reads from bus 0 (hardware in), processes, outputs to bus 0 (hardware out)
    pub fn create_synth(&mut self) -> Result<(), String> {
        let group_id = self.group_id.unwrap_or(0);
        let synth_id = self.base_node_id + 200;
        self.synth_id = Some(synth_id);

        let msg = Self::encode_osc_message(
            "/s_new",
            &[
                OscType::Str("mixerChannel".to_string()),
                OscType::Int(synth_id),
                OscType::Int(1),  // addToTail
                OscType::Int(group_id), // target: our group
                OscType::Str("in".to_string()),
                OscType::Int(2),  // read from bus 2 (SuperDirt output)
                OscType::Str("out".to_string()),
                OscType::Int(0),  // write to bus 0
                OscType::Str("vol".to_string()),
                OscType::Float(0.8),
                OscType::Str("lpf".to_string()),
                OscType::Float(20000.0),
                OscType::Str("hpf".to_string()),
                OscType::Float(20.0),
                OscType::Str("eqLow".to_string()),
                OscType::Float(0.0),
                OscType::Str("eqMid".to_string()),
                OscType::Float(0.0),
                OscType::Str("eqHigh".to_string()),
                OscType::Float(0.0),
                OscType::Str("pan".to_string()),
                OscType::Float(0.0),
            ],
        );
        self.send_raw(&msg)
    }

    /// Create a monitoring synth that routes SuperDirt output (bus 0) to mixer input (bus 2)
    /// This allows the mixer to process audio without modifying SuperDirt's config
    pub fn create_monitor_synth(&mut self) -> Result<(), String> {
        let monitor_id = self.base_node_id + 300;
        self.monitor_synth_id = Some(monitor_id);

        let msg = Self::encode_osc_message(
            "/s_new",
            &[
                OscType::Str("monitorChannel".to_string()),
                OscType::Int(monitor_id),
                OscType::Int(1),  // addToTail
                OscType::Int(0),  // target: default group
                OscType::Str("in".to_string()),
                OscType::Int(0),  // read from bus 0 (SuperDirt output)
                OscType::Str("out".to_string()),
                OscType::Int(2),  // write to bus 2 (mixer input)
            ],
        );
        self.send_raw(&msg)
    }

    /// Free the monitoring synth (restores direct SuperDirt → speakers routing)
    pub fn free_monitor_synth(&self) -> Result<(), String> {
        if let Some(monitor_id) = self.monitor_synth_id {
            let msg = Self::encode_osc_message("/n_free", &[OscType::Int(monitor_id)]);
            self.send_raw(&msg)
        } else {
            Ok(())
        }
    }

    /// Free the mixer synth (stops effect processing)
    pub fn free_synth(&self) -> Result<(), String> {
        if let Some(synth_id) = self.synth_id {
            let msg = Self::encode_osc_message("/n_free", &[OscType::Int(synth_id)]);
            self.send_raw(&msg)
        } else {
            Ok(())
        }
    }

    /// Free the group and all contained synths
    pub fn free_group(&self) -> Result<(), String> {
        if let Some(group_id) = self.group_id {
            let msg = Self::encode_osc_message("/n_free", &[OscType::Int(group_id)]);
            self.send_raw(&msg)
        } else {
            Ok(())
        }
    }

    /// Set a parameter on the mixer synth
    fn set_synth_param(&self, name: &str, value: f32) -> Result<(), String> {
        if let Some(synth_id) = self.synth_id {
            let msg = Self::encode_osc_message(
                "/n_set",
                &[
                    OscType::Int(synth_id),
                    OscType::Str(name.to_string()),
                    OscType::Float(value),
                ],
            );
            self.send_raw(&msg)
        } else {
            Ok(())
        }
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), String> {
        self.set_synth_param("vol", volume.clamp(0.0, 1.0))
    }

    /// Set low-pass filter frequency (20 - 20000 Hz)
    pub fn set_lpf(&self, freq: f32) -> Result<(), String> {
        self.set_synth_param("lpf", freq.clamp(20.0, 20000.0))
    }

    /// Set high-pass filter frequency (20 - 20000 Hz)
    pub fn set_hpf(&self, freq: f32) -> Result<(), String> {
        self.set_synth_param("hpf", freq.clamp(20.0, 20000.0))
    }

    /// Set EQ low band gain (-12 to +12 dB)
    pub fn set_eq_low(&self, db: f32) -> Result<(), String> {
        self.set_synth_param("eqLow", db.clamp(-12.0, 12.0))
    }

    /// Set EQ mid band gain (-12 to +12 dB)
    pub fn set_eq_mid(&self, db: f32) -> Result<(), String> {
        self.set_synth_param("eqMid", db.clamp(-12.0, 12.0))
    }

    /// Set EQ high band gain (-12 to +12 dB)
    pub fn set_eq_high(&self, db: f32) -> Result<(), String> {
        self.set_synth_param("eqHigh", db.clamp(-12.0, 12.0))
    }

    /// Set EQ (all three bands)
    pub fn set_eq(&self, low: f32, mid: f32, high: f32) -> Result<(), String> {
        self.set_eq_low(low)?;
        self.set_eq_mid(mid)?;
        self.set_eq_high(high)
    }

    /// Set pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub fn set_pan(&self, pan: f32) -> Result<(), String> {
        self.set_synth_param("pan", pan.clamp(-1.0, 1.0))
    }

    /// Set mute (0.0 = muted, restore volume on unmute)
    pub fn set_mute(&self, muted: bool) -> Result<(), String> {
        if muted {
            self.set_synth_param("vol", 0.0)
        } else {
            self.set_synth_param("vol", 0.8)
        }
    }

    /// Pause (free synth = muted, create new = unmuted)
    pub fn set_pause(&mut self, paused: bool) -> Result<(), String> {
        if paused {
            self.free_synth()
        } else {
            self.create_synth()
        }
    }

    /// Set playback speed (stub — SC tempo is controlled via TempoClock)
    pub fn set_speed(&self, _speed: f32) -> Result<(), String> {
        Ok(())
    }

    /// Free all nodes (cleanup)
    pub fn free_all(&self) -> Result<(), String> {
        self.free_monitor_synth()?;
        self.free_synth()?;
        self.free_group()
    }

    // ---- OSC encoding helpers ----

    fn encode_osc_message(address: &str, args: &[OscType]) -> Vec<u8> {
        let mut buf = Vec::new();
        Self::write_osc_string(&mut buf, address);
        let mut type_tags = String::from(",");
        for arg in args {
            match arg {
                OscType::Int(_) => type_tags.push('i'),
                OscType::Float(_) => type_tags.push('f'),
                OscType::Str(_) => type_tags.push('s'),
            }
        }
        Self::write_osc_string(&mut buf, &type_tags);
        for arg in args {
            match arg {
                OscType::Int(v) => buf.extend_from_slice(&v.to_be_bytes()),
                OscType::Float(v) => buf.extend_from_slice(&v.to_be_bytes()),
                OscType::Str(v) => Self::write_osc_string(&mut buf, v),
            }
        }
        buf
    }

    fn write_osc_string(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(s.as_bytes());
        buf.push(0);
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
    }

    fn write_pstring(buf: &mut Vec<u8>, s: &str) {
        buf.push(s.len() as u8);
        buf.extend_from_slice(s.as_bytes());
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
    }
}

enum OscType {
    Int(i32),
    Float(f32),
    Str(String),
}
