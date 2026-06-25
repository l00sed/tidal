//! Audio module

pub mod bpm;
mod discovery;
pub mod effects;
mod mpv;
pub mod output;
mod sample_cache;
mod source;
pub mod supercollider;

pub use bpm::{BpmAnalyzer, BpmResult};
pub use discovery::{SourceDiscovery, SourceType};
pub use mpv::MpvClient;
pub use output::AudioOutput;
pub use sample_cache::{RackPlayer, SampleEngine};
pub use source::{AudioSource, AudioSourceManager};
pub use supercollider::SuperColliderClient;
