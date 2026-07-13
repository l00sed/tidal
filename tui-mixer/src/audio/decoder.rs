use std::cell::UnsafeCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Lock-free f64 wrapper for time positions.
pub struct AtomicF64(AtomicU64);
impl AtomicF64 {
    pub fn new(v: f64) -> Self { Self(AtomicU64::new(v.to_bits())) }
    pub fn store(&self, v: f64) { self.0.store(v.to_bits(), Ordering::Release); }
    pub fn load(&self) -> f64 { f64::from_bits(self.0.load(Ordering::Acquire)) }
}
unsafe impl Sync for AtomicF64 {}

/// Lock-free SPSC ring buffer for decoded stereo interleaved samples.
///
/// Uses UnsafeCell for the buffer data — the atomic position counters
/// guarantee that the single writer and single reader never access the
/// same slot simultaneously, making this safe despite shared &self access.
pub struct AudioRingBuf {
    buf: UnsafeCell<Vec<f32>>,
    mask: usize,
    write_pos: AtomicU64,
    read_pos: AtomicU64,
}

impl AudioRingBuf {
    pub fn new(capacity_frames: usize) -> Self {
        let cap = (capacity_frames * 2).next_power_of_two();
        Self {
            buf: UnsafeCell::new(vec![0.0; cap]),
            mask: cap - 1,
            write_pos: AtomicU64::new(0),
            read_pos: AtomicU64::new(0),
        }
    }

    fn readable(&self) -> usize {
        (self.write_pos.load(Ordering::Acquire) - self.read_pos.load(Ordering::Acquire)) as usize & self.mask
    }

    fn writable(&self) -> usize {
        let cap = unsafe { (*self.buf.get()).len() };
        cap - self.readable()
    }

    pub fn write(&self, samples: &[f32]) -> usize {
        let to_write = samples.len().min(self.writable());
        if to_write == 0 { return 0; }
        let w = self.write_pos.load(Ordering::Relaxed) as usize & self.mask;
        let buf = unsafe { &mut *self.buf.get() };
        let cap = buf.len();
        let first = cap - w;
        if first >= to_write {
            buf[w..w + to_write].copy_from_slice(&samples[..to_write]);
        } else {
            buf[w..].copy_from_slice(&samples[..first]);
            buf[..to_write - first].copy_from_slice(&samples[first..to_write]);
        }
        self.write_pos.fetch_add(to_write as u64, Ordering::Release);
        to_write
    }

    pub fn read(&self, out: &mut [f32]) -> usize {
        let to_read = out.len().min(self.readable());
        if to_read == 0 { return 0; }
        let r = self.read_pos.load(Ordering::Relaxed) as usize & self.mask;
        let buf = unsafe { &*self.buf.get() };
        let cap = buf.len();
        let first = cap - r;
        if first >= to_read {
            out[..to_read].copy_from_slice(&buf[r..r + to_read]);
        } else {
            out[..first].copy_from_slice(&buf[r..]);
            out[first..to_read].copy_from_slice(&buf[..to_read - first]);
        }
        self.read_pos.fetch_add(to_read as u64, Ordering::Release);
        to_read
    }

    pub fn reset(&self) {
        self.write_pos.store(0, Ordering::Release);
        self.read_pos.store(0, Ordering::Release);
    }
}

unsafe impl Sync for AudioRingBuf {}

/// Metadata about a loaded file.
pub struct DecodedInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: Option<f64>,
}

/// Background decoder thread: fills a ring buffer from symphonia.
pub struct DecoderThread {
    pub ring: Arc<AudioRingBuf>,
    pub info: Arc<DecodedInfo>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    seek_pos: Arc<AtomicF64>,
    _handle: Option<thread::JoinHandle<()>>,
}

impl DecoderThread {
    pub fn load(path: &Path) -> Result<Self, String> {
        let ring = Arc::new(AudioRingBuf::new(16384));
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(true));
        let seek_pos = Arc::new(AtomicF64::new(-1.0));

        let file = std::fs::File::open(path).map_err(|e| format!("{}", e))?;
        let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());

        let hint = Hint::new();
        let format_opts = FormatOptions::default();
        let meta_opts = MetadataOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &meta_opts)
            .map_err(|e| format!("Probe error: {}", e))?;

        let mut format = probed.format;
        let track = format.tracks().iter()
            .find(|t| t.codec_params.sample_rate.is_some())
            .ok_or_else(|| "No audio track".to_string())?;

        let codec_params = track.codec_params.clone();
        let track_id = track.id;
        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let duration_secs = codec_params.n_frames.map(|f| f as f64 / sample_rate as f64);

        let info = Arc::new(DecodedInfo { sample_rate, channels, duration_secs });

        let codec_opts = DecoderOptions::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&codec_params, &codec_opts)
            .map_err(|e| format!("Codec error: {}", e))?;

        let stop_inner = Arc::clone(&stop);
        let paused_inner = Arc::clone(&paused);
        let seek_inner = Arc::clone(&seek_pos);
        let ring_inner = Arc::clone(&ring);

        let handle = thread::Builder::new()
            .name("decoder".into())
            .spawn(move || {
                let mut decode_buf = Vec::with_capacity(8192);
                loop {
                    if stop_inner.load(Ordering::Acquire) { break; }

                    let seek = seek_inner.load();
                    if seek >= 0.0 {
                        let seek_to = symphonia::core::formats::SeekTo::Time {
                            track_id: Some(track_id),
                            time: symphonia::core::units::Time { seconds: seek as u64, frac: 0.0 },
                        };
                        let _ = format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to);
                        decoder.reset();
                        ring_inner.reset();
                        seek_inner.store(-1.0);
                    }

                    if paused_inner.load(Ordering::Acquire) {
                        thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }

                    let packet = match format.next_packet() {
                        Ok(p) => p,
                        Err(symphonia::core::errors::Error::IoError(ref e))
                            if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            thread::sleep(std::time::Duration::from_millis(10));
                            continue;
                        }
                        Err(_) => { break; }
                    };

                    if packet.track_id() != track_id { continue; }

                    let audio_buf = match decoder.decode(&packet) {
                        Ok(d) => d,
                        Err(_) => continue,
                    };

                    let spec = *audio_buf.spec();
                    let num_channels = spec.channels.count() as u16;
                    let num_frames = audio_buf.frames();
                    let total_samples = num_frames * num_channels as usize;

                    decode_buf.clear();
                    decode_buf.reserve(total_samples);

                    let mut sample_buf = SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
                    sample_buf.copy_interleaved_ref(audio_buf);
                    decode_buf.extend_from_slice(sample_buf.samples());

                    // Write into ring buffer (blocking write)
                    let mut written = 0;
                    while written < decode_buf.len() && !stop_inner.load(Ordering::Acquire) {
                        let n = ring_inner.write(&decode_buf[written..]);
                        written += n;
                        if n == 0 { thread::yield_now(); }
                    }
                }
            })
            .map_err(|e| format!("Thread error: {}", e))?;

        Ok(Self {
            ring, info, _handle: Some(handle),
            stop, paused, seek_pos,
        })
    }

    pub fn play(&self) { self.paused.store(false, Ordering::Release); }
    pub fn pause(&self) { self.paused.store(true, Ordering::Release); }
    pub fn seek(&self, secs: f64) { self.seek_pos.store(secs); }
}

impl Drop for DecoderThread {
    fn drop(&mut self) { self.stop.store(true, Ordering::Release); }
}
