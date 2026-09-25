use crate::stretch::Stretcher;
use crate::Error;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use crossbeam_channel::{bounded, Receiver, Sender};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::format::{input, Pixel, Sample};
use ffmpeg_next::media::Type;
use ffmpeg_next::software::resampling::Context as ResampleContext;
use ffmpeg_next::software::scaling::{context::Context as ScaleContext, flag::Flags};
use ffmpeg_next::util::frame::audio::Audio as FFmpegAudioFrame;
use ffmpeg_next::util::frame::video::Video as FFmpegFrame;
use ffmpeg_next::Rational;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Position in the media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Position {
    Time(Duration),
    Frame(u64),
}

impl From<Duration> for Position {
    fn from(t: Duration) -> Self {
        Position::Time(t)
    }
}

impl From<u64> for Position {
    fn from(f: u64) -> Self {
        Position::Frame(f)
    }
}

#[derive(Debug, Clone, Default)]
struct Frame {
    data: Vec<u8>,
    width: u32,
    height: u32,
    timestamp: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeResult {
    Video,
    /// Decoded, then thrown away because the frame buffer was full.
    VideoDropped,
    Audio,
    Eos,
}

struct AudioRingBuffer {
    buffer: Vec<f32>,
    capacity: usize,
    read_pos: usize,
    write_pos: usize,
    len: usize,
    /// Counts the clears, so the audio callback knows to drop what it holds.
    clears: u64,
}

impl AudioRingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity],
            capacity,
            read_pos: 0,
            write_pos: 0,
            len: 0,
            clears: 0,
        }
    }

    fn write(&mut self, samples: &[f32]) -> usize {
        let to_write = samples.len().min(self.capacity - self.len);
        if to_write == 0 {
            return 0;
        }

        let first_chunk = (self.capacity - self.write_pos).min(to_write);
        self.buffer[self.write_pos..self.write_pos + first_chunk]
            .copy_from_slice(&samples[..first_chunk]);

        if first_chunk < to_write {
            self.buffer[..to_write - first_chunk].copy_from_slice(&samples[first_chunk..to_write]);
        }

        self.write_pos = (self.write_pos + to_write) % self.capacity;
        self.len += to_write;
        to_write
    }

    fn read(&mut self, output: &mut [f32]) -> usize {
        let available = self.len.min(output.len());
        if available == 0 {
            output.fill(0.0);
            return 0;
        }

        let first_chunk = (self.capacity - self.read_pos).min(available);
        output[..first_chunk]
            .copy_from_slice(&self.buffer[self.read_pos..self.read_pos + first_chunk]);

        if first_chunk < available {
            output[first_chunk..available].copy_from_slice(&self.buffer[..available - first_chunk]);
        }

        self.read_pos = (self.read_pos + available) % self.capacity;
        self.len -= available;
        output[available..].fill(0.0);

        available
    }

    fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
        self.clears += 1;
    }
}

/// How long the audio clock may be still before wall time takes over. The audio
/// device delivers a block at a time, so shorter gaps are normal.
const AUDIO_STALL_GRACE: Duration = Duration::from_millis(120);

/// How far ahead of the clock the decoder works, at most.
const DECODE_LEAD: Duration = Duration::from_millis(400);

/// The share of the frame buffer's own span the lead may use. The buffer is a
/// frame count, so its span depends on the frame rate: 30 frames is 1.2 s at
/// 25 fps but 0.5 s at 60 fps. Leading by more than the buffer can hold fills
/// it, which stops audio being decoded, and the forced audio decode then throws
/// away the video frames it pulls in with it. The picture ends up a sparse
/// subset of the frames that were decoded.
const LEAD_SHARE_OF_BUFFER: f64 = 0.6;

/// Slack on the resampler's output frame, for the samples it held back earlier.
const RESAMPLE_MARGIN: usize = 512;

/// Frames kept in hand at all times. Below this the presenter stops skipping and
/// the loop goes back to buffering.
const BUFFER_FLOOR: usize = 3;

/// Late frames thrown away per iteration, at most.
const MAX_SKIP: u32 = 5;

/// How early a frame may be presented.
const PRESENT_TOLERANCE: f64 = 0.010;

/// How late a frame has to be before it is skipped instead of presented.
const SKIP_THRESHOLD: f64 = 0.050;

/// Packets decoded per loop, with no sleep, while an accurate seek decodes up
/// to its target.
const SEEK_BURST: usize = 32;

/// One decoder's last second, for `RUST_LOG=gpui_video=debug`. Playback faults
/// here are all about rates and buffer levels over time, and a single line a
/// second is what makes them legible.
struct LoopStats {
    iterations: u64,
    video_packets: u64,
    audio_packets: u64,
    presented: u64,
    skipped: u64,
    dropped: u64,
    at: Instant,
}

impl Default for LoopStats {
    fn default() -> Self {
        Self {
            iterations: 0,
            video_packets: 0,
            audio_packets: 0,
            presented: 0,
            skipped: 0,
            dropped: 0,
            at: Instant::now(),
        }
    }
}

impl LoopStats {
    fn count(&mut self, decoded: DecodeResult) {
        match decoded {
            DecodeResult::Video => self.video_packets += 1,
            DecodeResult::VideoDropped => {
                self.video_packets += 1;
                self.dropped += 1;
            }
            DecodeResult::Audio => self.audio_packets += 1,
            DecodeResult::Eos => {}
        }
    }

    fn report(&mut self, video_lead: Duration, audio_lead: Duration, vbuf_len: usize) {
        if self.at.elapsed() < Duration::from_secs(1) {
            return;
        }
        log::debug!(
            "loop {:5}  vpkt {:3}  apkt {:3}  present {:3}  skip {:3}  drop {:3}  vbuf {:3}  lead v {:5.0}ms a {:5.0}ms",
            self.iterations,
            self.video_packets,
            self.audio_packets,
            self.presented,
            self.skipped,
            self.dropped,
            vbuf_len,
            video_lead.as_secs_f64() * 1000.0,
            audio_lead.as_secs_f64() * 1000.0,
        );
        *self = Self::default();
    }
}

#[derive(Debug, Clone)]
pub struct VideoOptions {
    pub frame_buffer_capacity: usize,
    pub looping: bool,
    pub speed: f64,
    /// Frames decoded before playback starts. A small number starts faster.
    pub prebuffer_frames: usize,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            frame_buffer_capacity: 120,
            looping: false,
            speed: 1.0,
            prebuffer_frames: 5,
        }
    }
}

enum DecoderCommand {
    /// `accurate` decodes from the keyframe up to the target and drops what
    /// comes before it. Otherwise playback starts at the keyframe.
    Seek {
        timestamp: i64,
        accurate: bool,
    },
    Stop,
}

/// Everything the decoder thread, the audio callback and the handle all touch.
/// One `Arc` carries the lot, so a new worker needs a single clone.
struct Shared {
    /// The frame on screen now.
    frame: Mutex<Frame>,
    frame_buffer: Mutex<VecDeque<Frame>>,
    frame_buffer_capacity: AtomicUsize,
    /// Counts presented frames. The element repaints only when this changes.
    frame_seq: AtomicU64,

    audio_ring: Mutex<AudioRingBuffer>,
    audio_pts: Mutex<Duration>,
    samples_played: AtomicU64,
    /// Where `samples_played` counts from. A seek or a loop zeroes the sample
    /// count, so without this the audio clock jumps back to the start of the
    /// clip while the decoded frames still carry their real timestamps.
    clock_base: Mutex<Duration>,
    video_clock: Mutex<Duration>,
    last_clock_update: Mutex<Instant>,

    speed_bits: AtomicU64,
    looping: AtomicBool,
    is_paused: AtomicBool,
    is_eos: AtomicBool,
    alive: AtomicBool,
    /// From a seek until its first frame is presented. A paused element keeps
    /// asking for frames while this holds.
    seeking: AtomicBool,

    volume: Mutex<f64>,
    muted: AtomicBool,
}

impl Shared {
    fn new(options: &VideoOptions) -> Self {
        Self {
            frame: Mutex::new(Frame::default()),
            frame_buffer: Mutex::new(VecDeque::with_capacity(options.frame_buffer_capacity)),
            frame_buffer_capacity: AtomicUsize::new(options.frame_buffer_capacity),
            frame_seq: AtomicU64::new(0),

            audio_ring: Mutex::new(AudioRingBuffer::new(48000 * 2 * 2)),
            audio_pts: Mutex::new(Duration::ZERO),
            samples_played: AtomicU64::new(0),
            clock_base: Mutex::new(Duration::ZERO),
            video_clock: Mutex::new(Duration::ZERO),
            last_clock_update: Mutex::new(Instant::now()),

            speed_bits: AtomicU64::new(options.speed.to_bits()),
            looping: AtomicBool::new(options.looping),
            is_paused: AtomicBool::new(false),
            is_eos: AtomicBool::new(false),
            alive: AtomicBool::new(true),
            seeking: AtomicBool::new(false),

            volume: Mutex::new(0.5),
            muted: AtomicBool::new(false),
        }
    }

    fn speed(&self) -> f64 {
        f64::from_bits(self.speed_bits.load(Ordering::SeqCst))
    }

    fn paused(&self) -> bool {
        self.is_paused.load(Ordering::Acquire)
    }

    fn eos(&self) -> bool {
        self.is_eos.load(Ordering::Acquire)
    }

    fn set_eos(&self, eos: bool) {
        self.is_eos.store(eos, Ordering::Release);
    }

    fn present(&self, frame: Frame) {
        *self.frame.lock() = frame;
        self.frame_seq.fetch_add(1, Ordering::Release);
    }

    /// Drops everything buffered and restarts both clocks at `position`.
    fn reset_to(&self, position: Duration) {
        self.frame_buffer.lock().clear();
        self.audio_ring.lock().clear();
        self.samples_played.store(0, Ordering::SeqCst);
        *self.clock_base.lock() = position;
        *self.video_clock.lock() = position;
        *self.audio_pts.lock() = position;
        *self.last_clock_update.lock() = Instant::now();
    }
}

struct Internal {
    id: u64,
    width: i32,
    height: i32,
    framerate: f64,
    duration: Duration,
    has_audio: bool,

    shared: Arc<Shared>,
    command_tx: Sender<DecoderCommand>,

    display_override: Mutex<(Option<u32>, Option<u32>)>,

    decoder_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    _audio_stream: Option<Stream>,
}

impl std::fmt::Debug for Internal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Video")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("framerate", &self.framerate)
            .field("has_audio", &self.has_audio)
            .finish_non_exhaustive()
    }
}

impl Drop for Internal {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::SeqCst);
        let _ = self.command_tx.send(DecoderCommand::Stop);
        if let Some(worker) = self.decoder_thread.lock().take() {
            let _ = worker.join();
        }
    }
}

#[derive(Debug, Clone)]
pub struct Video(Arc<Internal>);

impl Video {
    pub fn new(uri: &url::Url) -> Result<Self, Error> {
        Self::new_with_options(uri, VideoOptions::default())
    }

    pub fn new_with_options(uri: &url::Url, options: VideoOptions) -> Result<Self, Error> {
        ffmpeg::init()
            .map_err(|_| Error::Io(std::io::Error::other("Failed to initialize FFmpeg")))?;

        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

        let path = if uri.scheme() == "file" {
            uri.to_file_path()
                .map_err(|_| Error::Uri)?
                .to_string_lossy()
                .into_owned()
        } else {
            uri.as_str().to_string()
        };

        let ictx = input(&path).map_err(|_| Error::Uri)?;
        let video_stream = ictx.streams().best(Type::Video).ok_or(Error::Caps)?;
        let video_stream_index = video_stream.index();
        let time_base = video_stream.time_base();

        let decoder = ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())
            .map_err(|_| Error::Caps)?
            .decoder()
            .video()
            .map_err(|_| Error::Cast)?;

        let width = decoder.width() as i32;
        let height = decoder.height() as i32;

        let avg_frame_rate = video_stream.avg_frame_rate();
        let framerate = if avg_frame_rate.numerator() > 0 && avg_frame_rate.denominator() > 0 {
            avg_frame_rate.numerator() as f64 / avg_frame_rate.denominator() as f64
        } else {
            25.0
        };
        if !framerate.is_finite() || framerate <= 0.0 {
            return Err(Error::Framerate(framerate));
        }

        let duration = if ictx.duration() > 0 {
            Duration::from_micros(ictx.duration() as u64)
        } else {
            Duration::ZERO
        };

        let audio_stream_index = ictx.streams().best(Type::Audio).map(|s| s.index());
        let has_audio = audio_stream_index.is_some();

        log::info!(
            "Video: {}x{} @ {:.2}fps, {:.2}s, audio: {}",
            width,
            height,
            framerate,
            duration.as_secs_f64(),
            has_audio
        );

        drop(ictx);

        let shared = Arc::new(Shared::new(&options));
        let (command_tx, command_rx) = bounded(100);

        let (audio_stream, output_sample_rate) = match has_audio {
            true => match Self::setup_audio_output(Arc::clone(&shared)) {
                Ok((stream, rate)) => (Some(stream), rate),
                Err(e) => {
                    log::warn!("Audio setup failed: {:?}", e);
                    (None, 48000)
                }
            },
            false => (None, 48000),
        };

        let worker = Worker {
            shared: Arc::clone(&shared),
            path,
            video_stream_index,
            audio_stream_index,
            video_time_base: time_base,
            framerate,
            output_sample_rate,
            prebuffer_frames: options.prebuffer_frames,
            command_rx,
        };
        let decoder_thread = std::thread::spawn(move || {
            if let Err(e) = worker.run() {
                log::error!("Decoder error: {:?}", e);
            }
        });

        Ok(Video(Arc::new(Internal {
            id,
            width,
            height,
            framerate,
            duration,
            has_audio,
            shared,
            command_tx,
            display_override: Mutex::new((None, None)),
            decoder_thread: Mutex::new(Some(decoder_thread)),
            _audio_stream: audio_stream,
        })))
    }

    fn setup_audio_output(shared: Arc<Shared>) -> Result<(Stream, u32), Error> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No audio output device",
            ))
        })?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;

        let sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels();
        log::info!("Audio: {} Hz, {} channels", sample_rate, channels);

        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // Away from 1x the sound is stretched to the speed at the same pitch.
        // The clock counts the 1x samples used, so it stays right at any speed.
        let mut stretcher = Stretcher::new(sample_rate);
        // What the stretcher took from the ring and did not play, for 1x.
        let mut unplayed: Vec<f32> = Vec::new();
        let mut clears = 0;
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if shared.paused() {
                        data.fill(0.0);
                        return;
                    }

                    let speed = shared.speed();
                    let mut ring = shared.audio_ring.lock();
                    if ring.clears != clears {
                        clears = ring.clears;
                        stretcher.reset();
                        unplayed.clear();
                    }
                    if speed == 1.0 && !stretcher.idle() {
                        let mut rest = stretcher.unplayed();
                        rest.append(&mut unplayed);
                        unplayed = rest;
                    }
                    let pull = |slice: &mut [f32]| {
                        let n = unplayed.len().min(slice.len());
                        slice[..n].copy_from_slice(&unplayed[..n]);
                        unplayed.drain(..n);
                        n + ring.read(&mut slice[n..])
                    };
                    let read_count = if speed == 1.0 {
                        let mut pull = pull;
                        pull(data)
                    } else {
                        stretcher.process(data, speed, pull) as usize
                    };
                    drop(ring);

                    if shared.muted.load(Ordering::Acquire) {
                        data.fill(0.0);
                    } else {
                        let gain = (*shared.volume.lock()).powi(2) as f32;
                        if (gain - 1.0).abs() > 0.001 {
                            for sample in data.iter_mut() {
                                *sample = (*sample * gain).clamp(-1.0, 1.0);
                            }
                        }
                    }

                    shared
                        .samples_played
                        .fetch_add(read_count as u64, Ordering::SeqCst);
                },
                |err| log::error!("Audio stream error: {:?}", err),
                None,
            )
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;

        stream
            .play()
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;

        Ok((stream, sample_rate))
    }

    /// Unique for the life of the handle. Anything that caches a frame must key
    /// on this as well as on `frame_seq`, because the sequence starts at zero
    /// again for every file.
    pub fn id(&self) -> u64 {
        self.0.id
    }

    pub fn has_audio(&self) -> bool {
        self.0.has_audio
    }

    pub fn size(&self) -> (i32, i32) {
        (self.0.width, self.0.height)
    }

    pub fn aspect_ratio(&self) -> f32 {
        let (w, h) = self.size();
        if w <= 0 || h <= 0 {
            return 1.0;
        }
        w as f32 / h as f32
    }

    pub fn set_display_width(&self, width: Option<u32>) {
        self.0.display_override.lock().0 = width;
    }

    pub fn set_display_height(&self, height: Option<u32>) {
        self.0.display_override.lock().1 = height;
    }

    pub fn set_display_size(&self, width: Option<u32>, height: Option<u32>) {
        *self.0.display_override.lock() = (width, height);
    }

    pub fn display_size(&self) -> (u32, u32) {
        let natural_w = self.0.width.max(0) as u32;
        let natural_h = self.0.height.max(0) as u32;
        let aspect = if natural_h == 0 {
            1.0
        } else {
            natural_w as f32 / natural_h as f32
        };

        match *self.0.display_override.lock() {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) if aspect > 0.0 => (w, (w as f32 / aspect).round() as u32),
            (Some(w), None) => (w, natural_h),
            (None, Some(h)) => ((h as f32 * aspect).round() as u32, h),
            (None, None) => (natural_w, natural_h),
        }
    }

    pub fn framerate(&self) -> f64 {
        self.0.framerate
    }

    pub fn duration(&self) -> Duration {
        self.0.duration
    }

    pub fn set_volume(&self, volume: f64) {
        *self.0.shared.volume.lock() = volume.clamp(0.0, 1.0);
    }

    pub fn volume(&self) -> f64 {
        *self.0.shared.volume.lock()
    }

    pub fn set_muted(&self, muted: bool) {
        self.0.shared.muted.store(muted, Ordering::SeqCst);
    }

    pub fn muted(&self) -> bool {
        self.0.shared.muted.load(Ordering::Acquire)
    }

    pub fn eos(&self) -> bool {
        self.0.shared.eos()
    }

    pub fn looping(&self) -> bool {
        self.0.shared.looping.load(Ordering::SeqCst)
    }

    pub fn set_looping(&self, looping: bool) {
        self.0.shared.looping.store(looping, Ordering::SeqCst);
    }

    pub fn paused(&self) -> bool {
        self.0.shared.paused()
    }

    pub fn set_paused(&self, paused: bool) {
        let shared = &self.0.shared;
        let was_paused = shared.is_paused.swap(paused, Ordering::SeqCst);

        if was_paused != paused {
            if !paused {
                *shared.last_clock_update.lock() = Instant::now();
            }
            log::info!("{} playback", if paused { "Pausing" } else { "Resuming" });
        }

        if !paused && shared.eos() {
            log::info!("Restarting from EOS");
            let _ = self.seek(0, false);
        }
    }

    pub fn speed(&self) -> f64 {
        self.0.shared.speed()
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), Error> {
        if speed <= 0.0 || speed > 16.0 {
            return Err(Error::Framerate(speed));
        }

        log::info!("Setting speed to {:.2}x", speed);
        self.0
            .shared
            .speed_bits
            .store(speed.to_bits(), Ordering::SeqCst);
        *self.0.shared.last_clock_update.lock() = Instant::now();
        Ok(())
    }

    /// Go to `position`. An accurate seek shows the frame at that time, and the
    /// sound starts there too. A seek that is not accurate starts at the
    /// keyframe before it, which is faster.
    pub fn seek(&self, position: impl Into<Position>, accurate: bool) -> Result<(), Error> {
        let seconds = match position.into() {
            Position::Time(duration) => duration.as_secs_f64(),
            Position::Frame(frame) => frame as f64 / self.0.framerate,
        }
        .clamp(0.0, self.0.duration.as_secs_f64());

        // `Input::seek` seeks the container, so the timestamp is in AV_TIME_BASE
        // units (microseconds), not the video stream's time base. Converting to
        // the stream time base landed the seek at a small fraction of the target.
        let timestamp = (seconds * 1_000_000.0) as i64;
        log::info!("Seeking to {:.2}s ({} us)", seconds, timestamp);

        self.0.shared.set_eos(false);
        self.0.shared.seeking.store(true, Ordering::Release);
        self.0.shared.reset_to(Duration::from_secs_f64(seconds));

        self.0
            .command_tx
            .send(DecoderCommand::Seek {
                timestamp,
                accurate,
            })
            .map_err(|_| Error::Sync)
    }

    /// A seek has not presented its first frame yet.
    pub fn seeking(&self) -> bool {
        self.0.shared.seeking.load(Ordering::Acquire)
    }

    pub fn restart_stream(&self) -> Result<(), Error> {
        self.0.shared.set_eos(false);
        self.set_paused(false);
        self.seek(0, false)
    }

    /// The master clock, as the decoder last published it. A paused video holds
    /// its position, because the decoder stops updating the clock while paused.
    pub fn position(&self) -> Duration {
        *self.0.shared.video_clock.lock()
    }

    /// The time of the frame on screen.
    pub fn frame_time(&self) -> Duration {
        self.0.shared.frame.lock().timestamp
    }

    /// Increments every time a new frame is presented. The element uses it to
    /// skip the colour conversion and the texture upload when nothing changed.
    pub fn frame_seq(&self) -> u64 {
        self.0.shared.frame_seq.load(Ordering::Acquire)
    }

    pub fn current_frame_data(&self) -> Option<(Vec<u8>, u32, u32)> {
        let frame = self.0.shared.frame.lock();
        (!frame.data.is_empty()).then(|| (frame.data.clone(), frame.width, frame.height))
    }

    pub fn set_frame_buffer_capacity(&self, capacity: usize) {
        let shared = &self.0.shared;
        shared
            .frame_buffer_capacity
            .store(capacity, Ordering::SeqCst);

        let mut buf = shared.frame_buffer.lock();
        while buf.len() > capacity {
            buf.pop_front();
        }
    }

    pub fn frame_buffer_capacity(&self) -> usize {
        self.0.shared.frame_buffer_capacity.load(Ordering::SeqCst)
    }

    pub fn pop_buffered_frame(&self) -> Option<(Vec<u8>, u32, u32)> {
        let frame = self.0.shared.frame_buffer.lock().pop_front()?;
        Some((frame.data, frame.width, frame.height))
    }

    pub fn buffered_len(&self) -> usize {
        self.0.shared.frame_buffer.lock().len()
    }

    pub fn audio_buffer_level(&self) -> f32 {
        let ring = self.0.shared.audio_ring.lock();
        ring.len as f32 / ring.capacity as f32
    }
}

/// The audio stream's decoder, resampler and time base, together because they
/// only ever exist as a set.
struct AudioTrack {
    stream_index: usize,
    time_base: Rational,
    decoder: ffmpeg::decoder::Audio,
    resampler: ResampleContext,
    output_sample_rate: u32,
}

impl AudioTrack {
    fn open(
        ictx: &Input,
        stream_index: usize,
        output_sample_rate: u32,
    ) -> Result<AudioTrack, Error> {
        let stream = ictx.stream(stream_index).ok_or(Error::Caps)?;
        let time_base = stream.time_base();

        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|_| Error::Caps)?
            .decoder()
            .audio()
            .map_err(|_| Error::Cast)?;

        let resampler = ResampleContext::get(
            decoder.format(),
            decoder.channel_layout(),
            decoder.rate(),
            Sample::F32(ffmpeg::format::sample::Type::Packed),
            ffmpeg::ChannelLayout::STEREO,
            output_sample_rate,
        )
        .map_err(|_| Error::Caps)?;

        Ok(AudioTrack {
            stream_index,
            time_base,
            decoder,
            resampler,
            output_sample_rate,
        })
    }

    /// Seconds of audio waiting in the ring.
    fn queued(&self, shared: &Shared) -> (Duration, usize) {
        let ring = shared.audio_ring.lock();
        let seconds = ring.len as f64 / (self.output_sample_rate as f64 * 2.0);
        (Duration::from_secs_f64(seconds), ring.capacity - ring.len)
    }

    /// Decodes one packet into the ring. With `from` set, the sound before that
    /// time is dropped, and `from` clears once the sound reaches it.
    fn decode(
        &mut self,
        packet: &ffmpeg::Packet,
        shared: &Shared,
        from: &mut Option<Duration>,
    ) -> DecodeResult {
        if let Err(e) = self.decoder.send_packet(packet) {
            log::warn!("Send audio packet error: {:?}", e);
            return DecodeResult::Audio;
        }

        let mut decoded = FFmpegAudioFrame::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let start = decoded.pts().map(|pts| to_duration(pts, self.time_base));
            if let Some(start) = start {
                *shared.audio_pts.lock() = start;
            }
            let mut skip = 0;
            if let (Some(target), Some(start)) = (*from, start) {
                let length = decoded.samples() as f64 / self.decoder.rate().max(1) as f64;
                let early = target.as_secs_f64() - start.as_secs_f64();
                if early >= length {
                    continue;
                }
                // Interleaved stereo, so two samples per frame of sound.
                skip = (early.max(0.0) * self.output_sample_rate as f64) as usize * 2;
            }
            *from = None;

            // `ResampleContext::run` allocates the output frame with the INPUT
            // sample count, so upsampling 44.1 kHz to a 48 kHz device leaves
            // about 9 % of every packet buffered inside the resampler, and
            // `flush` does not allocate at all. The audio came out steadily
            // shorter than the video, which starved the ring that drives the
            // clock and forced a decode that threw away the video frames it
            // pulled in. Sizing the output frame for the output rate converts it
            // in one go.
            let in_rate = self.decoder.rate().max(1) as u64;
            let room = (decoded.samples() as u64 * self.output_sample_rate as u64 / in_rate)
                as usize
                + RESAMPLE_MARGIN;

            let mut resampled = packed_stereo_frame(room);
            let mut delay = match self.resampler.run(&decoded, &mut resampled) {
                Ok(delay) => delay,
                Err(e) => {
                    log::error!("Resampling error: {:?}", e);
                    continue;
                }
            };
            queue_resampled(&resampled, shared, skip);

            while delay.is_some() {
                let mut tail = packed_stereo_frame(room);
                match self.resampler.flush(&mut tail) {
                    Ok(next) => delay = next,
                    Err(e) => {
                        log::error!("Resampler flush error: {:?}", e);
                        break;
                    }
                }
                if tail.samples() == 0 {
                    break;
                }
                queue_resampled(&tail, shared, 0);
            }

            return DecodeResult::Audio;
        }

        DecodeResult::Audio
    }
}

fn to_duration(pts: i64, time_base: Rational) -> Duration {
    let seconds = pts as f64 * time_base.numerator() as f64 / time_base.denominator() as f64;
    Duration::from_secs_f64(seconds.max(0.0))
}

/// An output frame with room for `samples` per channel, so the resampler has
/// somewhere to put the whole conversion.
fn packed_stereo_frame(samples: usize) -> FFmpegAudioFrame {
    FFmpegAudioFrame::new(
        Sample::F32(ffmpeg::format::sample::Type::Packed),
        samples,
        ffmpeg::ChannelLayout::STEREO,
    )
}

/// Interleaved f32 stereo out of a resampled frame, into the ring, without the
/// first `skip` samples.
fn queue_resampled(resampled: &FFmpegAudioFrame, shared: &Shared, skip: usize) {
    let sample_count = resampled.samples() * 2;
    if sample_count <= skip {
        return;
    }
    let samples: Vec<f32> = resampled
        .data(0)
        .chunks_exact(4)
        .take(sample_count)
        .skip(skip)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    shared.audio_ring.lock().write(&samples);
}

/// The decoder thread. It demuxes, decodes and paces both streams, and hands
/// presented frames to `shared`.
struct Worker {
    shared: Arc<Shared>,
    path: String,
    video_stream_index: usize,
    audio_stream_index: Option<usize>,
    video_time_base: Rational,
    framerate: f64,
    output_sample_rate: u32,
    prebuffer_frames: usize,
    command_rx: Receiver<DecoderCommand>,
}

impl Worker {
    fn run(self) -> Result<(), Error> {
        let mut ictx = input(&self.path).map_err(|_| Error::Uri)?;

        let video_stream = ictx.stream(self.video_stream_index).ok_or(Error::Caps)?;
        let mut decoder =
            ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())
                .map_err(|_| Error::Caps)?
                .decoder()
                .video()
                .map_err(|_| Error::Cast)?;
        decoder.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });

        let mut scaler = ScaleContext::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::NV12,
            decoder.width(),
            decoder.height(),
            Flags::LANCZOS,
        )
        .map_err(|_| Error::Caps)?;

        let mut audio = self
            .audio_stream_index
            .map(|index| AudioTrack::open(&ictx, index, self.output_sample_rate))
            .transpose()?;

        let mut state = LoopState::default();
        let shared = &*self.shared;
        log::info!("Decoder loop started");

        while shared.alive.load(Ordering::Acquire) {
            if self.handle_commands(&mut ictx, &mut decoder, &mut audio, &mut state) {
                return Ok(());
            }

            if shared.eos()
                && self.drained(audio.is_some())
                && !self.restart_loop(&mut ictx, &mut decoder, &mut audio, &mut state)
            {
                continue;
            }

            let paused = shared.paused();
            let vbuf_len = shared.frame_buffer.lock().len();

            if !state.buffering && !paused && vbuf_len < BUFFER_FLOOR && !shared.eos() {
                log::warn!(
                    "Buffer critically low ({} frames)! Rebuffering...",
                    vbuf_len
                );
                state.buffering = true;
                continue;
            }

            if state.buffering {
                if vbuf_len >= self.prebuffer_frames {
                    log::info!("Buffering done ({} frames)", vbuf_len);
                    state.buffering = false;
                    *shared.last_clock_update.lock() = Instant::now();
                    // Buffering after a seek can outlast the stall grace, and
                    // the clock must not carry that time.
                    state.audio_clock_at = Instant::now();
                }
                let seeking = state.video_seek.is_some();
                for _ in 0..if seeking { SEEK_BURST } else { 5 } {
                    if !self.decode_one(
                        &mut ictx,
                        &mut decoder,
                        &mut scaler,
                        audio.as_mut(),
                        &mut state,
                    ) {
                        break;
                    }
                }
                // A paused clip shows the frame of a seek at once. The rest of
                // the prebuffer is for playing.
                if paused && state.seek_unshown {
                    if let Some(first) = shared.frame_buffer.lock().front().cloned() {
                        shared.present(first);
                        state.seek_shown(shared);
                    }
                }
                if !seeking {
                    std::thread::sleep(Duration::from_millis(5));
                }
                continue;
            }

            if paused {
                // A thumbnail paused before the first frame was presented would
                // otherwise stay blank, because presenting only happens below.
                // The first frame after a seek is shown the same way.
                if shared.frame.lock().data.is_empty() || state.seek_unshown {
                    if let Some(first) = shared.frame_buffer.lock().front().cloned() {
                        shared.present(first);
                        state.seek_shown(shared);
                    }
                }
                state.was_paused = true;
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            if state.was_paused {
                // The stall detector below measures wall time, and the audio
                // device consumes nothing while paused, so an idle tile looks
                // exactly like a ring that ran dry. Without this the clock
                // carried the whole idle gap on the first frame after a resume
                // and the clip raced to its end.
                state.audio_clock_at = Instant::now();
                state.was_paused = false;
            }

            let master_clock = self.master_clock(audio.is_some(), &mut state);
            self.present_due(master_clock, &mut state);
            self.pace_decoding(
                &mut ictx,
                &mut decoder,
                &mut scaler,
                audio.as_mut(),
                master_clock,
                &mut state,
            );

            std::thread::sleep(Duration::from_millis(1));
        }

        Ok(())
    }

    /// Both buffers empty. EOS means the demuxer reached the end, not that
    /// playback did: the buffers hold up to a couple of seconds, so they play
    /// out first. Looping on EOS alone cut every clip short by the buffer depth.
    fn drained(&self, has_audio: bool) -> bool {
        self.shared.frame_buffer.lock().is_empty()
            && (!has_audio || self.shared.audio_ring.lock().len == 0)
    }

    /// Returns true when the decoder was told to stop.
    fn handle_commands(
        &self,
        ictx: &mut Input,
        decoder: &mut ffmpeg::decoder::Video,
        audio: &mut Option<AudioTrack>,
        state: &mut LoopState,
    ) -> bool {
        while let Ok(command) = self.command_rx.try_recv() {
            match command {
                DecoderCommand::Seek {
                    timestamp,
                    accurate,
                } => {
                    log::info!("Seek to {} us", timestamp);
                    if let Err(e) = ictx.seek(timestamp, ..timestamp) {
                        log::error!("Seek failed: {:?}", e);
                    }
                    flush(decoder, audio);
                    self.shared.set_eos(false);
                    // The caller cleared the buffers before sending this, but
                    // decoding carried on until the command was read, so the
                    // front of each buffer can hold pre-seek data.
                    self.shared.frame_buffer.lock().clear();
                    self.shared.audio_ring.lock().clear();
                    let target = *self.shared.clock_base.lock();
                    state.restart_at(target);
                    state.seek_unshown = true;
                    if accurate {
                        state.video_seek = Some(VideoSeek { target, held: None });
                        state.audio_from = audio.as_ref().map(|_| target);
                    }
                }
                DecoderCommand::Stop => {
                    log::info!("Stopping decoder");
                    return true;
                }
            }
        }
        false
    }

    /// Seeks back to the start when the clip loops. Returns false when it does
    /// not loop, and the caller then idles until something else happens.
    fn restart_loop(
        &self,
        ictx: &mut Input,
        decoder: &mut ffmpeg::decoder::Video,
        audio: &mut Option<AudioTrack>,
        state: &mut LoopState,
    ) -> bool {
        if !self.shared.looping.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(16));
            return false;
        }

        log::info!("Looping");
        if let Err(e) = ictx.seek(0, ..) {
            log::error!("Loop seek failed: {:?}", e);
        }
        flush(decoder, audio);
        self.shared.set_eos(false);
        self.shared.reset_to(Duration::ZERO);
        state.restart_at(Duration::ZERO);
        true
    }

    /// The clock everything is timed against: the audio device's own sample
    /// count when there is audio, wall time scaled by the speed when there is
    /// not.
    fn master_clock(&self, has_audio: bool, state: &mut LoopState) -> Duration {
        let shared = &*self.shared;

        let clock = if has_audio {
            let played = shared.samples_played.load(Ordering::SeqCst);
            let audio_clock = *shared.clock_base.lock()
                + Duration::from_secs_f64(played as f64 / (self.output_sample_rate as f64 * 2.0));

            if audio_clock > state.last_audio_clock {
                state.last_audio_clock = audio_clock;
                state.audio_clock_at = Instant::now();
            }

            // The audio device hands over a block at a time, so between
            // callbacks the clock is simply still. Only a gap longer than the
            // device's own period is a real stall: the ring running dry, or the
            // end of the stream, where a frozen clock would never reach the
            // timestamps of the frames still buffered and the clip would never
            // loop. Adding wall time before then makes the clock run ahead and
            // snap back on every callback, which reads as jitter and makes the
            // presenter skip and hold frames.
            match state
                .audio_clock_at
                .elapsed()
                .checked_sub(AUDIO_STALL_GRACE)
            {
                Some(over) => {
                    if over > Duration::from_millis(400) {
                        log::warn!("audio stall fallback carrying {:?}", over);
                    }
                    state.last_audio_clock + over
                }
                None => state.last_audio_clock,
            }
        } else {
            let elapsed = shared.last_clock_update.lock().elapsed();
            *shared.video_clock.lock()
                + Duration::from_secs_f64(elapsed.as_secs_f64() * shared.speed())
        };

        // A clock that steps backwards makes the presenter hold a frame it has
        // already shown. Seeking and looping reset `last_master` instead.
        let clock = clock.max(state.last_master);
        state.last_master = clock;

        // `position()` reports this, so it stays right for both clocks.
        *shared.video_clock.lock() = clock;
        *shared.last_clock_update.lock() = Instant::now();
        clock
    }

    /// Shows the frame the clock has reached, then throws away any frame that is
    /// already too late to show.
    fn present_due(&self, master_clock: Duration, state: &mut LoopState) {
        let shared = &*self.shared;
        let mut buf = shared.frame_buffer.lock();

        let Some(next) = buf.front() else {
            return;
        };
        let diff = master_clock.as_secs_f64() - next.timestamp.as_secs_f64();
        if diff < -PRESENT_TOLERANCE && buf.len() > BUFFER_FLOOR {
            return;
        }

        let presented = buf.pop_front().expect("front was Some");
        drop(buf);
        shared.present(presented);
        state.seek_shown(shared);
        let stats = &mut state.stats;
        stats.presented += 1;

        let mut buf = shared.frame_buffer.lock();
        let mut skipped = 0;
        while skipped < MAX_SKIP && buf.len() > BUFFER_FLOOR {
            let Some(front) = buf.front() else { break };
            let behind = master_clock.as_secs_f64() - front.timestamp.as_secs_f64();
            if behind <= SKIP_THRESHOLD {
                break;
            }
            buf.pop_front();
            stats.skipped += 1;
            skipped += 1;
            log::warn!(
                "Skipped late frame ({:.1}ms behind), {} remaining",
                behind * 1000.0,
                buf.len()
            );
        }
    }

    /// Decodes at most one packet, chosen by how far each stream runs ahead of
    /// the clock. A full video buffer must never stop audio being decoded: the
    /// audio clock is the master, so starving it stalls presentation, which
    /// stops the video buffer draining, which starves it further.
    fn pace_decoding(
        &self,
        ictx: &mut Input,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut ScaleContext,
        audio: Option<&mut AudioTrack>,
        master_clock: Duration,
        state: &mut LoopState,
    ) {
        let shared = &*self.shared;
        let vbuf_len = shared.frame_buffer.lock().len();
        let vbuf_cap = shared.frame_buffer_capacity.load(Ordering::SeqCst);

        let video_lead = shared
            .frame_buffer
            .lock()
            .back()
            .map(|f| f.timestamp.saturating_sub(master_clock))
            .unwrap_or_default();
        let (audio_lead, audio_space) = match audio.as_ref() {
            Some(track) => track.queued(shared),
            None => (Duration::MAX, 0),
        };

        state.stats.iterations += 1;

        // Both streams aim for the same lead, and it has to fit inside the frame
        // buffer. Below a third of it the clock is about to stall, so audio is
        // decoded whatever else is full.
        let buffer_span = Duration::from_secs_f64(vbuf_cap as f64 / self.framerate.max(1.0));
        let lead = DECODE_LEAD.min(buffer_span.mul_f64(LEAD_SHARE_OF_BUFFER));
        let has_audio = audio.is_some();

        let should_decode = if has_audio && audio_lead < lead / 3 {
            true
        } else {
            (video_lead < lead || audio_lead < lead)
                && vbuf_len < vbuf_cap
                && (!has_audio || audio_space > 0)
        };

        if should_decode {
            self.decode_one(ictx, decoder, scaler, audio, state);
        }

        state.stats.report(video_lead, audio_lead, vbuf_len);
    }

    /// Demuxes one packet and decodes it. Returns false at the end of the stream
    /// or on a decode error, so a buffering burst can stop early.
    fn decode_one(
        &self,
        ictx: &mut Input,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut ScaleContext,
        audio: Option<&mut AudioTrack>,
        state: &mut LoopState,
    ) -> bool {
        match self.decode_next_packet(ictx, decoder, scaler, audio, state) {
            Ok(DecodeResult::Eos) => {
                self.shared.set_eos(true);
                false
            }
            Ok(result) => {
                state.stats.count(result);
                true
            }
            Err(e) => {
                log::error!("Decode error: {:?}", e);
                false
            }
        }
    }

    fn decode_next_packet(
        &self,
        ictx: &mut Input,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut ScaleContext,
        audio: Option<&mut AudioTrack>,
        state: &mut LoopState,
    ) -> Result<DecodeResult, Error> {
        let Some((stream, packet)) = ictx.packets().next() else {
            // A target after the last frame. The last frame is on screen there.
            if let Some((at, frame)) = state.video_seek.take().and_then(|seek| seek.held) {
                self.buffer_frame(scaler, &frame, at);
            }
            state.audio_from = None;
            return Ok(DecodeResult::Eos);
        };
        let stream_index = stream.index();

        if stream_index == self.video_stream_index {
            return Ok(self.decode_video(decoder, scaler, &packet, &mut state.video_seek));
        }

        if let Some(track) = audio {
            if stream_index == track.stream_index {
                return Ok(track.decode(&packet, &self.shared, &mut state.audio_from));
            }
        }

        Ok(DecodeResult::Video)
    }

    /// Decodes one packet into the frame buffer. During an accurate seek, the
    /// frames before the target are dropped before they are scaled, except the
    /// last one: it is on screen at the target if the next frame starts after
    /// it. Frame times are not always evenly spaced, so only the next frame
    /// tells.
    fn decode_video(
        &self,
        decoder: &mut ffmpeg::decoder::Video,
        scaler: &mut ScaleContext,
        packet: &ffmpeg::Packet,
        seek: &mut Option<VideoSeek>,
    ) -> DecodeResult {
        if let Err(e) = decoder.send_packet(packet) {
            log::warn!("Send video packet error: {:?}", e);
            return DecodeResult::Video;
        }

        let mut decoded = FFmpegFrame::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            let timestamp = to_duration(decoded.pts().unwrap_or(0), self.video_time_base);
            if let Some(pending) = seek {
                if timestamp < pending.target {
                    let frame = std::mem::replace(&mut decoded, FFmpegFrame::empty());
                    pending.held = Some((timestamp, frame));
                    continue;
                }
                let exact = timestamp == pending.target;
                let held = seek.take().and_then(|pending| pending.held);
                if let Some((at, frame)) = held.filter(|_| !exact) {
                    self.buffer_frame(scaler, &frame, at);
                }
            }
            return self.buffer_frame(scaler, &decoded, timestamp);
        }

        DecodeResult::Video
    }

    fn buffer_frame(
        &self,
        scaler: &mut ScaleContext,
        decoded: &FFmpegFrame,
        timestamp: Duration,
    ) -> DecodeResult {
        let mut nv12 = FFmpegFrame::empty();
        if let Err(e) = scaler.run(decoded, &mut nv12) {
            log::error!("Scaling error: {:?}", e);
            return DecodeResult::Video;
        }

        let frame = Frame {
            data: pack_nv12(&nv12),
            width: nv12.width(),
            height: nv12.height(),
            timestamp,
        };

        let capacity = self.shared.frame_buffer_capacity.load(Ordering::SeqCst);
        if capacity == 0 {
            return DecodeResult::Video;
        }

        let mut buf = self.shared.frame_buffer.lock();
        // Drop the newest frame when full. Dropping the front would throw
        // away the frame that is about to be presented. The lead gate should
        // keep this from happening: a frame thrown away here leaves a gap in
        // the picture.
        if buf.len() >= capacity {
            return DecodeResult::VideoDropped;
        }
        buf.push_back(frame);
        DecodeResult::Video
    }
}

fn flush(decoder: &mut ffmpeg::decoder::Video, audio: &mut Option<AudioTrack>) {
    decoder.flush();
    if let Some(track) = audio {
        track.decoder.flush();
    }
}

/// Copies an NV12 frame into one tight buffer, row by row, because FFmpeg pads
/// each plane's stride.
fn pack_nv12(frame: &FFmpegFrame) -> Vec<u8> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let mut data = Vec::with_capacity(width * height * 3 / 2);

    for (plane, rows) in [(0, height), (1, height / 2)] {
        let source = frame.data(plane);
        let stride = frame.stride(plane);
        for row in 0..rows {
            let start = row * stride;
            data.extend_from_slice(&source[start..start + width]);
        }
    }

    data
}

/// An accurate seek that has not reached its frame yet.
struct VideoSeek {
    target: Duration,
    /// The last frame before the target so far, not yet scaled.
    held: Option<(Duration, FFmpegFrame)>,
}

/// The decoder loop's own running state, separate from what the handle sees.
struct LoopState {
    buffering: bool,
    was_paused: bool,
    /// Tracks whether the audio clock is still moving. See `master_clock`.
    last_audio_clock: Duration,
    audio_clock_at: Instant,
    last_master: Duration,
    stats: LoopStats,
    /// Where an accurate seek starts each stream. What comes before is dropped.
    video_seek: Option<VideoSeek>,
    audio_from: Option<Duration>,
    /// A seek was handled and none of its frames is presented yet. The first
    /// one is presented even while paused.
    seek_unshown: bool,
}

impl Default for LoopState {
    fn default() -> Self {
        Self {
            buffering: true,
            was_paused: false,
            last_audio_clock: Duration::ZERO,
            audio_clock_at: Instant::now(),
            last_master: Duration::ZERO,
            stats: LoopStats::default(),
            video_seek: None,
            audio_from: None,
            seek_unshown: false,
        }
    }
}

impl LoopState {
    fn restart_at(&mut self, position: Duration) {
        self.buffering = true;
        self.last_audio_clock = position;
        self.audio_clock_at = Instant::now();
        self.last_master = position;
        self.video_seek = None;
        self.audio_from = None;
    }

    fn seek_shown(&mut self, shared: &Shared) {
        if self.seek_unshown {
            self.seek_unshown = false;
            shared.seeking.store(false, Ordering::Release);
        }
    }
}
