use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use stratum_dsp::{analyze_audio, AnalysisConfig};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Result of BPM detection for a single track
#[derive(Debug, Clone)]
pub struct BpmResult {
    pub bpm: f32,
    pub confidence: f32,
}

/// Background BPM analyzer that decodes audio files and detects tempo
pub struct BpmAnalyzer;

impl BpmAnalyzer {
    /// Analyze an audio file in a background thread. Calls `on_result` when done.
    pub fn analyze_file(path: &Path, on_result: Arc<Mutex<dyn Fn(BpmResult) + Send>>) {
        let path = path.to_path_buf();
        thread::spawn(move || {
            let result = Self::decode_and_analyze(&path);
            if let Ok(r) = result {
                if let Ok(cb) = on_result.lock() {
                    cb(r);
                }
            }
        });
    }

    fn decode_and_analyze(path: &Path) -> Result<BpmResult, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());

        let hint = Hint::new();
        let format_opts = FormatOptions::default();
        let meta_opts = MetadataOptions::default();
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &meta_opts)
            .map_err(|e| e.to_string())?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.sample_rate.is_some())
            .ok_or("No audio track found")?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100) as u32;

        let codec_opts = DecoderOptions::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &codec_opts)
            .map_err(|e| e.to_string())?;

        let mut samples: Vec<f32> = Vec::new();
        let max_samples = sample_rate as usize * 60; // 60 seconds max

        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break
                }
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    let spec = *audio_buf.spec();
                    let num_channels = spec.channels.count() as u32;
                    let mut sample_buf = SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
                    sample_buf.copy_interleaved_ref(audio_buf);

                    // Mix down to mono
                    let interleaved = sample_buf.samples();
                    for frame in interleaved.chunks(num_channels as usize) {
                        let mono_sample: f32 = frame.iter().sum::<f32>() / num_channels as f32;
                        samples.push(mono_sample);
                        if samples.len() >= max_samples {
                            break;
                        }
                    }
                }
                Err(_) => continue,
            }

            if samples.len() >= max_samples {
                break;
            }
        }

        if samples.is_empty() {
            return Err("No samples decoded".to_string());
        }

        let result =
            analyze_audio(&samples, sample_rate, AnalysisConfig::default()).map_err(|e| e.to_string())?;

        Ok(BpmResult {
            bpm: result.bpm as f32,
            confidence: result.bpm_confidence,
        })
    }
}
