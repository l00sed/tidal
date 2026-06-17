//! Audio module

pub mod capture;
mod discovery;
mod mpv;
mod sample_cache;
mod source;
pub mod supercollider;

pub use capture::{AudioCapture, DspParams};
pub use discovery::{DiscoveredSource, SourceDiscovery, SourceType};
pub use mpv::MpvClient;
pub use sample_cache::SampleEngine;
pub use source::{AudioSource, AudioSourceManager};
pub use supercollider::SuperColliderClient;
