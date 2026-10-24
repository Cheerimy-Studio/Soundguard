use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
pub const WINDOW_SECONDS: f64 = 30.0;
const EVIDENCE_RING_SECONDS: usize = 32;
const TARGET_EVIDENCE_RATE: u32 = 48_000;
const SYSTEM_PLAYBACK_THRESHOLD: f32 = 0.0045;

pub const DENOISE_OFF: u8 = 0;
pub const DENOISE_LIGHT: u8 = 1;
pub const DENOISE_STANDARD: u8 = 2;
pub const DENOISE_STRONG: u8 = 3;

/// Streaming denoise engine injected at startup. The RNNoise AI engine lives
/// in `rnnoise_engine.rs` (it needs the nnnoiseless crate); audio_core stays
/// dependency-free so the shared-core test script can compile it standalone.
pub trait DenoiseEngine: Send {
    /// Process a mono block. Must return exactly `input.len()` samples so the
    /// meter and the evidence ring keep a stable 1:1 sample mapping.
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>);
    /// Applied when the administrator changes the denoise settings; engines
    /// should drop learned state here (the 30 s window is reset as well).
    fn set_config(&mut self, mode: u8, voice_enhance: bool);
    /// Drop learned state (pause/resume, calibration, mode change).
    fn reset(&mut self);
}

#[derive(Debug, Clone)]
pub struct MeterSnapshot {
    pub current_db: Option<f64>,
    pub average_db: Option<f64>,
    pub peak_db: Option<f64>,
    /// Live 15-second rolling Leq. Used for the yellow early-warning level;
    /// the 30 s settlement window drives the official red alert.
    pub rolling_15s_db: Option<f64>,
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
}

impl Denoiser {
    fn new() -> Self {
        Self {
            filter: Biquad::high_pass(48_000),
            sample_rate: 48_000,
        }
    }

    fn configure(&mut self, sample_rate: u32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.filter = Biquad::high_pass(sample_rate);
        }
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

/// Tracks energy over a rolling time span using exponentially-spaced block
/// accumulators. Each block covers ~1 s of audio so the 15 s window stays
/// accurate to ±1 s without a full sample ring.
struct RollingEnergy {
    blocks: VecDeque<(Instant, f64, u64)>, // (started, sum_sq, frames)
    total_sum_sq: f64,
    total_frames: u64,
    span_secs: f64,
}

impl RollingEnergy {
    fn new(span_secs: f64) -> Self {
        Self {
            blocks: VecDeque::new(),
            total_sum_sq: 0.0,
            total_frames: 0,
            span_secs,
        }
    }

    fn push(&mut self, sum_sq: f64, frames: u64, now: Instant) {
        self.blocks.push_back((now, sum_sq, frames));
        self.total_sum_sq += sum_sq;
        self.total_frames += frames;
        self.trim(now);
    }

    fn trim(&mut self, now: Instant) {
        while let Some(front) = self.blocks.front() {
            if now.duration_since(front.0).as_secs_f64() > self.span_secs {
                let (_, sum_sq, frames) = self.blocks.pop_front().unwrap();
                self.total_sum_sq -= sum_sq;
                self.total_frames = self.total_frames.saturating_sub(frames);
            } else {
                break;
            }
        }
    }

    fn dbfs(&self) -> Option<f64> {
        if self.total_frames == 0 {
            return None;
        }
        Some(10.0 * (self.total_sum_sq / self.total_frames as f64).max(1e-14).log10())
    }

    fn reset(&mut self) {
        self.blocks.clear();
        self.total_sum_sq = 0.0;
        self.total_frames = 0;
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
    /// Rolling 15 s energy tracker for the early-warning display.
    rolling_15s: Mutex<RollingEnergy>,
    ring: Mutex<RingBuffer>,
    denoiser: Mutex<Denoiser>,
    denoise_mode: AtomicU8,
    voice_enhance: AtomicBool,
    engine: Mutex<Option<Box<dyn DenoiseEngine>>>,
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
            rolling_15s: Mutex::new(RollingEnergy::new(15.0)),
            ring: Mutex::new(RingBuffer {
                samples: VecDeque::with_capacity(48_000 * 4),
            }),
            denoiser: Mutex::new(Denoiser::new()),
            denoise_mode: AtomicU8::new(DENOISE_OFF),
            voice_enhance: AtomicBool::new(false),
            engine: Mutex::new(None),
            error: Mutex::new(None),
        })
    }

    pub fn set_offset(&self, offset_db: f32) {
        self.reset_window_and_ring();
        self.offset_db
            .store(f32::to_bits(offset_db), Ordering::Relaxed);
    }

    pub fn offset(&self) -> f32 {
        f32::from_bits(self.offset_db.load(Ordering::Relaxed))
    }

    /// Install the RNNoise engine. Called once at startup.
    pub fn set_engine(&self, engine: Box<dyn DenoiseEngine>) {
        *self
            .engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(engine);
    }

    /// Apply administrator-configured denoise settings. Changing either value
    /// restarts the 30 s measurement window so levels never mix pre/post
    /// configurations, and the evidence ring only holds one configuration.
    pub fn set_denoise(&self, mode: u8, voice_enhance: bool) {
        let mode = mode.min(DENOISE_STRONG);
        let changed = self.denoise_mode.swap(mode, Ordering::SeqCst) != mode
            || self.voice_enhance.swap(voice_enhance, Ordering::SeqCst) != voice_enhance;
        if changed {
            if let Some(engine) = self
                .engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_mut()
            {
                engine.set_config(mode, voice_enhance);
            }
            self.reset_window_and_ring();
        }
    }

    #[allow(dead_code)] // surfaced through Tauri status in a later revision
    pub fn denoise_mode(&self) -> u8 {
        self.denoise_mode.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    pub fn voice_enhance(&self) -> bool {
        self.voice_enhance.load(Ordering::Relaxed)
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
        // Keep lock ordering consistent with push_block: denoiser, engine,
        // ring, window.
        *self
            .denoiser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Denoiser::new();
        if let Some(engine) = self
            .engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_mut()
        {
            engine.reset();
        }
        self.ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .samples
            .clear();
        self.rolling_15s
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .reset();
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
            && f32::from_bits(self.system_peak.load(Ordering::Relaxed)) > SYSTEM_PLAYBACK_THRESHOLD;
        self.configure_ring(sample_rate);
        self.channels.store(channels as u16, Ordering::Relaxed);
        let frames = samples.len() / channels;
        if frames == 0 {
            return;
        }

        let mut sum_sq = 0.0_f64;
        let mut peak = 0.0_f32;
        let limit = self.ring_limit.load(Ordering::Relaxed).max(1);
        let denoise_mode = self.denoise_mode.load(Ordering::Relaxed);
        {
            let mut denoiser = self
                .denoiser
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            denoiser.configure(sample_rate);
            let mut engine = self
                .engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut ring = self
                .ring
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let mut mono: Vec<f32> = Vec::with_capacity(samples.len() / channels.max(1));
            for frame in samples.chunks_exact(channels) {
                let sample = if channels == 1 {
                    frame[0]
                } else {
                    frame.iter().copied().sum::<f32>() / channels as f32
                };
                let value = if sample.is_finite() { sample } else { 0.0 };
                mono.push(denoiser.filter.process(value));
            }

            // AI denoise stage (RNNoise via nnnoiseless). When off — or before
            // the engine has been installed — the high-pass filtered signal is
            // used directly.
            let mut denoised: Vec<f32>;
            let processed: &[f32] = if denoise_mode != DENOISE_OFF {
                match engine.as_mut() {
                    Some(engine) => {
                        denoised = Vec::with_capacity(mono.len());
                        engine.process(&mono, &mut denoised);
                        if denoised.len() == mono.len() {
                            &denoised
                        } else {
                            &mono
                        }
                    }
                    None => &mono,
                }
            } else {
                &mono
            };

            for &filtered in processed.iter() {
                sum_sq += (filtered as f64) * (filtered as f64);
                peak = peak.max(filtered.abs());
                if ring.samples.len() >= limit {
                    ring.samples.pop_front();
                }
                // Evidence matches what the meter measures: high-passed, and
                // AI-denoised when the administrator enabled it.
                ring.samples.push_back(filtered);
            }
        }
        if ignore_meter {
            return;
        }
        let effective_sum_sq = sum_sq;
        let effective_peak = peak;
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
        {
            let mut rolling = self
                .rolling_15s
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            rolling.push(effective_sum_sq, frames as u64, Instant::now());
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
        let capturing = self.capturing() && !suspended && self.ready();
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
        let audible = capturing && !ignoring_system && has_audio;

        let rolling_15s_dbfs = {
            let mut rolling = self
                .rolling_15s
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            rolling.trim(Instant::now());
            rolling.dbfs()
        };
        MeterSnapshot {
            current_db: audible.then(|| (instant_dbfs + offset).clamp(0.0, 160.0)),
            average_db: audible.then(|| (average_dbfs + offset).clamp(0.0, 160.0)),
            peak_db: audible.then(|| (peak_dbfs + offset).clamp(0.0, 160.0)),
            rolling_15s_db: audible
                .then(|| ((rolling_15s_dbfs.unwrap_or(instant_dbfs)) + offset).clamp(0.0, 160.0)),
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
        if window.started.elapsed().as_secs_f64() < WINDOW_SECONDS {
            return None;
        }
        if window.frames == 0 {
            window.reset();
            return None;
        }
        let frames = window.frames;
        let duration = window.started.elapsed().as_secs_f64();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_environmental_noise_is_not_attenuated_over_time() {
        let shared = AudioShared::new(90.0);
        shared.ready.store(true, Ordering::SeqCst);
        let block: Vec<f32> = (0..480)
            .map(|i| 0.1 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        for _ in 0..100 {
            shared.push_block(&block, 1, 48000);
        }
        let initial = shared.snapshot().current_db.unwrap();
        for _ in 0..4000 {
            shared.push_block(&block, 1, 48000);
        }
        let later = shared.snapshot().current_db.unwrap();
        assert!((initial - later).abs() < 0.1);
        assert!((later - 67.0).abs() < 0.2);
        let mut window = shared.window.lock().unwrap();
        window.started = Instant::now() - std::time::Duration::from_secs(30);
        drop(window);
        assert!(shared.take_window().unwrap().avg_dbfs.is_finite());
    }

    #[test]
    fn suspend_and_calibration_clear_stale_measurements() {
        let shared = AudioShared::new(90.0);
        shared.push_block(&[0.1; 480], 1, 48000);
        assert!(shared.take_evidence_wav(30).is_some());
        shared.set_suspended(true);
        assert!(shared.take_evidence_wav(30).is_none());
        shared.set_suspended(false);
        assert!(shared.take_evidence_wav(30).is_none());
        shared.push_block(&[0.1; 480], 1, 48000);
        shared.set_offset(95.0);
        assert!(shared.take_evidence_wav(30).is_none());
        shared.set_error(None);
        shared.set_aec_enabled(false);
        shared.set_system_peak(0.0);
    }

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
