//! Browser-compatible PCMU (G.711 μ-law), with ICE/TURN and bounded media tasks.
use crate::{audio::AudioEngineHandle, playback::AudioOutput};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
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
        let codec = RTCRtpCodecCapability {
            mime_type: MIME_TYPE_PCMU.into(),
            clock_rate: 8000,
            channels: 1,
            ..Default::default()
        };
        media
            .register_codec(
                RTCRtpCodecParameters {
                    capability: codec.clone(),
                    payload_type: 0,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|e| e.to_string())?;
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
            codec,
            "audio".into(),
            "soundguard".into(),
        ));
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
                while let Ok((packet, _)) = track.read_rtp().await {
                    // Only PT 0 is PCMU. Ignore telephone-event / comfort-noise
                    // packets, which would otherwise be decoded as loud noise.
                    if packet.header.payload_type != 0 {
                        continue;
                    }
                    let pcm = mulaw_to_pcm(&packet.payload);
                    let rms =
                        (pcm.iter().map(|s| s * s).sum::<f32>() / pcm.len().max(1) as f32).sqrt();
                    if rms > 0.015 {
                        *duck.lock().unwrap() =
                            std::time::Instant::now() + Duration::from_millis(280);
                    }
                    output.pcm(1, 8000, pcm);
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
            let (pcm_tx, mut pcm_rx) = tokio::sync::mpsc::channel::<Vec<f32>>(8);
            tasks.push(tokio::task::spawn_blocking(move || {
                let mut pending = Vec::new();
                while live.load(Ordering::SeqCst) {
                    if let Ok((rate, samples)) = rx.recv_timeout(Duration::from_millis(100)) {
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
            }));
            let mute = muted.clone();
            tasks.push(tokio::spawn(async move {
                while let Some(mut pcm) = pcm_rx.recv().await {
                    if mute.load(Ordering::SeqCst)
                        || std::time::Instant::now() < *duck.lock().unwrap()
                    {
                        pcm.fill(0.0);
                    }
                    let sample = webrtc::media::Sample {
                        data: bytes::Bytes::from(pcm_to_mulaw(&pcm)),
                        duration: Duration::from_millis(20),
                        ..Default::default()
                    };
                    if input_track.write_sample(&sample).await.is_err() {
                        break;
                    }
                }
            }));
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
    async fn negotiates_pcmu_with_browser_style_ice_and_transmits_rtp() {
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
        let peer = api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_PCMU.into(),
                clock_rate: 8000,
                channels: 1,
                ..Default::default()
            },
            "browser-audio".into(),
            "browser".into(),
        ));
        peer.add_track(track).await.unwrap();
        let (received_tx, mut received) = tokio::sync::mpsc::channel(1);
        peer.on_track(Box::new(move |track, _, _| {
            let tx = received_tx.clone();
            async move {
                if let Ok((packet, _)) = track.read_rtp().await {
                    let _ = tx.send(packet.payload.to_vec()).await;
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
        let (mut call, answer) = CallSession::answer_with_output(
            &peer.local_description().await.unwrap().sdp,
            "test-call",
            "intercom",
            json!([]),
            &audio,
            tx,
            AudioOutput::test_null(),
            settings.remove(0),
        )
        .await
        .unwrap();
        assert!(answer.contains("PCMU/8000"));
        assert!(!answer.contains("a=rtpmap:8 PCMA"));
        assert!(!answer.to_ascii_lowercase().contains("opus"), "answer unexpectedly offers Opus: {answer}");
        assert!(!answer.contains("a=rtpmap:101"), "answer unexpectedly offers telephone-event: {answer}");
        peer.set_remote_description(RTCSessionDescription::answer(answer).unwrap())
            .await
            .unwrap();
        let ice_task = tokio::spawn(async move {
            while let Some(v) = signals.recv().await {
                if let Ok(c) = serde_json::from_value(v["payload"]["candidate"].clone()) {
                    let _ = peer.add_ice_candidate(c).await;
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
        let packet = tokio::time::timeout(Duration::from_secs(4), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(packet.len(), 160);
        call.close().await;
        audio.shared.set_pcm_tap(None);
        ice_task.abort();
        audio.stop();
        router.lock().await.stop().await.unwrap();
    }
}
