//! RNNoise AI denoise engine (via the pure-Rust `nnnoiseless` port of
//! Xiph's RNNoise model) plus a speech-presence EQ for voice enhancement.
//!
//! Modes map to:
//! - `off`      bypass (engine not even scheduled by audio_core)
//! - `light`    55 % wet blend — gentle cleanup, minimal colouration
//! - `standard` full RNNoise output
//! - `strong`   full RNNoise plus a VAD-keyed gate that ducks non-speech
//!              segments by up to 10 dB (smoothed, never fully mutes)
//!
//! `voice_enhance` adds a +2.5 dB presence lift around 2.3 kHz and a slight
//! 5 kHz shelf so speech stays intelligible after denoising.
//!
//! RNNoise runs on 10 ms frames of 48 kHz mono in the 16-bit PCM range, so
//! samples are scaled by 32768 in and out.

use std::collections::VecDeque;

use nnnoiseless::DenoiseState;

use crate::audio::DenoiseEngine;

const FRAME: usize = DenoiseState::FRAME_SIZE; // 480 @ 48 kHz
const SCALE_IN: f32 = 32768.0;
const SCALE_OUT: f32 = 1.0 / 32768.0;

/// Peaking EQ biquad (RBJ cookbook).
struct PeakingEq {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl PeakingEq {
    fn new(sample_rate: f32, center: f32, q: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * center / sample_rate.max(8_000.0);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos_w0) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * out + self.z2;
        self.z2 = self.b2 * input - self.a2 * out;
        out
    }
}

pub struct RnnoiseEngine {
    state: Box<DenoiseState<'static>>,
    mode: u8,
    voice: bool,
    frame_buf: Vec<f32>,
    out_queue: VecDeque<f32>,
    vad_smooth: f32,
    presence: PeakingEq,
    air: PeakingEq,
}

impl RnnoiseEngine {
    pub fn new(mode: u8, voice: bool) -> Self {
        Self {
            state: DenoiseState::new(),
            mode,
            voice,
            frame_buf: Vec::with_capacity(FRAME),
            out_queue: VecDeque::with_capacity(FRAME * 4),
            vad_smooth: 0.0,
            presence: PeakingEq::new(48_000.0, 2_300.0, 0.9, 2.5),
            air: PeakingEq::new(48_000.0, 5_200.0, 0.7, 1.5),
        }
    }

    fn reset_internal(&mut self) {
        self.state = DenoiseState::new();
        self.frame_buf.clear();
        self.out_queue.clear();
        self.vad_smooth = 0.0;
        self.presence = PeakingEq::new(48_000.0, 2_300.0, 0.9, 2.5);
        self.air = PeakingEq::new(48_000.0, 5_200.0, 0.7, 1.5);
    }

    fn process_frame(&mut self) {
        let mut wet = [0.0f32; FRAME];
        let vad = self
            .state
            .process_frame(&mut wet, &self.frame_buf[..]);
        // RNNoise output is already in the 16-bit scale; return to [-1, 1].
        for sample in wet.iter_mut() {
            *sample *= SCALE_OUT;
        }

        self.vad_smooth = vad.max(self.vad_smooth * 0.93);
        let dry: [f32; FRAME] = std::array::from_fn(|i| self.frame_buf[i] * SCALE_OUT);
        let mixed: [f32; FRAME] = std::array::from_fn(|i| match self.mode {
            crate::audio::DENOISE_OFF => dry[i],
            crate::audio::DENOISE_LIGHT => 0.55 * wet[i] + 0.45 * dry[i],
            crate::audio::DENOISE_STRONG => {
                // Duck sustained non-speech segments without ever muting.
                let gate = 1.0 - 0.7 * ((0.15 - self.vad_smooth) / 0.15).clamp(0.0, 1.0);
                wet[i] * gate
            }
            // Standard and any unknown value use the full AI output.
            _ => wet[i],
        });

        for sample in mixed {
            let value = if self.voice {
                let shaped = self.presence.process(sample);
                self.air.process(shaped)
            } else {
                sample
            };
            self.out_queue.push_back(value.clamp(-1.0, 1.0));
        }
        self.frame_buf.clear();
    }
}

impl DenoiseEngine for RnnoiseEngine {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        for &sample in input {
            self.frame_buf.push(sample * SCALE_IN);
            if self.frame_buf.len() >= FRAME {
                self.process_frame();
            }
        }
        // 1:1 in/out mapping; the first frame (~10 ms) serves silence while
        // the model fills its pipeline.
        for _ in 0..input.len() {
            output.push(self.out_queue.pop_front().unwrap_or(0.0));
        }
    }

    fn set_config(&mut self, mode: u8, voice_enhance: bool) {
        let mode = mode.min(crate::audio::DENOISE_STRONG);
        if mode != self.mode || voice_enhance != self.voice {
            self.mode = mode;
            self.voice = voice_enhance;
            self.reset_internal();
        }
    }

    fn reset(&mut self) {
        self.reset_internal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            // xorshift64*; deterministic pseudo-noise in 16-bit scale.
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            let v = (x.wrapping_mul(0x2545F4914F6CDD1D) >> 33) as f32 / (u32::MAX as f32);
            (v - 0.5) * 6_000.0
        }
    }

    fn rms(samples: &[f32]) -> f64 {
        let sum = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>();
        (sum / samples.len().max(1) as f64).sqrt()
    }

    fn db(delta: f64) -> f64 {
        20.0 * delta.log10()
    }

    #[test]
    fn off_mode_is_identity() {
        let mut engine = RnnoiseEngine::new(crate::audio::DENOISE_OFF, false);
        let input: Vec<f32> = (0..FRAME * 4).map(|i| ((i % 97) as f32) / 40_000.0).collect();
        let mut out = Vec::new();
        engine.process(&input, &mut out);
        assert_eq!(out.len(), input.len());
        for (a, b) in input.iter().zip(out.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn standard_mode_suppresses_stationary_noise() {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let mut engine = RnnoiseEngine::new(crate::audio::DENOISE_STANDARD, false);
        let mut noisy: Vec<f32> = (0..FRAME * 60).map(|_| rng.next()).collect();
        noisy.truncate(FRAME * 60);

        let mut warm = Vec::new();
        engine.process(&noisy[..FRAME * 10], &mut warm);
        let mut tail = Vec::new();
        engine.process(&noisy[FRAME * 10..], &mut tail);

        // Wait for the model to settle before measuring suppression.
        let in_rms = rms(&noisy[FRAME * 10..]) as f64;
        let out_rms = rms(&tail[FRAME..]) as f64;
        let drop_db = db(in_rms / out_rms.max(1e-9));
        assert!(
            drop_db >= 8.0,
            "expected ≥8 dB suppression on stationary noise, got {drop_db:.2} dB"
        );
    }

    #[test]
    fn strong_mode_suppresses_noise_and_stays_finite() {
        let mut rng = Rng(0x1234567890ABCDEF);
        let mut engine = RnnoiseEngine::new(crate::audio::DENOISE_STRONG, true);
        let noisy: Vec<f32> = (0..FRAME * 40).map(|_| rng.next()).collect();
        let mut out = Vec::new();
        engine.process(&noisy, &mut out);
        assert_eq!(out.len(), noisy.len());
        assert!(out.iter().all(|s| s.is_finite()));
        let out_rms = rms(&out[FRAME..]) as f64;
        let in_rms = rms(&noisy[FRAME..]) as f64;
        // Pure stationary noise is the easiest case: heavy suppression is the
        // feature. Just guard that it is not bit-exact silence and that the
        // level did go down.
        assert!(out_rms > 0.0);
        assert!(db(in_rms / out_rms.max(1e-9)) >= 8.0);
    }

    #[test]
    fn voice_enhance_changes_output() {
        let mut rng = Rng(0xDEADBEEFCAFEBABE);
        let noisy: Vec<f32> = (0..FRAME * 20).map(|_| rng.next()).collect();
        let mut plain = RnnoiseEngine::new(crate::audio::DENOISE_STANDARD, false);
        let mut enhanced = RnnoiseEngine::new(crate::audio::DENOISE_STANDARD, true);
        let mut a = Vec::new();
        let mut b = Vec::new();
        plain.process(&noisy, &mut a);
        enhanced.process(&noisy, &mut b);
        let diff: f64 = a
            .iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs() as f64)
            .sum();
        assert!(diff > 0.0, "voice EQ had no effect");
    }
}
