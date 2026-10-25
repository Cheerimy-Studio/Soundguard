import { useCallback, useEffect, useRef, useState } from "react";
import { api, getToken } from "../api";
import type { IntercomMode, LiveMessage, SignalPayload } from "../types";

export type IntercomPhase = "idle" | "requesting" | "connecting" | "active" | "error";
export type SignalHandler = (message: LiveMessage) => void;
export type RegisterSignalHandler = (handler: SignalHandler) => () => void;
export type SendSignal = (message: unknown) => boolean;

export interface IntercomController {
  phase: IntercomPhase;
  mode: IntercomMode | null;
  error: string | null;
  muted: boolean;
  elapsed: number;
  localStream: MediaStream | null;
  remoteStream: MediaStream | null;
  start: (mode: IntercomMode) => Promise<void>;
  hangup: () => void;
  toggleMute: () => void;
  dismissError: () => void;
}

interface Options {
  iceServers: RTCIceServer[];
  sendSignal: SendSignal;
  registerSignalHandler: RegisterSignalHandler;
  deviceId: string | null;
}

interface RecordingSession {
  sessionToken: string | null;
  segmentTimer: number | null;
  callId: string;
  deviceId: string;
  mode: IntercomMode;
  startedAt: number;
  recorder: MediaRecorder | null;
  chunks: Blob[];
  context: AudioContext | null;
  destination: MediaStreamAudioDestinationNode | null;
  remoteSource: MediaStreamAudioSourceNode | null;
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      resolve(result.includes(",") ? result.split(",")[1] : result);
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

function signalId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `call-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

interface CallAudioGuard {
  attachRemote: (stream: MediaStream) => void;
  setUserMuted: (muted: boolean) => void;
  resume: () => void;
  close: () => void;
}

/**
 * Half-duplex anti-feedback guard. While the remote side is speaking, the
 * outgoing microphone track is disabled. This breaks the acoustic loop that
 * occurs when both endpoints are in the same room. The microphone reopens
 * shortly after the remote is quiet. Using the track itself avoids a second
 * audio graph, so auto-answer and background calls keep working.
 */
// Native collectors answer with PCMU/8000; force the browser sender to match.
function preferPcmu(pc: RTCPeerConnection) {
  try {
    const transceiver = pc
      .getTransceivers()
      .find((item) => item.sender?.track?.kind === "audio");
    const codecs = RTCRtpSender.getCapabilities?.("audio")?.codecs ?? [];
    const pcmu = codecs.filter((codec) => /PCMU\/8000/i.test(codec.mimeType));
    if (transceiver && pcmu.length) transceiver.setCodecPreferences(pcmu);
  } catch (error) {
    console.warn("codec preference failed", error);
  }
}

function createCallAudioGuard(localStream: MediaStream): CallAudioGuard | null {
  const AudioContextClass =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext })
      .webkitAudioContext;
  if (!AudioContextClass) return null;
  try {
    const context = new AudioContextClass();
    const tracks = localStream.getAudioTracks();
    let active = true;
    let sensor: MediaStreamAudioSourceNode | null = null;
    let animation = 0;
    let ducked = false;
    let userMuted = false;
    let lastLoud = 0;

    const apply = () => {
      tracks.forEach((track) => {
        track.enabled = !(userMuted || ducked);
      });
    };

    const attachRemote = (remoteStream: MediaStream) => {
      if (sensor || !active) return;
      void context.resume().catch(() => undefined);
      try {
        const analyser = context.createAnalyser();
        analyser.fftSize = 512;
        analyser.smoothingTimeConstant = 0.55;
        sensor = context.createMediaStreamSource(remoteStream);
        const silentSink = context.createGain();
        silentSink.gain.value = 0;
        sensor.connect(analyser);
        analyser.connect(silentSink);
        silentSink.connect(context.destination);
        const samples = new Float32Array(analyser.fftSize);
        const tick = () => {
          if (!active) return;
          analyser.getFloatTimeDomainData(samples);
          let sum = 0;
          for (let index = 0; index < samples.length; index += 1) {
            sum += samples[index] * samples[index];
          }
          const rms = Math.sqrt(sum / samples.length);
          const now = performance.now();
          if (rms > 0.015) lastLoud = now;
          const shouldDuck = now - lastLoud < 280;
          if (shouldDuck !== ducked) {
            ducked = shouldDuck;
            apply();
          }
          animation = requestAnimationFrame(tick);
        };
        animation = requestAnimationFrame(tick);
      } catch {
        // The remote track may not be attachable yet; keep the plain stream.
      }
    };

    const close = () => {
      active = false;
      if (animation) cancelAnimationFrame(animation);
      try {
        sensor?.disconnect();
      } catch {
        // Ignore.
      }
      ducked = false;
      userMuted = false;
      apply();
      try {
        void context.close();
      } catch {
        // Ignore.
      }
    };

    const setUserMuted = (muted: boolean) => {
      userMuted = muted;
      apply();
    };

    const resume = () => {
      void context.resume().catch(() => undefined);
    };

    void context.resume().catch(() => undefined);
    return { attachRemote, setUserMuted, resume, close };
  } catch {
    return null;
  }
}

export function useIntercom({
  iceServers,
  sendSignal,
  registerSignalHandler,
  deviceId,
}: Options): IntercomController {
  const [phase, setPhase] = useState<IntercomPhase>("idle");
  const [mode, setMode] = useState<IntercomMode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [muted, setMuted] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [localStream, setLocalStream] = useState<MediaStream | null>(null);
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null);

  const phaseRef = useRef<IntercomPhase>("idle");
  const pcRef = useRef<RTCPeerConnection | null>(null);
  const localStreamRef = useRef<MediaStream | null>(null);
  const remoteStreamRef = useRef<MediaStream | null>(null);
  const audioGuardRef = useRef<CallAudioGuard | null>(null);
  const callIdRef = useRef<string | null>(null);
  const deviceIdRef = useRef<string | null>(deviceId);
  const recordingRef = useRef<RecordingSession | null>(null);
  const pendingIceRef = useRef<RTCIceCandidateInit[]>([]);
  const remoteDescriptionSetRef = useRef(false);
  const timeoutRef = useRef<number | null>(null);
  const timerRef = useRef<number | null>(null);
  const startedAtRef = useRef<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    deviceIdRef.current = deviceId;
  }, [deviceId]);

  const updatePhase = useCallback((next: IntercomPhase) => {
    phaseRef.current = next;
    if (mountedRef.current) setPhase(next);
  }, []);

  const clearTimers = useCallback(() => {
    if (timeoutRef.current !== null) {
      window.clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
    if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const finalizeRecording = useCallback(async (session: RecordingSession) => {
    if (session.segmentTimer !== null) window.clearTimeout(session.segmentTimer);
    const endedAt = new Date();
    let blob: Blob | null = null;
    if (session.recorder && session.recorder.state !== "inactive") {
      blob = await new Promise<Blob | null>((resolve) => {
        let settled = false;
        const finish = () => {
          if (settled) return;
          settled = true;
          resolve(
            session.chunks.length > 0
              ? new Blob(session.chunks, {
                  type: session.recorder?.mimeType || "audio/webm",
                })
              : null,
          );
        };
        session.recorder!.onstop = finish;
        try {
          session.recorder!.stop();
        } catch {
          finish();
        }
        window.setTimeout(finish, 1200);
      });
    }
    try {
      session.remoteSource?.disconnect();
      if (session.context && session.context.state !== "closed") {
        await session.context.close();
      }
    } catch {
      // Recording cleanup is best effort.
    }

    try {
      let audioBase64: string | undefined;
      let audioMime: string | undefined;
      if (blob && blob.size > 0) {
        audioBase64 = await blobToBase64(blob);
        audioMime = blob.type || "audio/webm";
      }
      await api.saveCallRecord({
        deviceId: session.deviceId,
        mode: session.mode,
        startedAt: new Date(session.startedAt).toISOString(),
        endedAt: endedAt.toISOString(),
        status: "completed",
        audioBase64,
        audioMime,
      }, session.sessionToken);
    } catch (error) {
      console.warn("Call recording upload failed", error);
    }
  }, []);

  const startRecording = useCallback(
    function beginRecording(callId: string, callMode: IntercomMode, stream: MediaStream) {
      if (recordingRef.current) return;
      const device = deviceIdRef.current;
      if (!device) return;
      const session: RecordingSession = {
        sessionToken: getToken(),
        segmentTimer: null,
        callId,
        deviceId: device,
        mode: callMode,
        startedAt: Date.now(),
        recorder: null,
        chunks: [],
        context: null,
        destination: null,
        remoteSource: null,
      };
      recordingRef.current = session;
      // Bound memory and upload sizes during long-running calls.
      session.segmentTimer = window.setTimeout(() => {
        if (recordingRef.current !== session || callIdRef.current !== callId) return;
        recordingRef.current = null;
        void finalizeRecording(session);
        beginRecording(callId, callMode, stream);
      }, 240_000);
      try {
        const AudioContextClass =
          window.AudioContext ||
          (window as unknown as { webkitAudioContext?: typeof AudioContext })
            .webkitAudioContext;
        if (AudioContextClass && typeof MediaRecorder !== "undefined") {
          const context = new AudioContextClass({ sampleRate: 48_000 });
          const destination = context.createMediaStreamDestination();
          const source = context.createMediaStreamSource(stream);
          source.connect(destination);
          const preferredTypes = [
            "audio/webm;codecs=opus",
            "audio/webm",
            "audio/mp4",
          ];
          const mimeType = preferredTypes.find((type) =>
            MediaRecorder.isTypeSupported(type),
          );
          const recorder = new MediaRecorder(destination.stream, {
            ...(mimeType ? { mimeType } : {}),
            audioBitsPerSecond: 192_000,
          });
          session.context = context;
          session.destination = destination;
          let recordedBytes = 0;
          recorder.ondataavailable = (event) => {
            if (event.data.size > 0) {
              session.chunks.push(event.data);
              recordedBytes += event.data.size;
              if (recordedBytes >= 8 * 1024 * 1024 && recordingRef.current === session) {
                recordingRef.current = null;
                void finalizeRecording(session);
                if (callIdRef.current === callId) beginRecording(callId, callMode, stream);
              }
            }
          };
          recorder.start(1000);
          session.context = context;
          session.destination = destination;
          session.recorder = recorder;
          void context.resume();
          if (remoteStreamRef.current) {
            try {
              const remoteSource = context.createMediaStreamSource(
                remoteStreamRef.current,
              );
              remoteSource.connect(destination);
              session.remoteSource = remoteSource;
            } catch {
              // The remote stream may not contain an audio track yet.
            }
          }
        }
      } catch {
        // Metadata-only call records still work when MediaRecorder is unavailable.
      }
    },
    [finalizeRecording],
  );

  const attachRemoteToRecording = useCallback((stream: MediaStream) => {
    const session = recordingRef.current;
    if (!session?.context || !session.destination || session.remoteSource) return;
    try {
      const source = session.context.createMediaStreamSource(stream);
      source.connect(session.destination);
      session.remoteSource = source;
    } catch {
      // A track might already be attached by a previous event.
    }
  }, []);

  const resetCall = useCallback(
    (nextPhase: IntercomPhase = "idle", nextError: string | null = null) => {
      clearTimers();
      const closingCall = callIdRef.current;
      if (closingCall) sendSignal({ type: "hangup", callId: closingCall, deviceId: deviceIdRef.current, to: "device" });
      const session = recordingRef.current;
      recordingRef.current = null;
      if (session) {
        void finalizeRecording(session);
      }
      if (pcRef.current) {
        pcRef.current.onicecandidate = null;
        pcRef.current.ontrack = null;
        pcRef.current.onconnectionstatechange = null;
        pcRef.current.close();
        pcRef.current = null;
      }
      audioGuardRef.current?.close();
      audioGuardRef.current = null;
      localStreamRef.current?.getTracks().forEach((track) => track.stop());
      localStreamRef.current = null;
      remoteStreamRef.current = null;
      callIdRef.current = null;
      pendingIceRef.current = [];
      remoteDescriptionSetRef.current = false;
      startedAtRef.current = null;
      if (mountedRef.current) {
        setLocalStream(null);
        setRemoteStream(null);
        setMuted(false);
        setElapsed(0);
        setError(nextError);
        setMode(null);
      }
      updatePhase(nextPhase);
    },
    [clearTimers, finalizeRecording, updatePhase, sendSignal],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      resetCall("idle");
    };
  }, [resetCall]);

  const fail = useCallback(
    (message: string) => {
      resetCall("error", message);
    },
    [resetCall],
  );

  const flushIce = useCallback(async () => {
    const pc = pcRef.current;
    if (!pc) return;
    const candidates = pendingIceRef.current.splice(0);
    for (const candidate of candidates) {
      try {
        await pc.addIceCandidate(candidate);
      } catch {
        // A late candidate can fail after the peer connection closes; ignore it.
      }
    }
  }, []);

  useEffect(() => {
    const unsubscribe = registerSignalHandler((message) => {
      const callId = callIdRef.current;
      if (!callId) return;
      if (
        message.type !== "signal" &&
        message.type !== "hangup" &&
        message.type !== "signal-error"
      ) {
        return;
      }
      if (message.deviceId && message.deviceId !== deviceIdRef.current) return;
      if (message.type === "signal-error") {
        if (!message.callId || message.callId === callId) {
          fail(message.message || "对讲通道建立失败");
        }
        return;
      }
      if (message.callId !== callId) return;
      if (message.from && message.from !== "device" && message.from !== "admin") return;
      if (message.type === "hangup") {
        resetCall("idle", "对方已结束通话");
        return;
      }
      const pc = pcRef.current;
      const payload = message.payload as SignalPayload | undefined;
      if (!pc || !payload) return;
      if (payload.kind === "reject") {
        fail(payload.reason === "calls-disabled" ? "现场设备已关闭远程接听" : "现场设备拒绝了本次通话");
        return;
      }
      if (payload.kind === "answer" && payload.sdp) {
        void (async () => {
          try {
            await pc.setRemoteDescription({ type: "answer", sdp: payload.sdp });
            remoteDescriptionSetRef.current = true;
            await flushIce();
          } catch {
            fail("无法建立音频连接，可能是网络或设备音频权限问题");
          }
        })();
        return;
      }
      if (payload.kind === "ice" && payload.candidate) {
        void (async () => {
          if (remoteDescriptionSetRef.current) {
            try {
              await pc.addIceCandidate(payload.candidate);
            } catch {
              // Ignore stale candidates.
            }
          } else {
            pendingIceRef.current.push(payload.candidate!);
          }
        })();
      }
    });
    return unsubscribe;
  }, [fail, flushIce, registerSignalHandler, resetCall]);

  const start = useCallback(
    async (nextMode: IntercomMode) => {
      if (phaseRef.current === "requesting" || phaseRef.current === "connecting" || phaseRef.current === "active") {
        return;
      }
      if (!window.isSecureContext) {
        setError("浏览器安全策略禁止在 HTTP 页面使用麦克风，请通过 HTTPS 或 localhost 访问。");
        updatePhase("error");
        return;
      }
      if (!navigator.mediaDevices?.getUserMedia) {
        setError("当前浏览器不支持麦克风采集");
        updatePhase("error");
        return;
      }

      setError(null);
      setMuted(false);
      setElapsed(0);
      setMode(nextMode);
      const callId = signalId();
      callIdRef.current = callId;
      pendingIceRef.current = [];
      remoteDescriptionSetRef.current = false;
      updatePhase("requesting");

      let stream: MediaStream | null = null;
      try {
        stream = await navigator.mediaDevices.getUserMedia({
          audio: {
            echoCancellation: true,
            noiseSuppression: true,
            autoGainControl: true,
            channelCount: 1,
          },
          video: false,
        });
        if (callIdRef.current !== callId) {
          stream.getTracks().forEach((track) => track.stop());
          return;
        }
        localStreamRef.current = stream;
        setLocalStream(stream);
        const activeStream = stream;
        audioGuardRef.current = createCallAudioGuard(activeStream);

        if (!deviceIdRef.current) throw new Error("请先选择设备");
        const pc = new RTCPeerConnection({
          iceServers,
          iceCandidatePoolSize: 4,
        });
        pcRef.current = pc;

        activeStream.getTracks().forEach((track) => pc.addTrack(track, activeStream));
        preferPcmu(pc);
        pc.onicecandidate = (event) => {
          if (!event.candidate || callIdRef.current !== callId) return;
          sendSignal({
            type: "signal",
            callId,
            deviceId: deviceIdRef.current,
            mode: nextMode,
            to: "device",
            payload: { kind: "ice", candidate: event.candidate.toJSON() },
          });
        };
        pc.ontrack = (event) => {
          const streamFromPeer = event.streams[0] ?? new MediaStream([event.track]);
          remoteStreamRef.current = streamFromPeer;
          attachRemoteToRecording(streamFromPeer);
          if (mountedRef.current) setRemoteStream(streamFromPeer);
        };
        pc.onconnectionstatechange = () => {
          if (pcRef.current !== pc || callIdRef.current !== callId) return;
          if (pc.connectionState === "connected") {
            clearTimers();
            if (phaseRef.current !== "active") {
              updatePhase("active");
              if (!startedAtRef.current) startedAtRef.current = Date.now();
              startRecording(callId, nextMode, activeStream);
              timerRef.current = window.setInterval(() => {
                if (startedAtRef.current) {
                  setElapsed(Math.floor((Date.now() - startedAtRef.current) / 1000));
                }
              }, 1000);
            }
          } else if (pc.connectionState === "failed") {
            fail("语音连接中断，请重试或检查网络");
          }
        };

        updatePhase("connecting");
        const offer = await pc.createOffer();
        await pc.setLocalDescription(offer);
        if (pcRef.current !== pc || callIdRef.current !== callId) return;
        const sent = sendSignal({
          type: "signal",
          callId,
          deviceId: deviceIdRef.current,
          mode: nextMode,
          to: "device",
          payload: { kind: "offer", sdp: pc.localDescription?.sdp },
        });
        if (!sent) {
          throw new Error("对讲信令通道未连接，请稍后重试");
        }
        timeoutRef.current = window.setTimeout(() => {
          if (phaseRef.current === "requesting" || phaseRef.current === "connecting") {
            fail("设备未响应，请确认客户端在线并允许远程接听");
          }
        }, 20000);
      } catch (err) {
        stream?.getTracks().forEach((track) => track.stop());
        if (callIdRef.current !== callId) return;
        let message = "麦克风启动失败";
        if (err instanceof DOMException) {
          if (err.name === "NotAllowedError") message = "麦克风权限被拒绝，请在浏览器地址栏授予权限";
          else if (err.name === "NotFoundError") message = "未找到可用麦克风";
          else if (err.name === "NotReadableError") message = "麦克风被其他程序占用";
          else message = `麦克风错误：${err.message}`;
        } else if (err instanceof Error && err.message) {
          message = err.message;
        }
        fail(message);
      }
    },
    [
      attachRemoteToRecording,
      clearTimers,
      fail,
      iceServers,
      sendSignal,
      startRecording,
      updatePhase,
    ],
  );

  const hangup = useCallback(() => {
    const callId = callIdRef.current;
    if (callId) {
      sendSignal({
        type: "hangup",
        callId,
        deviceId: deviceIdRef.current,
        to: "device",
      });
    }
    resetCall("idle");
  }, [resetCall, sendSignal]);

  const toggleMute = useCallback(() => {
    const stream = localStreamRef.current;
    if (!stream) return;
    const nextMuted = !muted;
    if (audioGuardRef.current) {
      audioGuardRef.current.setUserMuted(nextMuted);
    } else {
      stream.getAudioTracks().forEach((track) => {
        track.enabled = !nextMuted;
      });
    }
    setMuted(nextMuted);
  }, [muted]);

  const dismissError = useCallback(() => {
    setError(null);
    updatePhase("idle");
  }, [updatePhase]);

  return {
    phase,
    mode,
    error,
    muted,
    elapsed,
    localStream,
    remoteStream,
    start,
    hangup,
    toggleMute,
    dismissError,
  };
}
