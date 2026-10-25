//! All networking runs off the UI thread. Each binding has its own cancellable session.
use crate::{audio::AudioEngineHandle, config::ClientConfig, rtc::CallSession};
use chrono::{DateTime, Timelike, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::{connect_async, tungstenite::Message};
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    pub desired_monitoring: bool,
    pub manual_monitoring: Option<bool>,
    pub threshold_db: f64,
    pub alert_cooldown_secs: u64,
    pub auto_record_evidence: bool,
    pub device_name: String,
    pub server_time: Option<DateTime<Utc>>,
    pub server_utc_offset_secs: i32,
    pub notification_enabled: bool,
    pub notification_title: String,
    pub notification_content: String,
    pub denoise_mode: String,
    pub voice_enhance: bool,
    pub war_mode_enabled: bool,
    pub war_mode_start: String,
    pub war_mode_end: String,
    pub capture_start: String,
    pub capture_end: String,
    pub totp_secret: String,
    pub has_fixed_password: bool,
    pub ice_servers: Value,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            desired_monitoring: true,
            manual_monitoring: None,
            threshold_db: 65.0,
            alert_cooldown_secs: 120,
            auto_record_evidence: true,
            device_name: "现场采集节点".into(),
            server_time: None,
            server_utc_offset_secs: 0,
            notification_enabled: false,
            notification_title: String::new(),
            notification_content: String::new(),
            denoise_mode: "off".into(),
            voice_enhance: false,
            war_mode_enabled: false,
            war_mode_start: "07:30".into(),
            war_mode_end: "08:00".into(),
            capture_start: String::new(),
            capture_end: String::new(),
            totp_secret: String::new(),
            has_fixed_password: false,
            ice_servers: json!([]),
        }
    }
}
pub use crate::schedule::within_window;
impl Policy {
    pub fn minute(&self) -> u32 {
        let now = self.server_time.unwrap_or_else(Utc::now)
            + chrono::Duration::seconds(self.server_utc_offset_secs.into());
        now.hour() * 60 + now.minute()
    }
    pub fn war(&self) -> bool {
        self.war_mode_enabled
            && within_window(&self.war_mode_start, &self.war_mode_end, self.minute())
    }
    pub fn capturing(&self) -> bool {
        self.manual_monitoring.unwrap_or(self.desired_monitoring)
            && within_window(&self.capture_start, &self.capture_end, self.minute())
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    pub id: String,
    pub mode: String,
    pub started_at: String,
    pub duration_secs: u32,
    pub has_audio: bool,
}
#[derive(Debug, Clone)]
pub struct NetState {
    pub connected: bool,
    pub signaling: bool,
    pub policy: Policy,
    pub last_error: String,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub pending_reports: usize,
    pub reports_sent: u64,
    pub call_state: String,
    pub call_mode: String,
    pub call_started: Option<Instant>,
    pub incoming: Option<Value>,
    pub records: Vec<CallRecord>,
    pub auth_result: Option<(u64, bool, String)>,
    pub exit_ready: bool,
}
impl Default for NetState {
    fn default() -> Self {
        Self {
            connected: false,
            signaling: false,
            policy: Policy::default(),
            last_error: String::new(),
            last_heartbeat: None,
            pending_reports: 0,
            reports_sent: 0,
            call_state: "idle".into(),
            call_mode: String::new(),
            call_started: None,
            incoming: None,
            records: Vec::new(),
            auth_result: None,
            exit_ready: false,
        }
    }
}
pub enum Command {
    Hangup,
    Accept,
    Decline,
    Mute(bool),
    AutoAnswer(bool),
    RefreshCalls,
    Play(String),
    StopPlayback,
    Verify(u64, String),
    Quit,
}
pub struct NetManager {
    state: Arc<Mutex<NetState>>,
    stop: Option<watch::Sender<bool>>,
    commands: Option<mpsc::UnboundedSender<Command>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl NetManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(NetState::default())),
            stop: None,
            commands: None,
            worker: None,
        }
    }
    pub fn start(&mut self, config: ClientConfig, audio: AudioEngineHandle) {
        let previous = self.worker.take();
        self.stop();
        self.state = Arc::new(Mutex::new(NetState::default()));
        let state = self.state.clone();
        let (tx, rx) = watch::channel(false);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        self.stop = Some(tx);
        self.commands = Some(cmd_tx);
        self.worker = Some(std::thread::spawn(move || {
            if let Some(previous) = previous {
                let _ = previous.join();
            }
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(v) => v,
                Err(e) => {
                    state.lock().unwrap().last_error = e.to_string();
                    return;
                }
            };
            runtime.block_on(run(config, audio, state, rx, cmd_rx));
        }));
    }
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(true);
        }
        self.commands = None;
    }
    pub fn send(&self, c: Command) -> bool {
        self.commands.as_ref().is_some_and(|tx| tx.send(c).is_ok())
    }
    pub fn poll(&self) -> NetState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
impl Drop for NetManager {
    fn drop(&mut self) {
        self.stop();
    }
}
async fn request<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    c: &ClientConfig,
    path: &str,
    body: Option<Value>,
) -> Result<T, String> {
    let url = format!("{}{}", c.server_url, path);
    let builder = if let Some(body) = body {
        http.post(url).json(&body)
    } else {
        http.get(url)
    };
    let response = builder
        .header("X-Device-Token", &c.device_token)
        .send()
        .await
        .map_err(|e| format!("无法连接服务器：{e}"))?;
    let status = response.status();
    if !status.is_success() {
        let error: Value = response.json().await.unwrap_or_default();
        return Err(format!(
            "HTTP {}: {}",
            status.as_u16(),
            error["error"].as_str().unwrap_or("服务器拒绝请求")
        ));
    }
    response
        .json()
        .await
        .map_err(|e| format!("服务器响应格式错误：{e}"))
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    device_id: String,
    avg_db: f64,
    peak_db: f64,
    min_db: f64,
    duration_secs: u32,
    sample_count: u32,
    recorded_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_base64: Option<String>,
}
fn ws_url(c: &ClientConfig) -> Result<String, String> {
    let mut url = reqwest::Url::parse(&format!("{}/api/device/ws", c.server_url))
        .map_err(|e| e.to_string())?;
    url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
        .map_err(|_| "信令协议无效")?;
    url.query_pairs_mut()
        .append_pair("deviceId", &c.device_id)
        .append_pair("token", &c.device_token);
    Ok(url.to_string())
}
async fn run(
    c: ClientConfig,
    audio: AudioEngineHandle,
    state: Arc<Mutex<NetState>>,
    mut stop: watch::Receiver<bool>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(v) => v,
        Err(e) => {
            state.lock().unwrap().last_error = e.to_string();
            return;
        }
    };
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut meter = tokio::time::interval(Duration::from_secs(1));
    let mut reconnect = tokio::time::interval(Duration::from_secs(3));
    let mut records_tick = tokio::time::interval(Duration::from_secs(30));
    for t in [
        &mut heartbeat,
        &mut meter,
        &mut reconnect,
        &mut records_tick,
    ] {
        t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    }
    let mut socket: Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > = None;
    let mut policy = Policy::default();
    let mut server_anchor = Instant::now();
    let mut server_timestamp = Utc::now();
    let mut auto_answer = c.auto_answer;
    let mut authorized_once = false;
    let mut early_ice = std::collections::HashMap::<String, Vec<Value>>::new();
    let mut reports = VecDeque::<Report>::new();
    let (mut call, mut pending): (Option<CallSession>, Option<(Value, Instant)>) = (None, None);
    let (signals, mut outbound) = mpsc::unbounded_channel::<Value>();
    let mut player: Option<crate::playback::AudioOutput> = None;
    macro_rules! end_call {
        () => {{
            if let Some(mut current) = call.take() {
                current.close().await;
            }
            audio.shared.set_pcm_tap(None);
            audio.shared.set_suspended(false);
            let mut s = state.lock().unwrap();
            s.call_state = "idle".into();
            s.call_started = None;
            s.incoming = None;
            let _ = pending.take();
        }};
    }
    loop {
        tokio::select! {
         _=stop.changed()=>{break;}
         Some(command)=commands.recv()=>{match command{
           Command::AutoAnswer(v)=>auto_answer=v,
           Command::Hangup|Command::Decline=>{let id=call.as_ref().map(|s|s.call_id.clone()).or_else(||pending.as_ref().and_then(|(v,_)|v["callId"].as_str().map(str::to_string)));if let Some(id)=id{let _=signals.send(json!({"type":"hangup","callId":id}));}end_call!();},
           Command::Accept=>{if let Some((v,_))=pending.take(){let _=signals.send(json!({"internalAccept":v}));}},
           Command::Mute(v)=>{if let Some(s)=&call{s.muted.store(v,std::sync::atomic::Ordering::SeqCst);}},
           Command::RefreshCalls=>{let path=format!("/api/device/calls?deviceId={}&limit=5",c.device_id);match request::<Vec<CallRecord>>(&http,&c,&path,None).await{Ok(v)=>state.lock().unwrap().records=v,Err(e)=>state.lock().unwrap().last_error=e}},
           Command::Verify(id,code)=>{let result=request::<Value>(&http,&c,"/api/device/totp",Some(json!({"deviceId":c.device_id,"code":code}))).await;let(ok,error)=match result{Ok(v)=>(v["verified"]==true,"验证码或密码不正确".into()),Err(e)=>(false,e)};state.lock().unwrap().auth_result=Some((id,ok,error));},
           Command::Play(id)=>{if call.is_some(){state.lock().unwrap().last_error="请先结束通话再播放录音".into();continue;}let path=format!("{}/api/device/calls/{}/audio",c.server_url,id);match http.get(path).header("X-Device-Token",&c.device_token).send().await{Ok(r)if r.status().is_success()=>{if let Ok(bytes)=r.bytes().await{match crate::playback::AudioOutput::new(){Ok(p)=>{if let Some(old)=player.take(){old.stop();}let out=p.clone();let generation=out.ticket();let s=state.clone();tokio::task::spawn_blocking(move||{if let Err(e)=out.play_file(bytes.to_vec(),generation){s.lock().unwrap().last_error=e;}});player=Some(p);},Err(e)=>state.lock().unwrap().last_error=e}}},_=>state.lock().unwrap().last_error="录音加载失败".into()}},
           Command::StopPlayback=>{if let Some(p)=player.take(){p.stop();}},
           Command::Quit=>{end_call!();audio.capture(false);let _=request::<Value>(&http,&c,"/api/device/local-stop",Some(json!({"deviceId":c.device_id}))).await;state.lock().unwrap().exit_ready=true;break;},
         }}
         _=heartbeat.tick()=>{
           let snap=audio.shared.snapshot();let body=json!({"deviceId":c.device_id,"monitoring":snap.capturing,"appVersion":env!("CARGO_PKG_VERSION"),"captureError":snap.error});
           let result=tokio::select!{_=stop.changed()=>break,result=request::<Policy>(&http,&c,"/api/device/heartbeat",Some(body))=>result};
           match result{Ok(p)=>{policy=p;authorized_once=true;server_timestamp=policy.server_time.unwrap_or_else(Utc::now);server_anchor=Instant::now();audio.shared.set_denoise(match policy.denoise_mode.as_str(){"light"=>crate::audio::DENOISE_LIGHT,"standard"=>crate::audio::DENOISE_STANDARD,"strong"=>crate::audio::DENOISE_STRONG,_=>crate::audio::DENOISE_OFF},policy.voice_enhance);let mut s=state.lock().unwrap();s.connected=true;s.last_error.clear();s.last_heartbeat=Some(Utc::now());s.policy=policy.clone();},Err(e)=>{if e.starts_with("HTTP 401")||e.starts_with("HTTP 403"){authorized_once=false;audio.capture(false);end_call!();socket=None;}let mut s=state.lock().unwrap();s.connected=false;s.last_error=e;}}
         }
         _=meter.tick()=>{
           policy.server_time=Some(server_timestamp+chrono::Duration::from_std(server_anchor.elapsed()).unwrap_or_default());
           audio.capture(if call.is_some(){true}else{policy.capturing()&&authorized_once});
           state.lock().unwrap().policy=policy.clone();
           if pending.as_ref().is_some_and(|(_,time)|time.elapsed()>Duration::from_secs(25)){if let Some((v,_))=pending.take(){let _=signals.send(json!({"type":"hangup","callId":v["callId"]}));}state.lock().unwrap().incoming=None;}
           if call.as_ref().is_some_and(|s|matches!(s.peer.connection_state(),webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Failed|webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Closed)){end_call!();}
           if call.is_some() && {let s=state.lock().unwrap();s.call_state=="connecting" && s.call_started.is_some_and(|t|t.elapsed()>Duration::from_secs(25))}{let id=call.as_ref().unwrap().call_id.clone();let _=signals.send(json!({"type":"hangup","callId":id}));end_call!();state.lock().unwrap().last_error="通话连接超时，已恢复监测".into();}
           if let Some(s)=&call{if s.peer.connection_state()==webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected{state.lock().unwrap().call_state="active".into();}}
           if let Some(w)=audio.shared.take_window(){let offset=audio.shared.offset() as f64;let avg=(w.avg_dbfs+offset).clamp(0.0,160.0);use base64::Engine;let audio_base64=if !policy.war()&&avg>=policy.threshold_db&&policy.auto_record_evidence{audio.shared.take_evidence_wav(30).map(|v|base64::engine::general_purpose::STANDARD.encode(v))}else{None};if reports.len()>=12{reports.pop_front();}reports.push_back(Report{device_id:c.device_id.clone(),avg_db:avg,peak_db:(w.peak_dbfs+offset).clamp(0.0,160.0),min_db:(w.min_dbfs+offset).clamp(0.0,160.0),duration_secs:w.duration_secs.round().clamp(1.0,300.0) as u32,sample_count:w.frames.min(u32::MAX as u64) as u32,recorded_at:policy.server_time.unwrap_or_else(Utc::now).to_rfc3339(),audio_base64});}
           if state.lock().unwrap().connected{
             if let Some(report)=reports.front(){match request::<Value>(&http,&c,"/api/device/telemetry",Some(serde_json::to_value(report).unwrap())).await{Ok(v)if v["accepted"]==true=>{reports.pop_front();state.lock().unwrap().reports_sent+=1;},Ok(_)=>state.lock().unwrap().last_error="服务器未确认遥测入库".into(),Err(e)=>state.lock().unwrap().last_error=e}}
             let s=audio.shared.snapshot();let _=request::<Value>(&http,&c,"/api/device/live",Some(json!({"deviceId":c.device_id,"currentDb":s.current_db.unwrap_or(0.0),"averageDb":s.average_db,"peakDb":s.peak_db,"rolling15sDb":s.rolling_15s_db,"capturing":s.capturing,"suspended":s.suspended,"systemPlaying":s.system_playing,"aecEnabled":s.aec_enabled,"recordedAt":Utc::now()}))).await;
           }
           state.lock().unwrap().pending_reports=reports.len();
         }
         _=records_tick.tick()=>{if state.lock().unwrap().connected{if let Ok(v)=request::<Vec<CallRecord>>(&http,&c,&format!("/api/device/calls?deviceId={}&limit=5",c.device_id),None).await{state.lock().unwrap().records=v;}}}
         _=reconnect.tick(),if socket.is_none()=>{if !state.lock().unwrap().connected{continue;}if let Ok(url)=ws_url(&c){let result=tokio::select!{_=stop.changed()=>break,r=tokio::time::timeout(Duration::from_secs(4),connect_async(url))=>r};if let Ok(Ok((ws,_)))=result{socket=Some(ws);state.lock().unwrap().signaling=true;}}}
         Some(value)=outbound.recv()=>{
           if let Some(offer)=value.get("internalAccept"){
             let id=offer["callId"].as_str().unwrap_or("");let mode=offer["mode"].as_str().unwrap_or("intercom");let sdp=offer["payload"]["sdp"].as_str().unwrap_or("");
             audio.shared.set_suspended(true);audio.capture(true);
             match CallSession::answer(sdp,id,mode,policy.ice_servers.clone(),&audio,signals.clone()).await{Ok((session,answer))=>{if let Some(candidates)=early_ice.remove(id){for candidate in candidates{if let Ok(c)=serde_json::from_value(candidate){let _=session.peer.add_ice_candidate(c).await;}}}call=Some(session);let mut s=state.lock().unwrap();s.incoming=None;s.call_state="connecting".into();s.call_mode=mode.into();s.call_started=Some(Instant::now());drop(s);let _=signals.send(json!({"type":"signal","callId":id,"payload":{"kind":"answer","sdp":answer}}));},Err(e)=>{end_call!();state.lock().unwrap().last_error=e;let _=signals.send(json!({"type":"signal","callId":id,"payload":{"kind":"reject","reason":"audio-unavailable"}}));}}
           }else if let Some(ws)=socket.as_mut(){if ws.send(Message::Text(value.to_string())).await.is_err(){socket=None;state.lock().unwrap().signaling=false;end_call!();}}
         }
         incoming=async{match socket.as_mut(){Some(ws)=>ws.next().await,None=>std::future::pending().await}}=>{
           match incoming{
             Some(Ok(Message::Text(text)))=>{if let Ok(v)=serde_json::from_str::<Value>(&text){let id=v["callId"].as_str().unwrap_or("");match v["type"].as_str().unwrap_or(""){
               "hangup"=>{if call.as_ref().is_some_and(|s|s.call_id==id)||pending.as_ref().is_some_and(|(s,_)|s["callId"]==id){end_call!();}},
               "command"=>{if v["action"]=="set-monitoring"{policy.desired_monitoring=v["enabled"].as_bool().unwrap_or(false);policy.manual_monitoring=v["manualMonitoring"].as_bool();}},
               "signal"=>match v["payload"]["kind"].as_str().unwrap_or(""){
                 "offer"=>{if call.is_some()||pending.is_some(){let _=signals.send(json!({"type":"signal","callId":id,"payload":{"kind":"reject","reason":"busy"}}));}else if auto_answer{let _=signals.send(json!({"internalAccept":v}));}else{state.lock().unwrap().incoming=Some(v.clone());pending=Some((v,Instant::now()));}},
                 "ice"=>{if let Some(s)=call.as_ref().filter(|s|s.call_id==id){if let Ok(candidate)=serde_json::from_value(v["payload"]["candidate"].clone()){let _=s.peer.add_ice_candidate(candidate).await;}}else if early_ice.len()<8{let q=early_ice.entry(id.to_string()).or_default();if q.len()<64{q.push(v["payload"]["candidate"].clone());}}},_=>{}
               },_=>{}
             }}},
             Some(Ok(Message::Ping(data)))=>{if let Some(ws)=socket.as_mut(){let _=ws.send(Message::Pong(data)).await;}},
             Some(Ok(Message::Close(_)))|Some(Err(_))|None=>{socket=None;state.lock().unwrap().signaling=false;end_call!();},_=>{}
           }
         }
        }
    }
    end_call!();
    if let Some(mut ws) = socket {
        let _ = ws.close(None).await;
    }
    if let Some(p) = player {
        p.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_manual_switch_survives_out_of_window_effective_response() {
        let mut policy = Policy {
            desired_monitoring: false,
            manual_monitoring: Some(true),
            capture_start: "07:00".into(),
            capture_end: "08:00".into(),
            server_utc_offset_secs: 8 * 3600,
            server_time: Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-25T23:30:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            ..Policy::default()
        };
        assert!(policy.capturing());
        policy.manual_monitoring = Some(false);
        assert!(!policy.capturing());
    }
    #[test]
    fn schedule_boundaries() {
        assert!(within_window("22:00", "06:00", 23 * 60));
        assert!(!within_window("22:00", "06:00", 12 * 60));
        assert!(within_window("", "", 0));
        assert!(!within_window("09:00", "", 480));
        assert!(within_window("09:00", "", 600));
    }
    #[test]
    fn websocket_url_encodes_tokens() {
        let c = ClientConfig {
            server_url: "https://example.org/prefix".into(),
            device_id: "room+1".into(),
            device_token: "a&b".into(),
            ..Default::default()
        };
        let url = ws_url(&c).unwrap();
        assert!(url.starts_with("wss://example.org/prefix/api/device/ws?"));
        assert!(url.contains("a%26b"));
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn authenticates_refreshes_live_and_rebinds_without_stale_state() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = hits.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut all = Vec::new();
                    let mut block = [0; 4096];
                    loop {
                        let n = stream.read(&mut block).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        all.extend_from_slice(&block[..n]);
                        if let Some(end) = all.windows(4).position(|p| p == b"\r\n\r\n") {
                            let header = String::from_utf8_lossy(&all[..end]);
                            let len = header
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            if all.len() >= end + 4 + len {
                                break;
                            }
                        }
                    }
                    let text = String::from_utf8_lossy(&all).into_owned();
                    seen.lock().unwrap().push(text.clone());
                    let body = if text.starts_with("POST /api/device/heartbeat") {
                        let name = if text.contains("token-two") {
                            "二号教室"
                        } else {
                            "一号教室"
                        };
                        json!({"deviceName":name,"thresholdDb":72.5,"desiredMonitoring":false,"denoiseMode":"off","serverTime":Utc::now(),"iceServers":[]}).to_string()
                    } else if text.starts_with("GET /api/device/calls") {
                        "[]".into()
                    } else if text.starts_with("POST /api/device/totp") {
                        "{\"verified\":true}".into()
                    } else {
                        "{\"accepted\":true}".into()
                    };
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        let mut net = NetManager::new();
        let audio = AudioEngineHandle::start(90.0);
        let mut c = ClientConfig {
            server_url: format!("http://{addr}"),
            device_id: "room-one".into(),
            device_token: "token-one".into(),
            local_password_hash: "test".into(),
            ..Default::default()
        };
        net.start(c.clone(), audio.clone());
        for _ in 0..100 {
            if net.poll().connected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(net.poll().connected);
        assert_eq!(net.poll().policy.device_name, "一号教室");
        assert_eq!(net.poll().policy.threshold_db, 72.5);
        net.send(Command::Verify(42, "fixed-password".into()));
        for _ in 0..100 {
            if net.poll().auth_result.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(net.poll().auth_result.unwrap().1);
        c.device_id = "room-two".into();
        c.device_token = "token-two".into();
        net.start(c, audio.clone());
        for _ in 0..100 {
            if net.poll().connected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(net.poll().policy.device_name, "二号教室");
        net.send(Command::Quit);
        for _ in 0..100 {
            if net.poll().exit_ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(net.poll().exit_ready);
        net.stop();
        audio.stop();
        assert!(hits
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("local-stop")));
        server.abort();
    }
}
