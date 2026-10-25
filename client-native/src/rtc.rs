//! WebRTC intercom with G711 μ-law codec + WASAPI audio playback.

use std::sync::Arc;
use std::time::Duration;

use webrtc::api::media_engine::MIME_TYPE_PCMA;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

const MULAW_BIAS: i32 = 0x84;
const MULAW_CLIP: i32 = 32635;

pub fn pcm_to_mulaw(pcm: &[f32]) -> Vec<u8> {
    pcm.iter().map(|&s| {
        let raw = (s.clamp(-1.0, 1.0) * 32767.0) as i16 as i32;
        let sign = if raw < 0 { 0x80i32 } else { 0 };
        let mag = raw.abs().min(MULAW_CLIP) + MULAW_BIAS;
        let exp = if mag < 0x100 { 0 }
            else if mag < 0x200 { 1 }
            else if mag < 0x400 { 2 }
            else if mag < 0x800 { 3 }
            else if mag < 0x1000 { 4 }
            else if mag < 0x2000 { 5 }
            else if mag < 0x4000 { 6 }
            else { 7 };
        let man = (mag >> (exp + 3)) & 0x0F;
        (!(sign | (exp << 4) | man)) as u8
    }).collect()
}

pub fn mulaw_to_pcm(data: &[u8]) -> Vec<f32> {
    data.iter().map(|&b| {
        let u = (!(b as i32)) & 0xFF;
        let sign = if u & 0x80 != 0 { -1i32 } else { 1i32 };
        let exp = (u >> 4) & 0x07;
        let man = u & 0x0F;
        let mag = (((man << 3) + MULAW_BIAS) << exp) - MULAW_BIAS;
        (sign * mag) as f32 / 32768.0
    }).collect()
}

pub struct CallSession {
    pub peer: Arc<webrtc::peer_connection::RTCPeerConnection>,
    pub send_track: Arc<TrackLocalStaticSample>,
    pub call_id: String,
    pub mode: String,
}

impl CallSession {
    pub async fn new_answerer(offer_sdp: &str, call_id: &str, mode: &str) -> Result<(Self, String), String> {
        let mut media = webrtc::api::media_engine::MediaEngine::default();
        media.register_default_codecs().map_err(|e| format!("codecs: {e}"))?;
        let api = APIBuilder::new().with_media_engine(media).build();
        let peer = api.new_peer_connection(RTCConfiguration::default()).await.map_err(|e| format!("peer: {e}"))?;
        let codec = RTCRtpCodecCapability { mime_type: MIME_TYPE_PCMA.to_string(), clock_rate: 8000, channels: 1, ..Default::default() };
        let send_track = Arc::new(TrackLocalStaticSample::new(codec, "audio".to_string(), "soundguard".to_string()));
        peer.add_track(send_track.clone()).await.map_err(|e| format!("track: {e}"))?;
        let offer = RTCSessionDescription::offer(offer_sdp.to_string()).map_err(|e| format!("offer: {e}"))?;
        peer.set_remote_description(offer).await.map_err(|e| format!("remote: {e}"))?;
        let answer = peer.create_answer(None).await.map_err(|e| format!("answer: {e}"))?;
        peer.set_local_description(answer.clone()).await.map_err(|e| format!("local: {e}"))?;
        Ok((Self { peer: Arc::new(peer), send_track, call_id: call_id.to_string(), mode: mode.to_string() }, answer.sdp))
    }

    pub async fn send_audio_48k(&self, pcm_48k: &[f32]) {
        let pcm_8k: Vec<f32> = pcm_48k.iter().step_by(6).copied().collect();
        let mulaw = pcm_to_mulaw(&pcm_8k);
        if mulaw.is_empty() { return; }
        let sample = webrtc::media::Sample {
            data: bytes::Bytes::from(mulaw),
            duration: Duration::from_millis(20),
            ..Default::default()
        };
        let _ = self.send_track.write_sample(&sample).await;
    }

    #[allow(dead_code)]
    pub async fn close(&self) {
        let _ = self.peer.close().await;
    }
}

pub struct AudioOutput {
    buffer: Arc<std::sync::Mutex<Vec<f32>>>,
}

impl AudioOutput {
    pub fn new() -> Result<Self, String> {
        let buffer = Arc::new(std::sync::Mutex::new(Vec::<f32>::with_capacity(80000)));
        let buf = buffer.clone();
        std::thread::Builder::new().name("sg-speaker".to_string()).spawn(move || {
            let _ = wasapi::initialize_mta();
            if let Err(e) = speaker_loop(buf) { eprintln!("speaker: {e}"); }
        }).map_err(|e| format!("speaker thread: {e}"))?;
        Ok(Self { buffer })
    }

    pub fn push_mulaw(&self, mulaw_data: &[u8]) {
        let pcm = mulaw_to_pcm(mulaw_data);
        self.buffer.lock().unwrap().extend_from_slice(&pcm);
        let mut b = self.buffer.lock().unwrap();
        if b.len() > 80000 { let excess = b.len() - 40000; b.drain(..excess); }
    }
}

fn speaker_loop(buffer: Arc<std::sync::Mutex<Vec<f32>>>) -> Result<(), String> {
    use wasapi::{DeviceEnumerator, Direction, Role, SampleType, StreamMode, WaveFormat};
    let enum_ = DeviceEnumerator::new().map_err(|e| format!("enum: {e}"))?;
    let dev = enum_.get_default_device_for_role(&Direction::Render, &Role::Communications)
        .or_else(|_| enum_.get_default_device(&Direction::Render))
        .map_err(|e| format!("speaker: {e}"))?;
    let mut ac = dev.get_iaudioclient().map_err(|e| format!("cl: {e}"))?;
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
    let (_, mp) = ac.get_device_period().map_err(|e| format!("pd: {e}"))?;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: mp };
    ac.initialize_client(&fmt, &Direction::Render, &mode).map_err(|e| format!("in: {e}"))?;
    let ev = ac.set_get_eventhandle().map_err(|e| format!("ev: {e}"))?;
    let render = ac.get_audiorenderclient().map_err(|e| format!("rc: {e}"))?;
    ac.start_stream().map_err(|e| format!("st: {e}"))?;
    let fb = fmt.get_blockalign() as usize;
    let mut out: Vec<u8> = Vec::with_capacity(fb * 2048);
    loop {
        if ev.wait_for_event(100).is_err() { continue; }
        let mut s8: Vec<f32> = Vec::new();
        { let mut b = buffer.lock().unwrap(); let take = (b.len()/160)*160; if take>0 { s8 = b.drain(..take).collect(); } }
        out.clear();
        if s8.is_empty() {
            out.resize(fb * 240, 0);
        } else {
            for &s in &s8 {
                for _ in 0..6 {
                    let by = s.to_le_bytes();
                    out.extend_from_slice(&by);
                    out.extend_from_slice(&by);
                }
            }
        }
        if !out.is_empty() { let n = out.len() / fb; let _ = render.write_to_device(n, &out, None); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mulaw_roundtrip() {
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 / 500.0 - 1.0) * 0.8).collect();
        let encoded = pcm_to_mulaw(&input);
        let decoded = mulaw_to_pcm(&encoded);
        assert_eq!(input.len(), decoded.len());
        for (o, d) in input.iter().zip(decoded.iter()) {
            assert!((o - d).abs() < 0.15, "mulaw err: {} at {}", (o - d).abs(), o);
        }
    }
}
