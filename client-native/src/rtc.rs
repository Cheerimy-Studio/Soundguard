//! Browser-compatible PCMU (G.711 μ-law), with ICE/TURN and bounded media tasks.
use crate::{audio::AudioEngineHandle, playback::AudioOutput};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc::UnboundedSender;
use webrtc::{
    api::{
        media_engine::{MediaEngine, MIME_TYPE_PCMU},
        APIBuilder,
    },
    ice_transport::ice_server::RTCIceServer,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};
pub fn pcm_to_mulaw(pcm: &[f32]) -> Vec<u8> {
    pcm.iter()
        .map(|s| {
            let raw = (s.clamp(-1.0, 1.0) * 32767.0) as i32;
            let sign = if raw < 0 { 0x80 } else { 0 };
            let mag = raw.abs().min(32635) + 132;
            let exp = (0..7).find(|e| mag < (0x100 << e)).unwrap_or(7);
            (!(sign | (exp << 4) | ((mag >> (exp + 3)) & 15))) as u8
        })
        .collect()
}
pub fn mulaw_to_pcm(data: &[u8]) -> Vec<f32> {
    data.iter()
        .map(|b| {
            let u = !*b;
            let mag = (((i32::from(u & 15) << 3) + 132) << ((u >> 4) & 7)) - 132;
            if u & 128 != 0 {
                -(mag as f32) / 32768.0
            } else {
                mag as f32 / 32768.0
            }
        })
        .collect()
}
/// Compares 16-bit RTP sequence numbers across wrap-around.
fn seq_older(seq: u16, expected: u16) -> bool {
    seq != expected && expected.wrapping_sub(seq) < 0x8000
}

/// Appends mono PCM at 48 kHz for the Opus encoder. WASAPI normally captures
/// at 48 kHz; other rates use linear interpolation as a best-effort fallback.
fn append_resampled_48k(pending: &mut Vec<f32>, rate: u32, samples: &[f32]) {
    if rate == 48_000 {
        pending.extend_from_slice(samples);
        return;
    }
    if rate == 0 {
        return;
    }
    let target = (samples.len() as f64 * 48_000.0 / rate as f64).round() as usize;
    for index in 0..target {
        let position = index as f64 * rate as f64 / 48_000.0;
        let base = position.floor() as usize;
        let fraction = (position - base as f64) as f32;
        let first = samples.get(base).copied().unwrap_or(0.0);
        let second = samples.get(base + 1).copied().unwrap_or(first);
        pending.push(first + (second - first) * fraction);
    }
}

pub fn ice_servers(value: Value) -> Result<Vec<RTCIceServer>, String> {
    let list = value.as_array().ok_or("ICE 配置必须为数组")?;
    list.iter()
        .map(|v| {
            let urls = if let Some(s) = v["urls"].as_str() {
                vec![s.to_string()]
            } else {
                serde_json::from_value::<Vec<String>>(v["urls"].clone())
                    .map_err(|_| "ICE urls 无效")?
            };
            Ok(RTCIceServer {
                urls,
                username: v["username"].as_str().unwrap_or("").into(),
                credential: v["credential"].as_str().unwrap_or("").into(),
                credential_type:
                    webrtc::ice_transport::ice_credential_type::RTCIceCredentialType::Password,
            })
        })
        .collect()
}
struct PeerOnError(Option<Arc<RTCPeerConnection>>);
impl Drop for PeerOnError {
    fn drop(&mut self) {
        if let Some(peer) = self.0.take() {
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    let _ = peer.close().await;
                });
            }
        }
    }
}
pub struct CallSession {
    pub peer: Arc<RTCPeerConnection>,
    pub call_id: String,
    pub muted: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    pub output: AudioOutput,
}
impl CallSession {
    pub async fn answer(
        offer: &str,
        id: &str,
        mode: &str,
        ice: Value,
        audio: &AudioEngineHandle,
        signal: UnboundedSender<Value>,
    ) -> Result<(Self, String), String> {
        Self::answer_with_output(
            offer,
            id,
            mode,
            ice,
            audio,
            signal,
            AudioOutput::new()?,
            Default::default(),
        )
        .await
    }
    pub async fn answer_with_output(
        offer: &str,
        id: &str,
        mode: &str,
        ice: Value,
        audio: &AudioEngineHandle,
        signal: UnboundedSender<Value>,
        output: AudioOutput,
        settings: webrtc::api::setting_engine::SettingEngine,
    ) -> Result<(Self, String), String> {
        let mut media = MediaEngine::default();
        // Opus gives wideband speech; PCMU remains as a fallback for browsers
        // that cannot offer Opus. The remote offer decides which track is sent.
        let opus_codec = RTCRtpCodecCapability {
            mime_type: webrtc::api::media_engine::MIME_TYPE_OPUS.into(),
            clock_rate: 48_000,
            channels: 2,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".into(),
            ..Default::default()
        };
        let pcmu_codec = RTCRtpCodecCapability {
            mime_type: MIME_TYPE_PCMU.into(),
            clock_rate: 8000,
            channels: 1,
            ..Default::default()
        };
        media
            .register_codec(
                RTCRtpCodecParameters {
                    capability: opus_codec.clone(),
                    payload_type: 111,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|e| e.to_string())?;
        media
            .register_codec(
                RTCRtpCodecParameters {
                    capability: pcmu_codec.clone(),
                    payload_type: 0,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|e| e.to_string())?;
        let use_opus = offer.to_ascii_lowercase().contains("opus/48000");
        let ice_servers = ice_servers(ice)?;
        let api = APIBuilder::new()
            .with_setting_engine(settings)
            .with_media_engine(media)
            .build();
        let peer = Arc::new(
            api.new_peer_connection(RTCConfiguration {
                ice_servers,
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?,
        );
        let mut cleanup = PeerOnError(Some(peer.clone()));
        let alive = Arc::new(AtomicBool::new(true));
        let muted = Arc::new(AtomicBool::new(false));
        let input_track = Arc::new(TrackLocalStaticSample::new(
            if use_opus {
                opus_codec.clone()
            } else {
                pcmu_codec.clone()
            },
            "audio".into(),
            "soundguard".into(),
        ));
        let mut opus_encoder = if use_opus {
            Some(
                opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Voip)
                    .map_err(|e| format!("Opus 编码器初始化失败：{e}"))?,
            )
        } else {
            None
        };
        let sender = peer
            .add_track(input_track.clone())
            .await
            .map_err(|e| e.to_string())?;
        let mut tasks = vec![tokio::spawn(async move {
            while sender.read_rtcp().await.is_ok() {}
        })];
        let signal2 = signal.clone();
        let call_id = id.to_string();
        peer.on_ice_candidate(Box::new(move |candidate|{let tx=signal2.clone();let id=call_id.clone();async move{if let Some(c)=candidate{if let Ok(c)=c.to_json(){let _=tx.send(json!({"type":"signal","callId":id,"payload":{"kind":"ice","candidate":c}}));}}}.boxed()}));
        let duck = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
        let remote_duck = duck.clone();
        let speaker = output.clone();
        peer.on_track(Box::new(move |track, _, _| {
            let output = speaker.clone();
            let duck = remote_duck.clone();
            async move {
                // Use the negotiated codec instead of assuming PT 0. Browsers
                // send Opus when both sides offer it; PCMU remains supported
                // for older clients. Other payload types (telephone-event,
                // comfort noise) are ignored.
                let codec = track.codec();
                let mime = codec.capability.mime_type.to_ascii_lowercase();
                let is_opus = mime == "audio/opus";
                let is_pcmu = mime.is_empty() || mime == "audio/pcmu";
                if !is_opus && !is_pcmu {
                    eprintln!("unsupported remote audio codec: {mime}");
                    return;
                }
                let codec_pt = codec.payload_type;
                let output_rate = if is_opus { 48_000 } else { 8_000 };
                let frame_samples = if is_opus { 960 } else { 160 };
                let mut opus_decoder = if is_opus {
                    match opus::Decoder::new(48_000, opus::Channels::Stereo) {
                        Ok(decoder) => Some(decoder),
                        Err(error) => {
                            eprintln!("Opus decoder init failed: {error}");
                            return;
                        }
                    }
                } else {
                    None
                };
                let mut decode = |payload: &[u8]| -> Vec<f32> {
                    if let Some(decoder) = opus_decoder.as_mut() {
                        let mut interleaved = vec![0.0_f32; 5760 * 2];
                        match decoder.decode_float(payload, &mut interleaved, false) {
                            Ok(frames) => (0..frames)
                                .map(|index| {
                                    (interleaved[index * 2] + interleaved[index * 2 + 1]) * 0.5
                                })
                                .collect(),
                            Err(_) => Vec::new(),
                        }
                    } else {
                        mulaw_to_pcm(payload)
                    }
                };
                let debug = std::env::var_os("SOUNDGUARD_RTC_DEBUG").is_some();
                let started = Instant::now();
                let mut ticker = tokio::time::interval(Duration::from_millis(20));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut buffer: HashMap<u16, Vec<u8>> = HashMap::new();
                let mut next_seq: Option<u16> = None;
                let mut last_progress = Instant::now();
                let mut ssrc: Option<u32> = None;
                let mut emitted = 0u64;
                loop {
                    tokio::select! {
                        packet = track.read_rtp() => {
                            let Ok((packet, _)) = packet else { break };
                            // Ignore telephone-event / comfort-noise packets,
                            // which would otherwise be decoded as loud noise.
                            if packet.header.payload_type != codec_pt {
                                continue;
                            }
                            if ssrc.is_some_and(|current| current != packet.header.ssrc) {
                                buffer.clear();
                                next_seq = None;
                                last_progress = Instant::now();
                            }
                            ssrc = Some(packet.header.ssrc);
                            let seq = packet.header.sequence_number;
                            let expected = *next_seq.get_or_insert(seq);
                            if seq_older(seq, expected) {
                                // Reordered before the first played frame:
                                // rewind instead of dropping the start of speech.
                                // Bound the rewind so one stale packet cannot
                                // pin the buffer hundreds of frames behind.
                                if emitted == 0 && expected.wrapping_sub(seq) <= 100 {
                                    next_seq = Some(seq);
                                    if buffer.len() < 32 {
                                        buffer.insert(seq, packet.payload.to_vec());
                                    }
                                }
                                continue;
                            }
                            if buffer.len() < 32 {
                                buffer.insert(seq, packet.payload.to_vec());
                            }
                        }
                        _ = ticker.tick() => {
                            let Some(expected) = next_seq else { continue };
                            if emitted == 0 && started.elapsed() < Duration::from_millis(40) {
                                continue;
                            }
                            if let Some(payload) = buffer.remove(&expected) {
                                let pcm = decode(&payload);
                                let rms = (pcm.iter().map(|s| s * s).sum::<f32>()
                                    / pcm.len().max(1) as f32)
                                    .sqrt();
                                if rms > 0.015 {
                                    *duck.lock().unwrap() =
                                        Instant::now() + Duration::from_millis(280);
                                }
                                output.pcm(1, output_rate, pcm);
                                emitted += 1;
                                if debug && (emitted == 1 || emitted % 50 == 0) {
                                    eprintln!(
                                        "rtc rx #{} seq={} codec={} rms={:.3}",
                                        emitted, expected, mime, rms
                                    );
                                }
                                next_seq = Some(expected.wrapping_add(1));
                                last_progress = Instant::now();
                            } else if last_progress.elapsed() >= Duration::from_millis(60) {
                                // A packet was lost: jump to the oldest buffered
                                // sequence, or advance one frame with silence.
                                if let Some((&next, _)) = buffer
                                    .iter()
                                    .min_by_key(|(&seq, _)| seq.wrapping_sub(expected))
                                {
                                    next_seq = Some(next);
                                } else {
                                    next_seq = Some(expected.wrapping_add(1));
                                    // Opus can synthesize concealment for the
                                    // lost frame; PCMU falls back to silence.
                                    let concealed = decode(&[]);
                                    let pcm = if concealed.is_empty() {
                                        vec![0.0; frame_samples]
                                    } else {
                                        concealed
                                    };
                                    output.pcm(1, output_rate, pcm);
                                }
                                last_progress = Instant::now();
                            }
                        }
                    }
                }
            }
            .boxed()
        }));
        peer.set_remote_description(
            RTCSessionDescription::offer(offer.into()).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
        let answer = peer.create_answer(None).await.map_err(|e| e.to_string())?;
        peer.set_local_description(answer)
            .await
            .map_err(|e| e.to_string())?;
        let sdp = peer.local_description().await.ok_or("没有本地 SDP")?.sdp;
        if mode != "broadcast" {
            let (tx, rx) = std::sync::mpsc::sync_channel(8);
            audio.shared.set_pcm_tap(Some(tx));
            let live = alive.clone();
            let opus_capture = use_opus;
            let (pcm_tx, mut pcm_rx) = tokio::sync::mpsc::channel::<Vec<f32>>(8);
            tasks.push(tokio::task::spawn_blocking(move || {
                let mut pending = Vec::new();
                while live.load(Ordering::SeqCst) {
                    if let Ok((rate, samples)) = rx.recv_timeout(Duration::from_millis(100)) {
                        if opus_capture {
                            append_resampled_48k(&mut pending, rate, &samples);
                            while pending.len() >= 960 {
                                let packet: Vec<_> = pending.drain(..960).collect();
                                let _ = pcm_tx.try_send(packet);
                            }
                        } else {
                            let ratio = (rate / 8000).max(1) as usize;
                            for chunk in samples.chunks_exact(ratio) {
                                pending.push(chunk.iter().sum::<f32>() / ratio as f32);
                            }
                            while pending.len() >= 160 {
                                let packet: Vec<_> = pending.drain(..160).collect();
                                let _ = pcm_tx.try_send(packet);
                            }
                        }
                    }
                }
            }));
            if use_opus {
                let mut encoder = opus_encoder.take().ok_or("Opus 编码器不可用")?;
                let mute = muted.clone();
                let out_duck = duck.clone();
                let track = input_track.clone();
                tasks.push(tokio::spawn(async move {
                    while let Some(mut pcm) = pcm_rx.recv().await {
                        if mute.load(Ordering::SeqCst)
                            || std::time::Instant::now() < *out_duck.lock().unwrap()
                        {
                            pcm.fill(0.0);
                        }
                        let mut encoded = vec![0u8; 4_000];
                        match encoder.encode_float(&pcm, &mut encoded) {
                            Ok(len) if len > 0 => {
                                let sample = webrtc::media::Sample {
                                    data: bytes::Bytes::copy_from_slice(&encoded[..len]),
                                    duration: Duration::from_millis(20),
                                    ..Default::default()
                                };
                                if track.write_sample(&sample).await.is_err() {
                                    break;
                                }
                            }
                            _ => continue,
                        }
                    }
                }));
            } else {
                let mute = muted.clone();
                let out_duck = duck.clone();
                let track = input_track.clone();
                tasks.push(tokio::spawn(async move {
                    while let Some(mut pcm) = pcm_rx.recv().await {
                        if mute.load(Ordering::SeqCst)
                            || std::time::Instant::now() < *out_duck.lock().unwrap()
                        {
                            pcm.fill(0.0);
                        }
                        let sample = webrtc::media::Sample {
                            data: bytes::Bytes::from(pcm_to_mulaw(&pcm)),
                            duration: Duration::from_millis(20),
                            ..Default::default()
                        };
                        if track.write_sample(&sample).await.is_err() {
                            break;
                        }
                    }
                }));
            }
        }
        cleanup.0 = None;
        Ok((
            Self {
                peer,
                call_id: id.into(),
                muted,
                alive,
                tasks,
                output,
            },
            sdp,
        ))
    }
    pub async fn close(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        for task in self.tasks.drain(..) {
            task.abort();
        }
        self.output.stop();
        let _ = self.peer.close().await;
    }
}
impl Drop for CallSession {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        for task in &self.tasks {
            task.abort();
        }
        self.output.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn g711_browser_vectors() {
        assert_eq!(pcm_to_mulaw(&[0.0]), vec![255]);
        assert_eq!(mulaw_to_pcm(&[255]), vec![0.0]);
        for s in [-0.8, -0.1, 0.0, 0.1, 0.8] {
            assert!((mulaw_to_pcm(&pcm_to_mulaw(&[s]))[0] - s).abs() < 0.04);
        }
    }
}

#[cfg(test)]
mod interoperability {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn negotiates_opus_with_browser_style_ice_and_transmits_rtp() {
        let mut media = MediaEngine::default();
        media.register_default_codecs().unwrap();
        use webrtc_util::vnet::{
            net::{Net, NetConfig},
            router::{Router, RouterConfig},
        };
        let router = Arc::new(tokio::sync::Mutex::new(
            Router::new(RouterConfig {
                cidr: "1.2.3.0/24".into(),
                ..Default::default()
            })
            .unwrap(),
        ));
        let mut settings = Vec::new();
        for ip in ["1.2.3.4", "1.2.3.5"] {
            let net = Arc::new(Net::new(Some(NetConfig {
                static_ips: vec![ip.into()],
                ..Default::default()
            })));
            let nic = net.get_nic().unwrap();
            router.lock().await.add_net(nic.clone()).await.unwrap();
            nic.lock().await.set_router(router.clone()).await.unwrap();
            let mut engine = webrtc::api::setting_engine::SettingEngine::default();
            engine.set_vnet(Some(net));
            settings.push(engine);
        }
        router.lock().await.start().await.unwrap();
        let api = APIBuilder::new()
            .with_setting_engine(settings.remove(0))
            .with_media_engine(media)
            .build();
        let peer = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: webrtc::api::media_engine::MIME_TYPE_OPUS.into(),
                clock_rate: 48_000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".into(),
                ..Default::default()
            },
            "browser-audio".into(),
            "browser".into(),
        ));
        peer.add_track(track.clone()).await.unwrap();
        let (received_tx, mut received) = tokio::sync::mpsc::channel(16);
        peer.on_track(Box::new(move |track, _, _| {
            let tx = received_tx.clone();
            async move {
                while let Ok((packet, _)) = track.read_rtp().await {
                    if tx.send(packet.payload.to_vec()).await.is_err() {
                        break;
                    }
                }
            }
            .boxed()
        }));
        let mut gathering = peer.gathering_complete_promise().await;
        let offer = peer.create_offer(None).await.unwrap();
        peer.set_local_description(offer).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(3), gathering.recv()).await;
        let audio = AudioEngineHandle::start(90.0);
        let (tx, mut signals) = tokio::sync::mpsc::unbounded_channel();
        let (capture, captured) = AudioOutput::test_capture();
        let (mut call, answer) = CallSession::answer_with_output(
            &peer.local_description().await.unwrap().sdp,
            "test-call",
            "intercom",
            json!([]),
            &audio,
            tx,
            capture,
            settings.remove(0),
        )
        .await
        .unwrap();
        assert!(answer.to_ascii_lowercase().contains("opus/48000"), "answer lacks Opus: {answer}");
        assert!(!answer.contains("a=rtpmap:8 PCMA"));
        assert!(!answer.contains("a=rtpmap:101"), "answer unexpectedly offers telephone-event: {answer}");
        peer.set_remote_description(RTCSessionDescription::answer(answer).unwrap())
            .await
            .unwrap();
        let peer_for_ice = peer.clone();
        let ice_task = tokio::spawn(async move {
            while let Some(v) = signals.recv().await {
                if let Ok(c) = serde_json::from_value(v["payload"]["candidate"].clone()) {
                    let _ = peer_for_ice.add_ice_candidate(c).await;
                }
            }
        });
        for _ in 0..100 {
            audio.test_signal();
            if call.peer.connection_state()
                == webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        for _ in 0..20 {
            audio.test_signal();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Browser -> native: send a 440 Hz Opus tone through the negotiated
        // track and require the native call output to receive non-silent audio.
        let tone: Vec<f32> = (0..960)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let mut opus_encoder =
            opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let mut encoded = vec![0u8; 4_000];
        let encoded_len = opus_encoder.encode_float(&tone, &mut encoded).unwrap();
        encoded.truncate(encoded_len);
        for _ in 0..10 {
            let sample = webrtc::media::Sample {
                data: bytes::Bytes::from(encoded.clone()),
                duration: Duration::from_millis(20),
                ..Default::default()
            };
            let _ = track.write_sample(&sample).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut captured_pcm = None;
        for _ in 0..50 {
            if let Ok(pcm) = captured.try_recv() {
                captured_pcm = Some(pcm);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let captured_pcm = captured_pcm.expect("native did not receive browser Opus audio");
        let rms = (captured_pcm.iter().map(|s| s * s).sum::<f32>()
            / captured_pcm.len().max(1) as f32)
            .sqrt();
        assert!(rms > 0.1, "native captured silent audio (rms {rms})");
        let mut opus_decoder = opus::Decoder::new(48_000, opus::Channels::Stereo).unwrap();
        let mut best_rms = 0.0_f32;
        for _ in 0..20 {
            let packet = match tokio::time::timeout(Duration::from_secs(2), received.recv()).await {
                Ok(Some(packet)) => packet,
                _ => break,
            };
            let mut decoded = vec![0.0_f32; 5760 * 2];
            let Ok(frames) = opus_decoder.decode_float(&packet, &mut decoded, false) else {
                continue;
            };
            if frames == 0 {
                continue;
            }
            let rms = (0..frames)
                .map(|index| decoded[index * 2] * decoded[index * 2])
                .sum::<f32>()
                / frames as f32;
            best_rms = best_rms.max(rms.sqrt());
            if best_rms > 0.05 {
                break;
            }
        }
        assert!(
            best_rms > 0.05,
            "browser received silent Opus audio (best rms {best_rms})"
        );
        call.close().await;
        audio.shared.set_pcm_tap(None);
        ice_task.abort();
        audio.stop();
        router.lock().await.stop().await.unwrap();
    }
}
