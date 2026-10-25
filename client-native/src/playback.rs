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
#[derive(Debug, Clone, Default)]
pub struct PlaybackStatus {
    pub id: String,
    pub title: String,
    pub duration: f32,
    pub position: f32,
    pub playing: bool,
    pub loaded: bool,
    pub error: Option<String>,
}

pub struct RecordingPlayer {
    tx: SyncSender<PlayerCommand>,
    status: std::sync::Arc<std::sync::Mutex<PlaybackStatus>>,
}

enum PlayerCommand {
    Load {
        id: String,
        title: String,
        bytes: Vec<u8>,
    },
    Toggle,
    Seek(f32),
    Stop,
}

impl RecordingPlayer {
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(8);
        let status = std::sync::Arc::new(std::sync::Mutex::new(PlaybackStatus::default()));
        let shared = status.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let (_stream, handle) = match rodio::OutputStream::try_default() {
                Ok(value) => value,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("扬声器初始化失败：{error}")));
                    return;
                }
            };
            let sink = match rodio::Sink::try_new(&handle) {
                Ok(value) => value,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut duration = 0.0_f32;
            loop {
                match rx.recv_timeout(std::time::Duration::from_millis(120)) {
                    Ok(PlayerCommand::Load { id, title, bytes }) => match decode_file_to_pcm(bytes) {
                        Ok((pcm, channels, rate)) => {
                            duration = pcm.len() as f32 / channels as f32 / rate as f32;
                            sink.clear();
                            sink.append(rodio::buffer::SamplesBuffer::new(channels, rate, pcm));
                            sink.play();
                            let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                            s.id = id;
                            s.title = title;
                            s.duration = duration;
                            s.position = 0.0;
                            s.playing = true;
                            s.loaded = true;
                            s.error = None;
                        }
                        Err(error) => {
                            sink.stop();
                            let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                            s.loaded = false;
                            s.playing = false;
                            s.error = Some(error);
                        }
                    },
                    Ok(PlayerCommand::Toggle) => {
                        if sink.is_paused() {
                            sink.play();
                        } else {
                            sink.pause();
                        }
                    }
                    Ok(PlayerCommand::Seek(position)) => {
                        let target = position.clamp(0.0, duration.max(0.0));
                        let _ = sink.try_seek(std::time::Duration::from_secs_f32(target));
                    }
                    Ok(PlayerCommand::Stop) => {
                        sink.clear();
                        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                        s.loaded = false;
                        s.playing = false;
                        s.position = 0.0;
                        s.id.clear();
                        s.title.clear();
                        s.duration = 0.0;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                        if s.loaded {
                            s.position = sink.get_pos().as_secs_f32().min(duration);
                            s.playing = !sink.is_paused() && !sink.empty();
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "扬声器响应超时".to_string())??;
        Ok(Self { tx, status })
    }
    pub fn load(&self, id: String, title: String, bytes: Vec<u8>) {
        let _ = self.tx.try_send(PlayerCommand::Load { id, title, bytes });
    }
    pub fn toggle(&self) {
        let _ = self.tx.try_send(PlayerCommand::Toggle);
    }
    pub fn seek(&self, position: f32) {
        let _ = self.tx.try_send(PlayerCommand::Seek(position));
    }
    pub fn stop(&self) {
        let _ = self.tx.try_send(PlayerCommand::Stop);
    }
    pub fn status(&self) -> PlaybackStatus {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

pub fn decode_file_to_pcm(bytes: Vec<u8>) -> Result<(Vec<f32>, u16, u32), String> {
    let mut samples = Vec::new();
    let mut channels = 1_u16;
    let mut rate = 48_000_u32;
    decode_file(bytes, |ch, sample_rate, part| {
        channels = ch;
        rate = sample_rate;
        samples.extend(part);
        Ok(())
    })?;
    Ok((samples, channels, rate))
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

/// Three rising/falling sweeps (1.5 s) so the alarm is clearly audible even
/// when the ambient level is high.
pub fn alarm_samples() -> Vec<f32> {
    let mut phase = 0.0_f32;
    let mut output = Vec::with_capacity(72_000);
    for i in 0..72_000 {
        let t = i as f32 / 48_000.0;
        let cycle = t % 0.5;
        if cycle < 0.38 {
            let frequency = 700.0 + 400.0 * (cycle / 0.38);
            phase += 2.0 * std::f32::consts::PI * frequency / 48_000.0;
            output.push(phase.sin() * 0.75);
        } else {
            phase = 0.0;
            output.push(0.0);
        }
    }
    output
}

/// Play through a normal default output stream; no shared queue or intercom
/// sink is involved, so the alarm always starts at full length.
pub fn play_alarm(volume: f32) -> Result<(), String> {
    let (_stream, handle) =
        rodio::OutputStream::try_default().map_err(|e| format!("扬声器初始化失败：{e}"))?;
    let sink = rodio::Sink::try_new(&handle).map_err(|e| e.to_string())?;
    sink.set_volume(volume.clamp(0.0, 1.0));
    sink.append(rodio::buffer::SamplesBuffer::new(
        1,
        48_000,
        alarm_samples(),
    ));
    sink.sleep_until_end();
    Ok(())
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
    #[ignore = "requires an audio output device"]
    fn recording_player_loads_and_seeks() {
        fn silent_wav(seconds: u32) -> Vec<u8> {
            let data_len = seconds * 48_000 * 2;
            let mut wav = Vec::with_capacity(44 + data_len as usize);
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data_len).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&48_000u32.to_le_bytes());
            wav.extend_from_slice(&96_000u32.to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data_len.to_le_bytes());
            wav.resize(44 + data_len as usize, 0);
            wav
        }
        let player = RecordingPlayer::new().expect("speaker");
        player.load("test".into(), "测试录音".into(), silent_wav(2));
        for _ in 0..30 {
            if player.status().loaded {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let status = player.status();
        assert!(status.loaded, "recording did not load: {:?}", status.error);
        assert!((status.duration - 2.0).abs() < 0.2, "duration={}", status.duration);
        player.seek(1.0);
        std::thread::sleep(std::time::Duration::from_millis(350));
        assert!(player.status().position > 0.7, "seek did not move");
        player.stop();
    }
    #[test]
    fn alarm_signal_is_finite_and_audible() {
        let samples = alarm_samples();
        assert_eq!(samples.len(), 72_000);
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
