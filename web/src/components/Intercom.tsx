import { useEffect, useRef, useState } from "react";
import {
  AudioLines,
  LoaderCircle,
  Mic,
  MicOff,
  Phone,
  PhoneOff,
  Radio,
  ShieldCheck,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import type { Device } from "../types";
import type { IntercomController } from "../hooks/useIntercom";
import { formatDuration } from "../lib/format";

function useMicLevel(stream: MediaStream | null): number {
  const [level, setLevel] = useState(0);

  useEffect(() => {
    if (!stream) {
      setLevel(0);
      return;
    }
    const AudioContextClass = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextClass) return;
    const context = new AudioContextClass();
    const analyser = context.createAnalyser();
    analyser.fftSize = 512;
    analyser.smoothingTimeConstant = 0.75;
    const source = context.createMediaStreamSource(stream);
    source.connect(analyser);
    const buffer = new Uint8Array(analyser.fftSize);
    let frame = 0;
    let lastUpdate = 0;
    const loop = (time: number) => {
      analyser.getByteTimeDomainData(buffer);
      if (time - lastUpdate > 80) {
        let sum = 0;
        for (let index = 0; index < buffer.length; index += 1) {
          const value = (buffer[index] - 128) / 128;
          sum += value * value;
        }
        const rms = Math.sqrt(sum / buffer.length);
        setLevel(Math.min(1, rms * 6));
        lastUpdate = time;
      }
      frame = requestAnimationFrame(loop);
    };
    void context.resume();
    frame = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(frame);
      source.disconnect();
      void context.close();
    };
  }, [stream]);

  return level;
}

interface IntercomCardProps {
  controller: IntercomController;
  device: Device;
  signalingConnected: boolean;
}

export function IntercomCard({ controller, device, signalingConnected }: IntercomCardProps) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [autoplayBlocked, setAutoplayBlocked] = useState(false);
  const micLevel = useMicLevel(controller.localStream);
  const active = controller.phase === "active";
  const busy = controller.phase === "requesting" || controller.phase === "connecting";
  const canCall = device.online && signalingConnected;

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.srcObject = controller.remoteStream;
    if (controller.remoteStream) {
      audio.play().then(() => setAutoplayBlocked(false)).catch(() => setAutoplayBlocked(true));
    }
  }, [controller.remoteStream]);

  useEffect(() => {
    if (!controller.remoteStream) setAutoplayBlocked(false);
  }, [controller.remoteStream]);

  return (
    <div className={`panel intercom-card ${active ? "is-active" : ""}`}>
      <div className="panel-header">
        <div>
          <div className="eyebrow">远程语音</div>
          <h3>远程语音</h3>
        </div>
        <div className={`signal-pill ${active ? "is-live" : ""}`}>
          <AudioLines size={14} />
          {active ? (controller.mode === "broadcast" ? "广播中" : "对讲中") : canCall ? "通道就绪" : "待连接"}
        </div>
      </div>

      <audio ref={audioRef} autoPlay playsInline />

      {controller.phase === "idle" && (
        <>
          <div className="intercom-illustration">
            <div className="intercom-ring intercom-ring-1" />
            <div className="intercom-ring intercom-ring-2" />
            <div className="intercom-core">
              <Radio size={26} />
            </div>
          </div>
          <p className="intercom-desc">
            通过管理端直接与现场双向通话，或向设备单向广播。音频使用端到端 WebRTC 传输。
          </p>
          <div className="intercom-actions">
            <button
              type="button"
              className="primary-button"
              disabled={!canCall}
              onClick={() => void controller.start("intercom")}
            >
              <Phone size={16} />
              双向对讲
            </button>
            <button
              type="button"
              className="secondary-button"
              disabled={!canCall}
              onClick={() => void controller.start("broadcast")}
            >
              <Volume2 size={16} />
              远程广播
            </button>
          </div>
          <div className="intercom-footnote">
            <ShieldCheck size={13} />
            {device.online
              ? signalingConnected
                ? "点击后现场设备将自动接通（可在客户端关闭自动接听）"
                : "设备在线，但对讲信令通道尚未连接"
              : "设备离线，无法发起语音通话"}
          </div>
        </>
      )}

      {(busy || active) && (
        <>
          <div className="call-stage">
            <div className={`call-orb ${active ? "is-active" : ""}`}>
              {busy ? <LoaderCircle size={26} className="spin" /> : <Mic size={26} />}
            </div>
            <div className="call-state">
              <strong>
                {busy
                  ? controller.phase === "requesting"
                    ? "正在获取麦克风…"
                    : "正在建立音频通道…"
                  : controller.mode === "broadcast"
                    ? "正在向现场广播"
                    : "双向对讲进行中"}
              </strong>
              <span>{active ? formatDuration(controller.elapsed) : "请稍候"}</span>
            </div>
          </div>

          {active && (
            <div className="mic-meter" aria-label="麦克风电平">
              {Array.from({ length: 18 }).map((_, index) => {
                const threshold = (index + 1) / 20;
                const lit = micLevel >= threshold;
                return (
                  <span
                    key={index}
                    className={`mic-meter-bar ${lit ? "is-lit" : ""}`}
                    style={{ height: `${24 + index * 2.6}%` }}
                  />
                );
              })}
              <div className="mic-meter-label">
                {controller.muted ? <MicOff size={13} /> : <Mic size={13} />}
                {controller.muted ? "麦克风已静音" : "麦克风工作正常"}
              </div>
            </div>
          )}

          {autoplayBlocked && (
            <button
              type="button"
              className="audio-unlock"
              onClick={() => {
                void audioRef.current?.play().then(() => setAutoplayBlocked(false)).catch(() => undefined);
              }}
            >
              <VolumeX size={15} />
              浏览器阻止了自动播放，点击启用现场声音
            </button>
          )}

          <div className="intercom-actions call-actions">
            <button type="button" className="secondary-button" onClick={controller.toggleMute} disabled={!active}>
              {controller.muted ? <MicOff size={16} /> : <Mic size={16} />}
              {controller.muted ? "取消静音" : "静音"}
            </button>
            <button type="button" className="danger-button" onClick={controller.hangup}>
              <PhoneOff size={16} />
              结束通话
            </button>
          </div>
        </>
      )}

      {controller.phase === "error" && (
        <div className="call-error">
          <div className="call-error-icon">
            <VolumeX size={20} />
          </div>
          <div>
            <strong>对讲未建立</strong>
            <p>{controller.error}</p>
          </div>
          <button type="button" className="icon-button ghost" onClick={controller.dismissError} aria-label="关闭">
            <X size={16} />
          </button>
        </div>
      )}

      {controller.phase === "error" && (
        <div className="intercom-actions">
          <button
            type="button"
            className="primary-button"
            disabled={!canCall}
            onClick={() => void controller.start("intercom")}
          >
            <Phone size={16} />
            重试对讲
          </button>
          <button type="button" className="secondary-button" onClick={controller.dismissError}>
            关闭
          </button>
        </div>
      )}
    </div>
  );
}

interface CallDockProps {
  controller: IntercomController;
  deviceName: string;
}

export function CallDock({ controller, deviceName }: CallDockProps) {
  if (controller.phase === "idle" || controller.phase === "error") return null;
  return (
    <div className="call-dock">
      <div className={`call-dock-wave ${controller.phase === "active" ? "is-active" : ""}`}>
        <span />
        <span />
        <span />
        <span />
      </div>
      <div className="call-dock-copy">
        <strong>
          {controller.phase === "active"
            ? controller.mode === "broadcast"
              ? "正在广播"
              : "正在对讲"
            : "正在接通"}
        </strong>
        <span>
          {deviceName} · {formatDuration(controller.elapsed)}
        </span>
      </div>
      <button
        type="button"
        className="icon-button"
        onClick={controller.toggleMute}
        disabled={controller.phase !== "active"}
        aria-label={controller.muted ? "取消静音" : "静音"}
      >
        {controller.muted ? <MicOff size={17} /> : <Mic size={17} />}
      </button>
      <button type="button" className="call-dock-end" onClick={controller.hangup} aria-label="结束通话">
        <PhoneOff size={17} />
      </button>
    </div>
  );
}
