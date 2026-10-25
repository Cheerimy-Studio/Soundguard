//! Bounded speaker queue. The output stream never leaves its owning thread.
use std::sync::mpsc::{self, SyncSender};
#[derive(Clone)]
pub struct AudioOutput {
    tx: SyncSender<Output>,
    generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
enum Output {
    Pcm(u16, u32, Vec<f32>, bool, u64),
    Stop,
    Volume(f32),
}
impl AudioOutput {
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(32);
        let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let current = generation.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let stream =
                rodio::OutputStream::try_default().map_err(|e| format!("扬声器初始化失败：{e}"));
            let (_stream, handle) = match stream {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let sink = match rodio::Sink::try_new(&handle) {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut last_generation = 0;
            while let Ok(msg) = rx.recv() {
                let latest = current.load(std::sync::atomic::Ordering::SeqCst);
                if latest != last_generation {
                    sink.clear();
                    last_generation = latest;
                }
                match msg {
                    Output::Pcm(ch, rate, pcm, realtime, id) => {
                        while !realtime
                            && sink.len() >= 16
                            && id == current.load(std::sync::atomic::Ordering::SeqCst)
                        {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        if sink.len() < 16
                            && id == current.load(std::sync::atomic::Ordering::SeqCst)
                        {
                            sink.append(rodio::buffer::SamplesBuffer::new(ch, rate, pcm));
                        }
                    }
                    Output::Stop => sink.clear(),
                    Output::Volume(v) => sink.set_volume(v.clamp(0.0, 1.0)),
                }
            }
            sink.stop();
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "扬声器响应超时")??;
        Ok(Self { tx, generation })
    }
    pub fn pcm(&self, channels: u16, rate: u32, pcm: Vec<f32>) {
        let _ = self.tx.try_send(Output::Pcm(
            channels,
            rate,
            pcm,
            true,
            self.generation.load(std::sync::atomic::Ordering::SeqCst),
        ));
    }
    pub fn stop(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.try_send(Output::Stop);
    }
    pub fn volume(&self, v: f32) {
        let _ = self.tx.try_send(Output::Volume(v));
    }
    pub fn ticket(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn play_file(&self, bytes: Vec<u8>, generation: u64) -> Result<(), String> {
        let result = decode_file(bytes, |ch, rate, pcm| {
            if generation != self.ticket() {
                return Err("播放已取消".into());
            }
            self.tx
                .send(Output::Pcm(ch, rate, pcm, false, generation))
                .map_err(|_| "扬声器已关闭".into())
        });
        if generation != self.ticket() {
            Ok(())
        } else {
            result
        }
    }
    #[cfg(test)]
    pub fn test_null() -> Self {
        let (tx, rx) = mpsc::sync_channel(32);
        std::thread::spawn(move || while rx.recv().is_ok() {});
        Self {
            tx,
            generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}
pub fn decode_file(
    bytes: Vec<u8>,
    emit: impl FnMut(u16, u32, Vec<f32>) -> Result<(), String>,
) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_file_inner(bytes, emit)
    }))
    .map_err(|_| "录音容器损坏，无法安全解码".to_string())?
}
fn decode_file_inner(
    bytes: Vec<u8>,
    mut emit: impl FnMut(u16, u32, Vec<f32>) -> Result<(), String>,
) -> Result<(), String> {
    use symphonia::core::{
        audio::SampleBuffer,
        codecs::{DecoderOptions, CODEC_TYPE_OPUS},
        formats::FormatOptions,
        io::{MediaSourceStream, MediaSourceStreamOptions},
        meta::MetadataOptions,
        probe::Hint,
    };
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("无法识别录音格式：{e}"))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("录音中没有音轨")?;
    let id = track.id;
    let params = track.codec_params.clone();
    let mut frames = 0;
    if params.codec == CODEC_TYPE_OPUS {
        // Symphonia demuxes WebM/Ogg; libopus decodes their Opus packets.
        let channels = params.channels.map(|c| c.count()).unwrap_or(1);
        let mode = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            _ => return Err("只支持单声道或双声道 Opus 录音".into()),
        };
        let header = params.extra_data.as_deref().unwrap_or(&[]);
        if header.starts_with(b"OpusHead") && (header.len() < 19 || header[18] != 0) {
            return Err("不支持此 Opus 通道映射".into());
        }
        let mut skip = if header.starts_with(b"OpusHead") && header.len() >= 12 {
            u16::from_le_bytes([header[10], header[11]]) as usize
        } else {
            params.delay.unwrap_or(0) as usize
        };
        let mut decoder =
            opus::Decoder::new(48000, mode).map_err(|e| format!("Opus 初始化失败：{e}"))?;
        let mut buffer = vec![0.0_f32; 5760 * channels];
        while let Some(packet) = next_packet(format.as_mut())? {
            if packet.track_id() != id {
                continue;
            }
            let n = decoder
                .decode_float(&packet.data, &mut buffer, false)
                .map_err(|e| format!("Opus 录音损坏：{e}"))?;
            let trim = skip.min(n);
            skip -= trim;
            let pcm = &buffer[trim * channels..n * channels];
            if !pcm.is_empty() {
                frames += pcm.len();
                emit(channels as u16, 48000, pcm.to_vec())?;
            }
        }
    } else {
        let mut decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| format!("不支持的录音编码：{e}"))?;
        while let Some(packet) = next_packet(format.as_mut())? {
            if packet.track_id() != id {
                continue;
            }
            let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
            let spec = *decoded.spec();
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            frames += buf.samples().len();
            emit(
                spec.channels.count() as u16,
                spec.rate,
                buf.samples().to_vec(),
            )?;
        }
    }
    if frames == 0 {
        return Err("录音中没有可播放的音频数据".into());
    }
    Ok(())
}

/// Same three 880 Hz tones as the legacy client's over-limit alarm.
pub fn alarm_samples() -> Vec<f32> {
    (0..48000)
        .map(|i| {
            let t = i as f32 / 48000.0;
            let local = t % 0.36;
            if local < 0.28 {
                (2.0 * std::f32::consts::PI * 880.0 * t).sin() * (-8.0 * local / 0.28).exp()
            } else {
                0.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_webm_opus_without_losing_tail() {
        let bytes = include_bytes!("../tests/fixtures/browser-tone.webm");
        let mut samples = Vec::new();
        let mut channels = 0;
        decode_file(bytes.to_vec(), |ch, rate, pcm| {
            assert_eq!(rate, 48000);
            channels = ch;
            samples.extend(pcm);
            Ok(())
        })
        .unwrap();
        let duration = samples.len() as f64 / channels as f64 / 48000.0;
        assert!(
            (1.1..1.5).contains(&duration),
            "decoded duration={duration}"
        );
        assert!(samples.iter().all(|v| v.is_finite()));
        assert!(samples.iter().any(|v| v.abs() > 0.1));
    }
    #[test]
    fn rejects_non_audio_instead_of_silent_success() {
        assert!(decode_file(b"{broken}".to_vec(), |_, _, _| Ok(())).is_err());
        let mut malformed = vec![0; 132];
        malformed[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
        assert!(decode_file(malformed, |_, _, _| Ok(())).is_err());
    }
    #[test]
    fn alarm_signal_is_finite_and_audible() {
        let samples = alarm_samples();
        assert_eq!(samples.len(), 48_000);
        assert!(samples.iter().all(|v| v.is_finite()));
        assert!(samples.iter().any(|v| v.abs() > 0.25));
        assert!(samples.iter().any(|v| v.abs() < 0.001));
    }
}

fn next_packet(
    format: &mut dyn symphonia::core::formats::FormatReader,
) -> Result<Option<symphonia::core::formats::Packet>, String> {
    match format.next_packet() {
        Ok(p) => Ok(Some(p)),
        Err(symphonia::core::errors::Error::IoError(e))
            if e.kind() == std::io::ErrorKind::UnexpectedEof =>
        {
            Ok(None)
        }
        Err(e) => Err(format!("录音数据损坏：{e}")),
    }
}
