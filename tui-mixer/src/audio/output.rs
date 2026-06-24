//! Audio output device enumeration and selection via cpal
//!
//! Tracks available output devices and the user's selection.
//! Actual audio routing is handled by MPV's `audio-device` property —
//! this module provides device discovery and selection state only.

use cpal::traits::{DeviceTrait, HostTrait};

/// Device type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DeviceType {
    Speakers,
    Headphones,
    Bluetooth,
    Usb,
    Hdmi,
    Unknown,
}

/// Audio output device with type detection
pub struct AudioDevice {
    pub name: String,
    pub device_type: DeviceType,
}

/// Audio output device manager (device list + selection state)
pub struct AudioOutput {
    /// Available output devices
    devices: Vec<AudioDevice>,
    /// Currently selected device name
    selected_device: Option<String>,
}

impl AudioOutput {
    /// Create a new audio output manager
    pub fn new() -> Self {
        let devices = Self::enumerate_devices();
        Self {
            devices,
            selected_device: None,
        }
    }

    /// Enumerate all output devices with type detection
    fn enumerate_devices() -> Vec<AudioDevice> {
        let host = cpal::default_host();
        let mut devices = Vec::new();

        tracing::debug!("Enumerating output devices on host: {:?}", host.id());

        if let Ok(output_devices) = host.output_devices() {
            for device in output_devices {
                let desc = match device.description() {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let name = desc.name().to_string();
                let device_type = Self::map_device_type(desc.device_type(), desc.interface_type());
                tracing::debug!("  Output device: {} ({:?}) [{}, {}]",
                    name, device_type, desc.device_type(), desc.interface_type());
                devices.push(AudioDevice {
                    name,
                    device_type,
                });
            }
        } else {
            tracing::error!("Failed to enumerate output devices");
        }

        devices
    }

    /// Map cpal's DeviceType + InterfaceType to our DeviceType
    fn map_device_type(cpal_type: cpal::DeviceType, interface: cpal::InterfaceType) -> DeviceType {
        match cpal_type {
            cpal::DeviceType::Headphones | cpal::DeviceType::Headset => DeviceType::Headphones,
            cpal::DeviceType::Speaker | cpal::DeviceType::Earpiece => {
                match interface {
                    cpal::InterfaceType::Bluetooth => DeviceType::Bluetooth,
                    cpal::InterfaceType::Usb => DeviceType::Usb,
                    cpal::InterfaceType::Hdmi => DeviceType::Hdmi,
                    _ => DeviceType::Speakers,
                }
            }
            _ => {
                match interface {
                    cpal::InterfaceType::Bluetooth => DeviceType::Bluetooth,
                    cpal::InterfaceType::Usb => DeviceType::Usb,
                    cpal::InterfaceType::Hdmi => DeviceType::Hdmi,
                    _ => DeviceType::Speakers,
                }
            }
        }
    }

    /// Get the list of available device names
    pub fn devices(&self) -> Vec<String> {
        self.devices.iter().map(|d| d.name.clone()).collect()
    }

    /// Get devices filtered by type
    #[allow(dead_code)]
    pub fn devices_by_type(&self, device_type: DeviceType) -> Vec<&AudioDevice> {
        self.devices.iter().filter(|d| d.device_type == device_type).collect()
    }

    /// Get the currently selected device name
    pub fn selected_device(&self) -> Option<&str> {
        self.selected_device.as_deref()
    }

    /// Record a device selection (actual routing is done by MPV)
    pub fn select_main_device(&mut self, name: &str) -> Result<(), String> {
        if !self.devices.iter().any(|d| d.name == name) {
            return Err(format!("Device '{}' not found", name));
        }
        self.selected_device = Some(name.to_string());
        Ok(())
    }

    /// Record a CUE device selection
    pub fn select_cue_device(&mut self, name: &str) -> Result<(), String> {
        if !self.devices.iter().any(|d| d.name == name) {
            return Err(format!("Device '{}' not found", name));
        }
        self.selected_device = Some(name.to_string());
        Ok(())
    }

    /// Refresh the device list
    pub fn refresh_devices(&mut self) {
        self.devices = Self::enumerate_devices();
    }
}

impl Default for AudioOutput {
    fn default() -> Self {
        Self::new()
    }
}
