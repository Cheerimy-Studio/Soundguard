//! NVIDIA Background Noise Removal (BNR) NIM client.
//!
//! Protocol (build.nvidia.com/nvidia/bnr, hosted on NVCF):
//! - gRPC over TLS to `grpc.nvcf.nvidia.com:443`
//! - service `nvidia.ai4m.bnr.v1.BNR`, bidirectional streaming RPC `EnhanceAudio`
//! - metadata: `authorization: Bearer <nvapi-key>` and `function-id`
//! - request stream: one `EnhanceAudioConfig` message, then 64 KiB chunks of
//!   little-endian **float32 PCM samples** (not WAV bytes)
//! - response stream: chunks of float32 PCM samples at the same sample rate
//!
//! The gRPC wire frames and protobuf messages are hand-rolled with `prost`
//! derives so the server does not need `protoc` or a tonic codegen step.

use prost::Message as _;

pub const DEFAULT_FUNCTION_ID: &str = "b415e29a-bf92-475c-aefe-69f9ee8ca72e";
pub const NVCF_URI: &str = "https://grpc.nvcf.nvidia.com:443";
const METHOD_PATH: &str = "/nvidia.ai4m.bnr.v1.BNR/EnhanceAudio";
const CHUNK_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, PartialEq, prost::Message)]
pub struct EnhanceAudioConfig {
    #[prost(float, optional, tag = "1")]
    pub intensity_ratio: Option<f32>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct EnhanceAudioRequest {
    #[prost(oneof = "enhance_audio_request::StreamInput", tags = "1, 2")]
    pub stream_input: Option<enhance_audio_request::StreamInput>,
}

pub mod enhance_audio_request {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum StreamInput {
        #[prost(bytes, tag = "1")]
        AudioStreamData(#[prost(bytes)] Vec<u8>),
        #[prost(message, tag = "2")]
        Config(super::EnhanceAudioConfig),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct EnhanceAudioResponse {
    #[prost(oneof = "enhance_audio_response::StreamOutput", tags = "1, 2")]
    pub stream_output: Option<enhance_audio_response::StreamOutput>,
}

pub mod enhance_audio_response {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum StreamOutput {
        #[prost(bytes, tag = "1")]
        AudioStreamData(#[prost(bytes)] Vec<u8>),
        #[prost(message, tag = "2")]
        Config(super::EnhanceAudioConfig),
    }
}

/// gRPC message frame: 1-byte compressed flag + u32 big-endian length.
pub fn grpc_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(0u8);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Incremental parser for the response gRPC frame stream.
#[derive(Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    pub fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > MAX_RESPONSE_BYTES {
            // Guard against runaway servers; the caller treats this as an error.
            self.buffer.clear();
            self.buffer.push(0xFF);
        }
    }

    /// Pop the next complete protobuf payload, if any.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        if self.buffer.len() < 5 {
            return None;
        }
        let length = u32::from_be_bytes(self.buffer[1..5].try_into().ok()?) as usize;
        if self.buffer.len() < 5 + length {
            return None;
        }
        let payload = self.buffer[5..5 + length].to_vec();
        self.buffer.drain(..5 + length);
        Some(payload)
    }
}

/// Parse a `EnhanceAudioResponse` payload and return its audio chunk, if any.
pub fn response_audio(payload: &[u8]) -> Option<Vec<u8>> {
    let response = EnhanceAudioResponse::decode(payload).ok()?;
    match response.stream_output? {
        enhance_audio_response::StreamOutput::AudioStreamData(data) => Some(data),
        enhance_audio_response::StreamOutput::Config(_) => None,
    }
}

/// Minimal PCM WAV (16-bit, mono) writer.
pub fn encode_wav_mono16(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_size = samples.len() * 2;
    let mut wav = Vec::with_capacity(44 + data_size);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_size as u32).to_le_bytes()));
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_size as u32).to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Extract mono i16 samples from a validated 16-bit PCM WAV. Multi-channel
/// input is averaged; the sample rate is returned alongside.
pub fn decode_wav_mono16(bytes: &[u8]) -> Result<(Vec<i16>, u32), String> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("不是有效的 WAV 文件".to_string());
    }
    let u32_at = |offset: usize| -> Result<u32, String> {
        bytes
            .get(offset..offset + 4)
            .and_then(|slice| slice.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| "WAV 头部不完整".to_string())
    };
    let u16_at = |offset: usize| -> Result<u16, String> {
        bytes
            .get(offset..offset + 2)
            .and_then(|slice| slice.try_into().ok())
            .map(u16::from_le_bytes)
            .ok_or_else(|| "WAV 头部不完整".to_string())
    };
    if u32_at(4)? as usize + 8 != bytes.len() {
        return Err("WAV 长度不匹配".to_string());
    }
    let mut offset = 12usize;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut data: Option<&[u8]> = None;
    while offset + 8 <= bytes.len() {
        let tag = &bytes[offset..offset + 4];
        let length = u32_at(offset + 4)? as usize;
        let start = offset + 8;
        let end = start + length;
        if end > bytes.len() {
            return Err("WAV 数据块越界".to_string());
        }
        match tag {
            b"fmt " => {
                if length < 16 || u16_at(start)? != 1 {
                    return Err("仅支持 16-bit PCM WAV".to_string());
                }
                channels = u16_at(start + 2)?;
                sample_rate = u32_at(start + 4)?;
            }
            b"data" => data = Some(&bytes[start..end]),
            _ => {}
        }
        offset = end + (length % 2);
    }
    let data = data.ok_or_else(|| "WAV 缺少 data 块".to_string())?;
    if channels == 0 || data.len() % (channels as usize * 2) != 0 {
        return Err("WAV 数据长度与声道数不匹配".to_string());
    }
    let frames: Vec<i16> = data
        .chunks_exact(channels as usize * 2)
        .map(|frame| {
            let sum: i32 = frame
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes(pair.try_into().unwrap()) as i32)
                .sum();
            (sum / channels as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect();
    Ok((frames, sample_rate))
}


fn request_frames(samples_f32: &[f32], intensity: f32) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let config = EnhanceAudioRequest {
        stream_input: Some(enhance_audio_request::StreamInput::Config(
            EnhanceAudioConfig {
                intensity_ratio: Some(intensity.clamp(0.0, 1.0)),
            },
        )),
    };
    frames.push(grpc_frame(&config.encode_to_vec()));
    let bytes: Vec<u8> = samples_f32.iter().flat_map(|s| s.to_le_bytes()).collect();
    for chunk in bytes.chunks(CHUNK_BYTES) {
        let request = EnhanceAudioRequest {
            stream_input: Some(enhance_audio_request::StreamInput::AudioStreamData(
                chunk.to_vec(),
            )),
        };
        frames.push(grpc_frame(&request.encode_to_vec()));
    }
    frames
}

/// Build the full request byte stream (config + all audio chunks). Used by the
/// streaming sender.
pub fn build_request_bytes(samples_f32: &[f32], intensity: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for frame in request_frames(samples_f32, intensity) {
        bytes.extend_from_slice(&frame);
    }
    bytes
}

fn is_retryable(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("unavailable")
        || lower.contains("deadline")
        || lower.contains("timeout")
        || lower.contains("temporarily")
        || lower.contains("429")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("connection")
        || lower.contains("发送失败")
}

/// Call the NVIDIA BNR NIM with automatic retry on transient failures.
/// `wav` must be a 16-bit PCM WAV; the result is a 48 kHz mono 16-bit WAV.
pub async fn denoise_wav_with_retry(
    wav: &[u8],
    api_key: &str,
    function_id: &str,
    intensity: f32,
) -> Result<Vec<u8>, String> {
    let mut last_error = String::new();
    let mut delay_secs = 1u64;
    for attempt in 1..=3 {
        match denoise_wav_once(wav, api_key, function_id, intensity).await {
            Ok(result) => return Ok(result),
            Err(error) => {
                last_error = error;
                if attempt == 3 || !is_retryable(&last_error) {
                    break;
                }
                tracing::warn!(
                    "NVIDIA BNR attempt {attempt} failed: {last_error}; retrying in {delay_secs}s"
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                delay_secs *= 3;
            }
        }
    }
    Err(format!("NVIDIA 降噪失败（已自动重试 3 次）：{last_error}"))
}

async fn denoise_wav_once(
    wav: &[u8],
    api_key: &str,
    function_id: &str,
    intensity: f32,
) -> Result<Vec<u8>, String> {
    if api_key.trim().is_empty() {
        return Err("未配置 NVIDIA API Key".to_string());
    }
    let (samples_i16, sample_rate) = decode_wav_mono16(wav)?;
    if sample_rate != 48_000 {
        return Err(format!(
            "证据采样率为 {sample_rate} Hz，BNR 仅支持 48 kHz"
        ));
    }
    let samples_f32: Vec<f32> = samples_i16
        .iter()
        .map(|&sample| f32::from(sample) / 32768.0)
        .collect();
    if samples_f32.is_empty() {
        return Err("证据音频为空".to_string());
    }

    let request_bytes = build_request_bytes(&samples_f32, intensity);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|error| format!("HTTP 客户端初始化失败: {error}"))?;

    let response = client
        .post(format!("{NVCF_URI}{METHOD_PATH}"))
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("authorization", format!("Bearer {}", api_key.trim()))
        .header("function-id", function_id)
        .body(request_bytes)
        .send()
        .await
        .map_err(|error| format!("连接 NVIDIA 失败: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("NVIDIA 返回 {status}: {}", truncate(&body, 300)));
    }
    if let Some(grpc_status) = response.headers().get("grpc-status") {
        let code = grpc_status.to_str().unwrap_or_default().to_string();
        if code != "0" {
            let message = response
                .headers()
                .get("grpc-message")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("gRPC 调用被拒绝")
                .to_string();
            return Err(format!("NVIDIA gRPC 错误 {code}: {message}"));
        }
    }

    let mut reader = FrameReader::default();
    let mut audio_f32: Vec<f32> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|error| format!("读取 NVIDIA 响应失败: {error}"))?;
        reader.push(&chunk);
        while let Some(payload) = reader.next_frame() {
            if let Some(data) = response_audio(&payload) {
                for pair in data.chunks_exact(4) {
                    audio_f32.push(f32::from_le_bytes(pair.try_into().unwrap()));
                }
            }
        }
    }
    if audio_f32.is_empty() {
        return Err("NVIDIA 未返回任何降噪音频".to_string());
    }

    let out_i16: Vec<i16> = audio_f32
        .iter()
        .map(|&sample| (sample * 32768.0).clamp(-32768.0, 32767.0).round() as i16)
        .collect();
    Ok(encode_wav_mono16(&out_i16, 48_000))
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let cut: String = text.chars().take(limit).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_frame_encodes_optional_float() {
        let request = EnhanceAudioRequest {
            stream_input: Some(enhance_audio_request::StreamInput::Config(
                EnhanceAudioConfig {
                    intensity_ratio: Some(0.5),
                },
            )),
        };
        let bytes = request.encode_to_vec();
        // field 2 (config), wire type 2 -> tag 0x12; inner: field 1 fixed32
        // carrying 0.5f32 = LE 00 00 00 3F.
        assert_eq!(&bytes[..7], &[0x12, 0x05, 0x0d, 0, 0, 0, 0x3f][..]);
        let round = EnhanceAudioRequest::decode(&bytes[..]).unwrap();
        assert_eq!(round, request);
    }

    #[test]
    fn grpc_frames_roundtrip_through_reader() {
        let payloads: Vec<Vec<u8>> = vec![vec![1, 2, 3], vec![4; 70000], vec![]];
        let mut bytes = Vec::new();
        for payload in &payloads {
            bytes.extend_from_slice(&grpc_frame(payload));
        }
        let mut reader = FrameReader::default();
        let mut parsed = Vec::new();
        // Feed in awkward small chunks to exercise the incremental parser.
        for chunk in bytes.chunks(777) {
            reader.push(chunk);
            while let Some(payload) = reader.next_frame() {
                parsed.push(payload);
            }
        }
        assert_eq!(parsed, payloads);
    }

    #[test]
    fn response_audio_extracts_bytes_and_ignores_config() {
        let audio = EnhanceAudioResponse {
            stream_output: Some(enhance_audio_response::StreamOutput::AudioStreamData(
                vec![9, 8, 7],
            )),
        };
        assert_eq!(response_audio(&audio.encode_to_vec()).unwrap(), vec![9, 8, 7]);
        let config = EnhanceAudioResponse {
            stream_output: Some(enhance_audio_response::StreamOutput::Config(
                EnhanceAudioConfig { intensity_ratio: None },
            )),
        };
        assert!(response_audio(&config.encode_to_vec()).is_none());
    }

    #[test]
    fn wav_roundtrip_with_channel_folding() {
        let wav = encode_wav_mono16(&[0, 16384, -16384, 32767], 48_000);
        let (samples, rate) = decode_wav_mono16(&wav).unwrap();
        assert_eq!(rate, 48_000);
        assert_eq!(samples, vec![0, 16384, -16384, 32767]);
    }

    #[test]
    fn retry_classifier() {
        assert!(is_retryable("连接 NVIDIA 失败: timeout"));
        assert!(is_retryable("NVIDIA 返回 503 Service Unavailable"));
        assert!(!is_retryable("NVIDIA gRPC 错误 16: invalid key"));
        assert!(!is_retryable("未配置 NVIDIA API Key"));
    }
}
