//! WASAPI audio capture + measurement engine — self-contained for native client.
//! 80Hz high-pass → RNNoise AI denoise → 30s Leq + 15s rolling window.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wasapi::{initialize_mta, DeviceEnumerator, Direction, Handle, Role, SampleType, StreamMode, WaveFormat, AudioCaptureClient};

const WINDOW_SECONDS: f64 = 30.0;
const EVIDENCE_RING_SECONDS: usize = 32;
const TARGET_CAPTURE_RATE: u32 = 48_000;
const TARGET_CAPTURE_CHANNELS: usize = 2;

pub const DENOISE_OFF: u8 = 0;
pub const DENOISE_LIGHT: u8 = 1;
pub const DENOISE_STANDARD: u8 = 2;
pub const DENOISE_STRONG: u8 = 3;

#[derive(Debug, Clone)]
pub struct MeterSnapshot {
    pub current_db: Option<f64>,
    pub average_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub rolling_15s_db: Option<f64>,
    pub progress: f32,
    pub capturing: bool,
    pub suspended: bool,
    pub ready: bool,
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
    b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, z1: f32, z2: f32,
}

impl Biquad {
    fn high_pass(rate: u32) -> Self {
        let r = rate.max(8_000) as f32;
        let omega = 2.0 * std::f32::consts::PI * 80.0 / r;
        let cos = omega.cos();
        let alpha = omega.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 + cos) / 2.0) / a0,
            b1: (-(1.0 + cos)) / a0,
            b2: ((1.0 + cos) / 2.0) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0, z2: 0.0,
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

struct Accumulator {
    started: Instant, sum_sq: f64, frames: u64, peak: f32, min_block: f64,
}
impl Accumulator {
    fn new() -> Self { Self { started: Instant::now(), sum_sq: 0.0, frames: 0, peak: 0.0, min_block: f64::INFINITY } }
    fn reset(&mut self) { *self = Self::new(); }
}

struct RollingEnergy {
    blocks: VecDeque<(Instant, f64, u64)>, total_sq: f64, total_n: u64, span: f64,
}
impl RollingEnergy {
    fn new(span: f64) -> Self { Self { blocks: VecDeque::new(), total_sq: 0.0, total_n: 0, span } }
    fn push(&mut self, sq: f64, n: u64, now: Instant) {
        self.blocks.push_back((now, sq, n));
        self.total_sq += sq; self.total_n += n;
        while let Some((t, s, f)) = self.blocks.front() {
            if now.duration_since(*t).as_secs_f64() > self.span {
                self.total_sq -= s; self.total_n -= f;
                self.blocks.pop_front();
            } else { break; }
        }
    }
    fn dbfs(&self) -> Option<f64> {
        if self.total_n == 0 { return None; }
        Some(10.0 * (self.total_sq / self.total_n as f64).max(1e-14).log10())
    }
    fn reset(&mut self) { self.blocks.clear(); self.total_sq = 0.0; self.total_n = 0; }
}

pub trait DenoiseEngine: Send {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>);
    fn set_config(&mut self, mode: u8, voice: bool);
    fn reset(&mut self);
}

const RN_FRAME: usize = nnnoiseless::DenoiseState::FRAME_SIZE;
const RN_SCALE: f32 = 32768.0;
const RN_INV: f32 = 1.0 / 32768.0;

pub struct NativeRnnoiseEngine {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    mode: u8, voice: bool,
    buf: Vec<f32>, queue: VecDeque<f32>, vad: f32,
}

impl NativeRnnoiseEngine {
    pub fn new(mode: u8, voice: bool) -> Self {
        Self {
            state: nnnoiseless::DenoiseState::new(),
            mode, voice,
            buf: Vec::with_capacity(RN_FRAME),
            queue: VecDeque::with_capacity(RN_FRAME * 4),
            vad: 0.0,
        }
    }
    fn do_frame(&mut self) {
        let mut wet = [0.0f32; RN_FRAME];
        let vad = self.state.process_frame(&mut wet, &self.buf);
        for s in wet.iter_mut() { *s *= RN_INV; }
        self.vad = vad.max(self.vad * 0.93);
        let dry: Vec<f32> = self.buf.iter().map(|&s| s * RN_INV).collect();
        for i in 0..RN_FRAME {
            let v = match self.mode {
                DENOISE_OFF => dry[i],
                DENOISE_LIGHT => 0.55 * wet[i] + 0.45 * dry[i],
                DENOISE_STRONG => {
                    let g = 1.0 - 0.7 * ((0.15 - self.vad) / 0.15).clamp(0.0, 1.0);
                    wet[i] * g
                }
                _ => wet[i],
            };
            self.queue.push_back(v.clamp(-1.0, 1.0));
        }
        self.buf.clear();
    }
}

impl DenoiseEngine for NativeRnnoiseEngine {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        for &s in input {
            self.buf.push(s * RN_SCALE);
            if self.buf.len() >= RN_FRAME { self.do_frame(); }
        }
        for _ in 0..input.len() {
            output.push(self.queue.pop_front().unwrap_or(0.0));
        }
    }
    fn set_config(&mut self, mode: u8, voice: bool) {
        if mode != self.mode || voice != self.voice {
            self.mode = mode; self.voice = voice;
            self.state = nnnoiseless::DenoiseState::new();
            self.buf.clear(); self.queue.clear();
        }
    }
    fn reset(&mut self) {
        self.state = nnnoiseless::DenoiseState::new();
        self.buf.clear(); self.queue.clear();
    }
}

pub struct AudioShared {
    capturing: AtomicBool,
    suspended: AtomicBool,
    ready: AtomicBool,
    instant_dbfs: AtomicU32,
    offset_db: AtomicU32,
    source_rate: AtomicU32,
    channels: AtomicU16,
    ring_limit: AtomicUsize,
    denoise_mode: AtomicU8,
    voice_enhance: AtomicBool,
    window: Mutex<Accumulator>,
    rolling: Mutex<RollingEnergy>,
    ring: Mutex<VecDeque<f32>>,
    filter: Mutex<Biquad>,
    engine: Mutex<Option<Box<dyn DenoiseEngine>>>,
    error: Mutex<Option<String>>,
}

impl AudioShared {
    pub fn new(offset: f32) -> Arc<Self> {
        Arc::new(Self {
            capturing: AtomicBool::new(true),
            suspended: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            instant_dbfs: AtomicU32::new((-100.0f32).to_bits()),
            offset_db: AtomicU32::new(offset.to_bits()),
            source_rate: AtomicU32::new(0),
            channels: AtomicU16::new(0),
            ring_limit: AtomicUsize::new(0),
            denoise_mode: AtomicU8::new(DENOISE_OFF),
            voice_enhance: AtomicBool::new(false),
            window: Mutex::new(Accumulator::new()),
            rolling: Mutex::new(RollingEnergy::new(15.0)),
            ring: Mutex::new(VecDeque::with_capacity(48000 * 4)),
            filter: Mutex::new(Biquad::high_pass(48000)),
            engine: Mutex::new(None),
            error: Mutex::new(None),
        })
    }

    pub fn set_engine(&self, e: Box<dyn DenoiseEngine>) {
        *self.engine.lock().unwrap() = Some(e);
    }

    pub fn set_denoise(&self, mode: u8, voice: bool) {
        let m = mode.min(DENOISE_STRONG);
        if self.denoise_mode.swap(m, Ordering::SeqCst) != m
            || self.voice_enhance.swap(voice, Ordering::SeqCst) != voice {
            if let Some(e) = self.engine.lock().unwrap().as_mut() { e.set_config(m, voice); }
            self.reset_all();
        }
    }

    pub fn set_offset(&self, v: f32) {
        self.offset_db.store(v.to_bits(), Ordering::Relaxed);
        self.reset_all();
    }

    pub fn set_suspended(&self, v: bool) {
        self.suspended.store(v, Ordering::SeqCst);
        if v { self.reset_all(); }
    }

    pub fn set_ready(&self, v: bool) {
        self.ready.store(v, Ordering::SeqCst);
    }

    fn reset_all(&self) {
        *self.rolling.lock().unwrap() = RollingEnergy::new(15.0);
        *self.window.lock().unwrap() = Accumulator::new();
        self.ring.lock().unwrap().clear();
        if let Some(e) = self.engine.lock().unwrap().as_mut() { e.reset(); }
    }

    pub fn push_block(&self, samples: &[f32], channels: usize, sample_rate: u32) {
        if channels == 0 || samples.is_empty() || !self.capturing.load(Ordering::SeqCst) {
            return;
        }
        let frames = samples.len() / channels;
        if frames == 0 { return; }

        let rate = sample_rate.max(1);
        if self.source_rate.swap(rate, Ordering::SeqCst) != rate {
            self.ring_limit.store(rate as usize * EVIDENCE_RING_SECONDS, Ordering::SeqCst);
        }
        self.channels.store(channels as u16, Ordering::Relaxed);

        let dm = self.denoise_mode.load(Ordering::Relaxed);
        let mut sum_sq = 0.0_f64;
        let mut peak = 0.0_f32;
        let limit = self.ring_limit.load(Ordering::Relaxed).max(1);

        let mono: Vec<f32> = {
            let mut f = self.filter.lock().unwrap();
            samples.chunks_exact(channels).map(|fr| {
                let s = if channels == 1 { fr[0] } else { fr.iter().sum::<f32>() / channels as f32 };
                let v = if s.is_finite() { s } else { 0.0 };
                f.process(v)
            }).collect()
        };

        let proc: Vec<f32> = if dm != DENOISE_OFF {
            let mut eng = self.engine.lock().unwrap();
            let mut out = Vec::with_capacity(mono.len());
            if let Some(e) = eng.as_mut() {
                e.process(&mono, &mut out);
                if out.len() == mono.len() { out } else { mono }
            } else { mono }
        } else { mono };

        {
            let mut ring = self.ring.lock().unwrap();
            for &s in &proc {
                sum_sq += (s as f64) * (s as f64);
                peak = peak.max(s.abs());
                if ring.len() >= limit { ring.pop_front(); }
                ring.push_back(s);
            }
        }

        let rms = (sum_sq / frames as f64).sqrt();
        self.instant_dbfs.store((20.0 * rms.max(1e-7).log10() as f32).to_bits(), Ordering::Relaxed);

        {
            let mut w = self.window.lock().unwrap();
            w.sum_sq += sum_sq;
            w.frames += frames as u64;
            w.peak = w.peak.max(peak);
        }
        {
            let mut r = self.rolling.lock().unwrap();
            r.push(sum_sq, frames as u64, Instant::now());
        }
    }

    pub fn snapshot(&self) -> MeterSnapshot {
        let off = f32::from_bits(self.offset_db.load(Ordering::Relaxed)) as f64;
        let inst = f32::from_bits(self.instant_dbfs.load(Ordering::Relaxed)) as f64;
        let susp = self.suspended.load(Ordering::SeqCst);
        let ready = self.ready.load(Ordering::SeqCst);
        let capt = self.capturing.load(Ordering::SeqCst) && !susp && ready;

        let w = self.window.lock().unwrap();
        let has = w.frames > 0;
        let avg = if has { 10.0 * (w.sum_sq / w.frames as f64).max(1e-14).log10() } else { inst };
        let pk = if w.peak > 0.0 { 20.0 * (w.peak as f64).log10() } else { inst };
        let el = w.started.elapsed().as_secs_f64();
        drop(w);

        let r15 = self.rolling.lock().unwrap().dbfs();

        MeterSnapshot {
            current_db: capt.then(|| (inst + off).clamp(0.0, 160.0)),
            average_db: capt.then(|| (avg + off).clamp(0.0, 160.0)),
            peak_db: capt.then(|| (pk + off).clamp(0.0, 160.0)),
            rolling_15s_db: capt.then(|| ((r15.unwrap_or(inst)) + off).clamp(0.0, 160.0)),
            progress: if susp { 0.0 } else { (el / WINDOW_SECONDS).clamp(0.0, 1.0) as f32 },
            capturing: capt,
            suspended: susp,
            ready,
            error: None,
            source_rate: self.source_rate.load(Ordering::Relaxed),
            channels: self.channels.load(Ordering::Relaxed),
        }
    }

    pub fn take_window(&self) -> Option<WindowReport> {
        if !self.capturing.load(Ordering::SeqCst) || self.suspended.load(Ordering::SeqCst) { return None; }
        let mut w = self.window.lock().unwrap();
        if w.started.elapsed().as_secs_f64() < WINDOW_SECONDS { return None; }
        if w.frames == 0 { w.reset(); return None; }
        let f = w.frames;
        let d = w.started.elapsed().as_secs_f64();
        let avg = 10.0 * (w.sum_sq / f as f64).max(1e-14).log10();
        let pk = if w.peak > 0.0 { 20.0 * (w.peak as f64).log10() } else { -100.0 };
        let mn = if w.min_block.is_finite() { w.min_block } else { avg };
        w.reset();
        Some(WindowReport { avg_dbfs: avg, peak_dbfs: pk, min_dbfs: mn, frames: f, duration_secs: d })
    }

    pub fn take_evidence_wav(&self, secs: u32) -> Option<Vec<u8>> {
        let sr = self.source_rate.load(Ordering::SeqCst);
        if sr == 0 { return None; }
        let want = (sr as usize).saturating_mul(secs as usize);
        let ring = self.ring.lock().unwrap();
        if ring.is_empty() { return None; }
        let take = want.min(ring.len());
        let src: Vec<f32> = ring.iter().skip(ring.len() - take).copied().collect();
        drop(ring);

        let dsz = src.len() * 2;
        let mut wav = Vec::with_capacity(44 + dsz);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + dsz as u32).to_le_bytes()));
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sr.to_le_bytes());
        wav.extend_from_slice(&(sr * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(dsz as u32).to_le_bytes());
        for s in src {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            wav.extend_from_slice(&v.to_le_bytes());
        }
        Some(wav)
    }
}

pub struct AudioEngineHandle {
    pub shared: Arc<AudioShared>,
    running: Arc<AtomicBool>,
}

impl AudioEngineHandle {
    pub fn start(calibration_offset: f32) -> Self {
        let shared = AudioShared::new(calibration_offset);
        shared.set_engine(Box::new(NativeRnnoiseEngine::new(DENOISE_OFF, false)));
        let running = Arc::new(AtomicBool::new(true));
        let s = shared.clone();
        let r = running.clone();
        std::thread::Builder::new()
            .name("sg-audio".to_string())
            .spawn(move || audio_loop(s, r))
            .expect("audio thread");
        Self { shared, running }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn audio_loop(shared: Arc<AudioShared>, running: Arc<AtomicBool>) {
    let _ = initialize_mta();
    while running.load(Ordering::SeqCst) {
        if let Err(e) = capture_once(&shared) {
            eprintln!("audio: {e}");
            std::thread::sleep(Duration::from_secs(3));
        }
    }
}

fn capture_once(shared: &AudioShared) -> Result<(), String> {
    let enum_ = DeviceEnumerator::new().map_err(|e| format!("enum: {e}"))?;
    let dev = enum_
        .get_default_device_for_role(&Direction::Capture, &Role::Communications)
        .or_else(|_| enum_.get_default_device(&Direction::Capture))
        .map_err(|e| format!("mic: {e}"))?;
    let mut ac = dev.get_iaudioclient().map_err(|e| format!("cl: {e}"))?;
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, TARGET_CAPTURE_RATE as usize, TARGET_CAPTURE_CHANNELS, None);
    let fb = fmt.get_blockalign() as usize;
    let (_, mp) = ac.get_device_period().map_err(|e| format!("pd: {e}"))?;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: mp };
    ac.initialize_client(&fmt, &Direction::Capture, &mode).map_err(|e| format!("in: {e}"))?;
    let ev: Handle = ac.set_get_eventhandle().map_err(|e| format!("ev: {e}"))?;
    let cap: AudioCaptureClient = ac.get_audiocaptureclient().map_err(|e| format!("cp: {e}"))?;
    ac.start_stream().map_err(|e| format!("st: {e}"))?;

    let mut q: VecDeque<u8> = VecDeque::with_capacity(fb * 2048);
    let mut sb: Vec<f32> = Vec::with_capacity(4096);
    shared.set_ready(true);

    loop {
        if ev.wait_for_event(120).is_err() { continue; }
        cap.read_from_device_to_deque(&mut q).map_err(|e| format!("rd: {e}"))?;
        let n = q.len() / fb * fb;
        if n == 0 { continue; }
        sb.clear();
        for _ in 0..(n / 4) {
            let mut b = [0u8; 4];
            for x in b.iter_mut() { *x = q.pop_front().unwrap_or(0); }
            sb.push(f32::from_le_bytes(b));
        }
        shared.push_block(&sb, TARGET_CAPTURE_CHANNELS, TARGET_CAPTURE_RATE);
    }
}
