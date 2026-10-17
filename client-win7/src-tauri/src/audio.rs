use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use wasapi::{
    get_default_device, get_default_device_for_role, initialize_mta, AudioCaptureClient,
    AudioClient, Direction, Handle, Role, SampleType, StreamMode, WaveFormat,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

pub const WINDOW_SECONDS: f64 = 30.0;
const EVIDENCE_RING_SECONDS: usize = 32;
const TARGET_EVIDENCE_RATE: u32 = 48_000;
const TARGET_CAPTURE_RATE: u32 = 44_100;
const TARGET_CAPTURE_CHANNELS: usize = 2;
const SYSTEM_PLAYBACK_THRESHOLD: f32 = 0.0045;

#[derive(Debug, Clone)]
pub struct MeterSnapshot {
    pub current_db: Option<f64>,
    pub average_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub progress: f32,
    pub capturing: bool,
    pub suspended: bool,
    pub ready: bool,
    pub aec_enabled: bool,
    pub system_playing: bool,
    pub error: Option<String>,
    pub source_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct WindowReport {
    pub avg_dbfs: f64,
    pub peak_dbfs: f64,
    pub min_dbfs: f64,
    pub frames: u64,
    pub duration_secs: f64,
}

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn high_pass(sample_rate: u32) -> Self {
        let rate = sample_rate.max(8_000) as f32;
        let frequency = 80.0_f32;
        let omega = 2.0 * std::f32::consts::PI * frequency / rate;
        let cos = omega.cos();
        let alpha = omega.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 + cos) / 2.0) / a0,
            b1: (-(1.0 + cos)) / a0,
            b2: ((1.0 + cos) / 2.0) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }
}

struct Denoiser {
    filter: Biquad,
    sample_rate: u32,
    noise_floor: f32,
    ready: bool,
    gate: f32,
}

impl Denoiser {
    fn new() -> Self {
        Self {
            filter: Biquad::high_pass(48_000),
            sample_rate: 48_000,
            noise_floor: 1.0e-5,
            ready: false,
            gate: 1.0,
        }
    }

    fn configure(&mut self, sample_rate: u32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.filter = Biquad::high_pass(sample_rate);
        }
    }

    fn block_gain(&mut self, block_rms: f32) -> f32 {
        if !self.ready {
            self.noise_floor = (block_rms * 0.5).max(1.0e-5);
            self.ready = true;
            self.gate = 1.0;
            return 1.0;
        }
        if block_rms < self.noise_floor {
            self.noise_floor = self.noise_floor * 0.98 + block_rms * 0.02;
        } else {
            self.noise_floor = self.noise_floor * 0.9995 + block_rms * 0.0005;
        }
        let floor = self.noise_floor.max(1.0e-5);
        let ratio = block_rms / floor;
        let target = if ratio <= 1.15 {
            0.32
        } else if ratio >= 2.2 {
            1.0
        } else {
            0.32 + (ratio - 1.15) / 1.05 * 0.68
        };
        self.gate = (self.gate * 0.55 + target * 0.45).clamp(0.0, 1.0);
        self.gate
    }
}

struct Accumulator {
    started: Instant,
    sum_sq: f64,
    frames: u64,
    peak: f32,
    min_block_dbfs: f64,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            sum_sq: 0.0,
            frames: 0,
            peak: 0.0,
            min_block_dbfs: f64::INFINITY,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

struct RingBuffer {
    samples: VecDeque<f32>,
}

pub struct AudioShared {
    capturing: AtomicBool,
    suspended: AtomicBool,
    ready: AtomicBool,
    aec_enabled: AtomicBool,
    system_peak: AtomicU32,
    instant_dbfs: AtomicU32,
    offset_db: AtomicU32,
    source_rate: AtomicU32,
    channels: AtomicU16,
    total_frames: AtomicU64,
    ring_limit: AtomicUsize,
    window: Mutex<Accumulator>,
    ring: Mutex<RingBuffer>,
    denoiser: Mutex<Denoiser>,
    error: Mutex<Option<String>>,
}

impl AudioShared {
    pub fn new(offset_db: f32) -> Arc<Self> {
        Arc::new(Self {
            capturing: AtomicBool::new(true),
            suspended: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            aec_enabled: AtomicBool::new(false),
            system_peak: AtomicU32::new(0f32.to_bits()),
            instant_dbfs: AtomicU32::new(f32::to_bits(-100.0)),
            offset_db: AtomicU32::new(f32::to_bits(offset_db)),
            source_rate: AtomicU32::new(0),
            channels: AtomicU16::new(0),
            total_frames: AtomicU64::new(0),
            ring_limit: AtomicUsize::new(0),
            window: Mutex::new(Accumulator::new()),
            ring: Mutex::new(RingBuffer {
                samples: VecDeque::with_capacity(48_000 * 4),
            }),
            denoiser: Mutex::new(Denoiser::new()),
            error: Mutex::new(None),
        })
    }

    pub fn set_offset(&self, offset_db: f32) {
        self.offset_db
            .store(f32::to_bits(offset_db), Ordering::Relaxed);
    }

    pub fn offset(&self) -> f32 {
        f32::from_bits(self.offset_db.load(Ordering::Relaxed))
    }

    pub fn set_capturing(&self, capturing: bool) {
        self.capturing.store(capturing, Ordering::SeqCst);
        if !capturing {
            self.reset_window_and_ring();
        }
    }

    pub fn capturing(&self) -> bool {
        self.capturing.load(Ordering::SeqCst)
    }

    pub fn set_suspended(&self, suspended: bool) {
        let changed = self.suspended.swap(suspended, Ordering::SeqCst) != suspended;
        if changed {
            self.reset_window_and_ring();
        }
    }

    pub fn suspended(&self) -> bool {
        self.suspended.load(Ordering::SeqCst)
    }

    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    pub fn aec_enabled(&self) -> bool {
        self.aec_enabled.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_error(&self, message: Option<String>) {
        *self
            .error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = message;
    }

    fn set_aec_enabled(&self, enabled: bool) {
        self.aec_enabled.store(enabled, Ordering::Relaxed);
    }

    fn set_system_peak(&self, peak: f32) {
        let previous = f32::from_bits(self.system_peak.load(Ordering::Relaxed));
        let smoothed = peak.clamp(0.0, 4.0).max(previous * 0.82);
        self.system_peak
            .store(smoothed.to_bits(), Ordering::Relaxed);
    }

    fn reset_window_and_ring(&self) {
        // Keep lock ordering consistent with push_block: denoiser, ring, window.
        *self
            .denoiser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Denoiser::new();
        self.ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .samples
            .clear();
        self.window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .reset();
    }

    fn configure_ring(&self, sample_rate: u32) {
        let rate = sample_rate.max(1);
        if self.source_rate.swap(rate, Ordering::SeqCst) != rate {
            let limit = rate as usize * EVIDENCE_RING_SECONDS;
            self.ring_limit.store(limit, Ordering::SeqCst);
            let mut ring = self
                .ring
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            ring.samples.clear();
            ring.samples.reserve(limit.min(rate as usize * 8));
        }
    }

    pub fn push_block(&self, samples: &[f32], channels: usize, sample_rate: u32) {
        if channels == 0 || samples.is_empty() || !self.capturing() || self.suspended() {
            return;
        }
        // When Windows AEC is unavailable, detected render playback is excluded
        // from the meter, but still kept in the evidence ring so an alert can
        // retain the complete 30-second interval.
        let ignore_meter = !self.aec_enabled()
            && f32::from_bits(self.system_peak.load(Ordering::Relaxed))
                > SYSTEM_PLAYBACK_THRESHOLD;
        self.configure_ring(sample_rate);
        self.channels.store(channels as u16, Ordering::Relaxed);
        let frames = samples.len() / channels;
        if frames == 0 {
            return;
        }

        let mut sum_sq = 0.0_f64;
        let mut peak = 0.0_f32;
        let limit = self.ring_limit.load(Ordering::Relaxed).max(1);
        let gate = {
            let mut denoiser = self
                .denoiser
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            denoiser.configure(sample_rate);
            let mut ring = self
                .ring
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for frame in samples.chunks_exact(channels) {
                let sample = if channels == 1 {
                    frame[0]
                } else {
                    frame.iter().copied().sum::<f32>() / channels as f32
                };
                let value = if sample.is_finite() { sample } else { 0.0 };
                let filtered = denoiser.filter.process(value);
                sum_sq += (filtered as f64) * (filtered as f64);
                peak = peak.max(filtered.abs());
                if ring.samples.len() >= limit {
                    ring.samples.pop_front();
                }
                // Evidence keeps the high-pass filtered signal so the full
                // 30-second interval remains audible without DC/rumble.
                ring.samples.push_back(filtered);
            }
            let block_rms = (sum_sq / frames as f64).sqrt() as f32;
            denoiser.block_gain(block_rms)
        };
        if ignore_meter {
            return;
        }
        let effective_sum_sq = sum_sq * (gate as f64) * (gate as f64);
        let effective_peak = peak * gate;
        let rms = (effective_sum_sq / frames as f64).sqrt();
        let block_dbfs = 20.0 * rms.max(1e-7).log10();
        self.instant_dbfs
            .store(f32::to_bits(block_dbfs as f32), Ordering::Relaxed);
        self.total_frames
            .fetch_add(frames as u64, Ordering::Relaxed);

        {
            let mut window = self
                .window
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            window.sum_sq += effective_sum_sq;
            window.frames += frames as u64;
            window.peak = window.peak.max(effective_peak);
            window.min_block_dbfs = window.min_block_dbfs.min(block_dbfs);
        }
    }

    pub fn snapshot(&self) -> MeterSnapshot {
        let offset = self.offset() as f64;
        let instant_dbfs = f32::from_bits(self.instant_dbfs.load(Ordering::Relaxed)) as f64;
        let suspended = self.suspended();
        let window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let capturing = self.capturing() && !suspended;
        let has_audio = window.frames > 0;
        let average_dbfs = if has_audio {
            10.0 * (window.sum_sq / window.frames as f64).max(1e-14).log10()
        } else {
            instant_dbfs
        };
        let peak_dbfs = if window.peak > 0.0 {
            20.0 * (window.peak as f64).log10()
        } else {
            instant_dbfs
        };
        let elapsed = window.started.elapsed().as_secs_f64();
        drop(window);
        let system_peak = f32::from_bits(self.system_peak.load(Ordering::Relaxed));
        let ignoring_system = !self.aec_enabled() && system_peak > SYSTEM_PLAYBACK_THRESHOLD;
        let audible = capturing && !ignoring_system;

        MeterSnapshot {
            current_db: audible.then(|| (instant_dbfs + offset).clamp(0.0, 160.0)),
            average_db: audible.then(|| (average_dbfs + offset).clamp(0.0, 160.0)),
            peak_db: audible.then(|| (peak_dbfs + offset).clamp(0.0, 160.0)),
            progress: if suspended || ignoring_system {
                0.0
            } else {
                (elapsed / WINDOW_SECONDS).clamp(0.0, 1.0) as f32
            },
            capturing: self.capturing(),
            suspended,
            ready: self.ready(),
            aec_enabled: self.aec_enabled(),
            system_playing: system_peak > SYSTEM_PLAYBACK_THRESHOLD,
            error: self.error(),
            source_rate: self.source_rate.load(Ordering::Relaxed),
            channels: self.channels.load(Ordering::Relaxed),
        }
    }

    pub fn take_window(&self) -> Option<WindowReport> {
        if !self.capturing() || self.suspended() {
            return None;
        }
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if window.frames == 0 || window.started.elapsed().as_secs_f64() < WINDOW_SECONDS {
            return None;
        }
        let frames = window.frames;
        let duration = frames as f64 / self.source_rate.load(Ordering::SeqCst).max(1) as f64;
        let avg_dbfs = 10.0 * (window.sum_sq / frames as f64).max(1e-14).log10();
        let peak_dbfs = if window.peak > 0.0 {
            20.0 * (window.peak as f64).log10()
        } else {
            -100.0
        };
        let min_dbfs = if window.min_block_dbfs.is_finite() {
            window.min_block_dbfs
        } else {
            avg_dbfs
        };
        window.reset();
        Some(WindowReport {
            avg_dbfs,
            peak_dbfs,
            min_dbfs,
            frames,
            duration_secs: duration,
        })
    }

    pub fn take_evidence_wav(&self, seconds: u32) -> Option<Vec<u8>> {
        if self.suspended() {
            return None;
        }
        let source_rate = self.source_rate.load(Ordering::SeqCst);
        if source_rate == 0 {
            return None;
        }
        let wanted = (source_rate as usize).saturating_mul(seconds as usize);
        let ring = self
            .ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if ring.samples.is_empty() {
            return None;
        }
        let take = wanted.min(ring.samples.len());
        let start = ring.samples.len() - take;
        let source: Vec<f32> = ring.samples.iter().skip(start).copied().collect();
        drop(ring);
        let resampled = resample_linear(&source, source_rate, TARGET_EVIDENCE_RATE);
        if resampled.is_empty() {
            return None;
        }
        Some(encode_wav_i16(&resampled, TARGET_EVIDENCE_RATE))
    }
}

fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut output = Vec::with_capacity(output_len);
    for index in 0..output_len {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let right = (left + 1).min(input.len() - 1);
        let fraction = (position - left as f64) as f32;
        let sample = input[left] * (1.0 - fraction) + input[right] * fraction;
        output.push(sample.clamp(-1.0, 1.0));
    }
    output
}

fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_size = samples.len() * 2;
    let mut buffer = Vec::with_capacity(44 + data_size);
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&((36 + data_size) as u32).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes());
    buffer.extend_from_slice(&sample_rate.to_le_bytes());
    buffer.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    buffer.extend_from_slice(&2u16.to_le_bytes());
    buffer.extend_from_slice(&16u16.to_le_bytes());
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&(data_size as u32).to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    buffer
}

pub enum AudioCommand {
    Start,
    Pause,
    Retry,
}

struct WasapiCapture {
    audio_client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    queue: VecDeque<u8>,
    frame_bytes: usize,
    channels: usize,
    sample_rate: u32,
    aec_enabled: bool,
    render_meter: Option<IAudioMeterInformation>,
    sample_buffer: Vec<f32>,
    last_meter_poll: Instant,
}

impl WasapiCapture {
    fn new() -> Result<Self, String> {
        let device = get_default_device_for_role(&Direction::Capture, &Role::Communications)
            .or_else(|_| get_default_device(&Direction::Capture))
            .map_err(|error| format!("无法打开默认麦克风: {error}"))?;
        let mut audio_client = device
            .get_iaudioclient()
            .map_err(|error| format!("无法获取麦克风客户端: {error}"))?;
        let desired_format = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            TARGET_CAPTURE_RATE as usize,
            TARGET_CAPTURE_CHANNELS,
            None,
        );
        let frame_bytes = desired_format.get_blockalign() as usize;
        let (_default_period, min_period) = audio_client
            .get_device_period()
            .map_err(|error| format!("无法读取音频周期: {error}"))?;

        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };
        audio_client
            .initialize_client(&desired_format, &Direction::Capture, &mode)
            .map_err(|error| format!("无法初始化麦克风: {error}"))?;

        let mut aec_enabled = false;
        let render_device = get_default_device(&Direction::Render).ok();
        let render_id = render_device
            .as_ref()
            .and_then(|device| device.get_id().ok());
        match audio_client.is_aec_supported() {
            Ok(true) => {
                if let Ok(control) = audio_client.get_aec_control() {
                    if control
                        .set_echo_cancellation_render_endpoint(render_id)
                        .is_ok()
                    {
                        aec_enabled = true;
                    }
                }
            }
            Ok(false) => {}
            Err(error) => eprintln!("Could not query AEC support: {error}"),
        }
        // Always keep a render meter so system playback can be detected even when
        // the audio driver does not expose hardware echo cancellation.
        let render_meter = get_render_meter();

        let event = audio_client
            .set_get_eventhandle()
            .map_err(|error| format!("无法创建音频事件: {error}"))?;
        let capture_client = audio_client
            .get_audiocaptureclient()
            .map_err(|error| format!("无法创建采集客户端: {error}"))?;
        audio_client
            .start_stream()
            .map_err(|error| format!("无法启动麦克风: {error}"))?;

        Ok(Self {
            audio_client,
            capture_client,
            event,
            queue: VecDeque::with_capacity(frame_bytes * 2048),
            frame_bytes,
            channels: TARGET_CAPTURE_CHANNELS,
            sample_rate: TARGET_CAPTURE_RATE,
            aec_enabled,
            render_meter,
            sample_buffer: Vec::with_capacity(4096),
            last_meter_poll: Instant::now() - Duration::from_secs(1),
        })
    }

    fn wait_and_read(&mut self, shared: &AudioShared) -> Result<(), String> {
        if self.event.wait_for_event(120).is_err() {
            return Ok(());
        }
        self.capture_client
            .read_from_device_to_deque(&mut self.queue)
            .map_err(|error| format!("读取麦克风失败: {error}"))?;

        let complete = self.queue.len() / self.frame_bytes * self.frame_bytes;
        if complete > 0 {
            self.sample_buffer.clear();
            for _ in 0..(complete / 4) {
                let mut bytes = [0u8; 4];
                for byte in &mut bytes {
                    if let Some(value) = self.queue.pop_front() {
                        *byte = value;
                    }
                }
                self.sample_buffer.push(f32::from_le_bytes(bytes));
            }
            shared.push_block(&self.sample_buffer, self.channels, self.sample_rate);
        }

        if self.last_meter_poll.elapsed() >= Duration::from_millis(120) {
            self.last_meter_poll = Instant::now();
            if let Some(meter) = &self.render_meter {
                if let Ok(peak) = unsafe { meter.GetPeakValue() } {
                    shared.set_system_peak(peak);
                }
            }
        }
        Ok(())
    }

    fn pause(&self) {
        let _ = self.audio_client.stop_stream();
    }

    fn resume(&self) {
        let _ = self.audio_client.start_stream();
    }

    fn stop(&self) {
        let _ = self.audio_client.stop_stream();
    }
}

fn get_render_meter() -> Option<IAudioMeterInformation> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok()
    }
}

pub fn spawn_audio_engine(shared: Arc<AudioShared>) -> Sender<AudioCommand> {
    let (sender, receiver) = mpsc::channel::<AudioCommand>();
    std::thread::Builder::new()
        .name("soundguard-audio".to_string())
        .spawn(move || audio_loop(shared, receiver))
        .expect("could not start audio thread");
    sender
}

fn audio_loop(shared: Arc<AudioShared>, receiver: Receiver<AudioCommand>) {
    if let Err(error) = initialize_mta().ok() {
        shared.set_error(Some(format!("Windows 音频初始化失败: {error}")));
        return;
    }

    let mut capture: Option<WasapiCapture> = None;
    let mut next_retry = Instant::now();
    let mut consecutive_failures = 0_u32;

    loop {
        loop {
            match receiver.try_recv() {
                Ok(AudioCommand::Start) => {
                    shared.set_capturing(true);
                    shared.reset_window_and_ring();
                    if let Some(current) = &capture {
                        current.resume();
                    } else {
                        next_retry = Instant::now();
                    }
                }
                Ok(AudioCommand::Pause) => {
                    shared.set_capturing(false);
                    shared.set_system_peak(0.0);
                    if let Some(current) = &capture {
                        current.pause();
                    }
                }
                Ok(AudioCommand::Retry) => {
                    if let Some(current) = capture.take() {
                        current.stop();
                    }
                    shared.ready.store(false, Ordering::SeqCst);
                    shared.set_error(None);
                    shared.set_aec_enabled(false);
                    next_retry = Instant::now();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        if capture.is_none() && Instant::now() >= next_retry {
            match WasapiCapture::new() {
                Ok(current) => {
                    shared.ready.store(true, Ordering::SeqCst);
                    shared.set_error(None);
                    shared.set_aec_enabled(current.aec_enabled);
                    if !shared.capturing() {
                        current.pause();
                    }
                    capture = Some(current);
                    consecutive_failures = 0;
                }
                Err(error) => {
                    shared.ready.store(false, Ordering::SeqCst);
                    shared.set_error(Some(error));
                    next_retry = Instant::now() + Duration::from_secs(10);
                }
            }
        }

        if let Some(current) = &mut capture {
            if shared.capturing() {
                match current.wait_and_read(&shared) {
                    Ok(()) => {
                        consecutive_failures = 0;
                    }
                    Err(error) => {
                        consecutive_failures += 1;
                        if consecutive_failures >= 10 {
                            shared.set_error(Some(error));
                            if let Some(current) = capture.take() {
                                current.stop();
                            }
                            next_retry = Instant::now() + Duration::from_secs(5);
                        }
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_contains_full_thirty_seconds() {
        let shared = AudioShared::new(90.0);
        let rate = 48_000usize;
        let block = vec![0.01_f32; 480];
        for _ in 0..(rate * 31 / block.len()) {
            shared.push_block(&block, 1, rate as u32);
        }
        let wav = shared.take_evidence_wav(30).expect("evidence wav");
        assert!(wav.len() > 44);
        assert_eq!(&wav[0..4], b"RIFF");
        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        let samples = data_size / 2;
        assert_eq!(samples, 48_000 * 30);
    }
}
