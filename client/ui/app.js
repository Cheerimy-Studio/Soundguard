(() => {
  "use strict";

  const tauri = window.__TAURI__;
  const isDesktop = Boolean(tauri && tauri.core && typeof tauri.core.invoke === "function");
  const isPreview = !isDesktop;
  const $ = (id) => document.getElementById(id);

  const els = {
    setupScreen: $("setup-screen"),
    setupForm: $("setup-form"),
    setupServerUrl: $("setup-server-url"),
    setupDeviceId: $("setup-device-id"),
    setupDeviceToken: $("setup-device-token"),
    setupPassword: $("setup-password"),
    setupConfirmPassword: $("setup-confirm-password"),
    setupError: $("setup-error"),
    setupSubmit: $("setup-submit"),
    connectionPill: $("connection-pill"),
    connectionText: $("connection-text"),
    deviceName: $("device-name"),
    notificationCard: $("notification-card"),
    notificationTitle: $("notification-title"),
    notificationContent: $("notification-content"),
    meterState: $("meter-state"),
    meterStateText: $("meter-state-text"),
    currentDb: $("current-db"),
    averageDb: $("average-db"),
    peakDb: $("peak-db"),
    thresholdDb: $("threshold-db"),
    nextReport: $("next-report"),
    levelFill: $("level-fill"),
    levelThreshold: $("level-threshold"),
    monitorCard: document.querySelector(".monitor-card"),
    noiseBanner: $("noise-banner"),
    noiseTitle: $("noise-title"),
    noiseText: $("noise-text"),
    errorBanner: $("error-banner"),
    errorText: $("error-text"),
    errorRetry: $("error-retry"),
    windowFill: $("window-fill"),
    windowHint: $("window-hint"),
    waveCanvas: $("wave-canvas"),
    signalState: $("signal-state"),
    intercomTitle: $("intercom-title"),
    intercomDesc: $("intercom-desc"),
    autoAnswerToggle: $("auto-answer-toggle"),
    serverUrl: $("server-url"),
    deviceId: $("device-id"),
    lastHeartbeat: $("last-heartbeat"),
    lastError: $("last-error"),
    networkState: $("network-state"),
    networkStateText: $("network-state-text"),
    settingsButton: $("settings-button"),
    openSettings: $("open-settings"),
    settingsAuthModal: $("settings-auth-modal"),
    settingsAuthTitle: $("settings-auth-title"),
    settingsAuthCopy: $("settings-auth-copy"),
    settingsPin: $("settings-pin"),
    settingsAuthError: $("settings-auth-error"),
    settingsAuthCancel: $("settings-auth-cancel"),
    settingsAuthConfirm: $("settings-auth-confirm"),
    settingsBackdrop: $("settings-backdrop"),
    settingsDrawer: $("settings-drawer"),
    settingsClose: $("settings-close"),
    settingsCancel: $("settings-cancel"),
    settingsForm: $("settings-form"),
    settingsSave: $("settings-save"),
    settingsError: $("settings-error"),
    serverUrlInput: $("server-url-input"),
    deviceIdInput: $("device-id-input"),
    deviceTokenInput: $("device-token-input"),
    toggleToken: $("toggle-token"),
    calibrationRange: $("calibration-range"),
    alertVolume: $("alert-volume"),
    calibrationNumber: $("calibration-number"),
    calibrationValue: $("calibration-value"),
    retryAudio: $("retry-audio"),
    closeButton: $("exit-button"),
    closeModal: $("close-modal"),
    cancelClose: $("cancel-close"),
    confirmClose: $("confirm-close"),
    exitPin: $("exit-pin"),
    exitError: $("exit-error"),
    incomingModal: $("incoming-modal"),
    incomingTitle: $("incoming-title"),
    incomingDesc: $("incoming-desc"),
    acceptCall: $("accept-call"),
    declineCall: $("decline-call"),
    activeCall: $("active-call"),
    callMode: $("call-mode"),
    callTimer: $("call-timer"),
    muteCall: $("mute-call"),
    endCall: $("end-call"),
    unlockAudio: $("unlock-audio"),
    remoteAudio: $("remote-audio"),
    callRecords: $("call-records"),
    callAudio: $("call-audio"),
    refreshCalls: $("refresh-calls"),
    toastStack: $("toast-stack"),
    githubButton: $("github-button"),
  };

  const state = {
    config: null,
    status: null,
    meter: {
      currentDb: null,
      averageDb: null,
      peakDb: null,
      progress: 0,
      thresholdDb: 65,
      capturing: false,
      suspended: false,
      ready: false,
      aecEnabled: false,
      systemPlaying: false,
      error: null,
      sourceRate: 0,
      channels: 0,
    },
    callActive: false,
    authAction: "settings",
    setupRequired: false,
    wavePoints: [],
    lastOver: false,
    ws: null,
    wsGeneration: 0,
    wsReconnectTimer: null,
    autoAnswer: localStorage.getItem("soundguard.autoAnswer") !== "false",
    alertVolume: Number(localStorage.getItem("soundguard.alertVolume") || "22"),
    call: null,
    incomingOffer: null,
    incomingTimer: null,
    iceQueue: [],
    remoteDescriptionSet: false,
    callStartedAt: null,
    callRecords: [],
    callRecordTimer: null,
    previewTimer: null,
    previewTimeouts: [],
    callAudioUrl: null,
    chartResizeObserver: null,
  };

  function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
  }

  function formatDb(value) {
    return value === null || value === undefined || !Number.isFinite(value)
      ? "--"
      : Number(value).toFixed(1);
  }

  function parseDate(value) {
    if (!value) return null;
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  function formatRelative(value) {
    const date = parseDate(value);
    if (!date) return "--";
    const seconds = Math.round((Date.now() - date.getTime()) / 1000);
    if (seconds < 5) return "刚刚";
    if (seconds < 60) return `${seconds} 秒前`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`;
    return date.toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" });
  }

  function formatCallTime(value) {
    const date = parseDate(value);
    if (!date) return "--";
    const now = new Date();
    const sameDay =
      date.getFullYear() === now.getFullYear() &&
      date.getMonth() === now.getMonth() &&
      date.getDate() === now.getDate();
    const time = date.toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    return sameDay
      ? `今天 ${time}`
      : `${date.toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" })} ${time}`;
  }

  function formatDuration(seconds) {
    const total = Math.max(0, Math.floor(seconds || 0));
    const mins = Math.floor(total / 60);
    const secs = total % 60;
    return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }

  function toast(message, type = "info", duration = 4000) {
    const node = document.createElement("div");
    node.className = `toast ${type}`;
    node.textContent = message;
    els.toastStack.appendChild(node);
    while (els.toastStack.children.length > 4) {
      els.toastStack.firstElementChild?.remove();
    }
    window.setTimeout(() => {
      node.style.opacity = "0";
      node.style.transform = "translateY(4px)";
      window.setTimeout(() => node.remove(), 200);
    }, duration);
  }

  let beepContext = null;
  function beep(frequency = 840, duration = 0.18, volume = 0.06) {
    try {
      const AudioContextClass = window.AudioContext || window.webkitAudioContext;
      if (!AudioContextClass) return;
      if (!beepContext) beepContext = new AudioContextClass();
      if (beepContext.state === "suspended") void beepContext.resume();
      const oscillator = beepContext.createOscillator();
      const gain = beepContext.createGain();
      oscillator.type = "sine";
      oscillator.frequency.value = frequency;
      gain.gain.setValueAtTime(volume, beepContext.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.0001, beepContext.currentTime + duration);
      oscillator.connect(gain);
      gain.connect(beepContext.destination);
      oscillator.start();
      oscillator.stop(beepContext.currentTime + duration);
    } catch {
      // Sound feedback is optional.
    }
  }

  // A longer, louder three-tone alarm so an over-limit window is noticed
  // even in a noisy classroom.
  function overLimitAlarm() {
    const volume = state.alertVolume / 100;
    const pattern = [
      [880, 0, 0.28],
      [880, 0.36, 0.28],
      [880, 0.72, 0.28],
    ];
    for (const [frequency, delay, duration] of pattern) {
      window.setTimeout(() => beep(frequency, duration, volume), delay * 1000);
    }
  }

  function signalingUrl(config) {
    const base = String(config.serverUrl || "").replace(/\/+$/, "");
    const wsBase = base.replace(/^http/i, "ws");
    return `${wsBase}/api/device/ws?token=${encodeURIComponent(
      config.deviceToken,
    )}&deviceId=${encodeURIComponent(config.deviceId)}`;
  }

  function apiUrl(path) {
    const base = String(state.config?.serverUrl || "").replace(/\/+$/, "");
    return `${base}${path}`;
  }

  /* ---------------- Chart ---------------- */

  function setupChart() {
    const canvas = els.waveCanvas;
    if (!canvas) return;
    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.max(1, window.devicePixelRatio || 1);
      canvas.width = Math.max(1, Math.round(rect.width * dpr));
      canvas.height = Math.max(1, Math.round(rect.height * dpr));
      const ctx = canvas.getContext("2d");
      if (ctx) {
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      }
    };
    resize();
    state.chartResizeObserver = new ResizeObserver(resize);
    state.chartResizeObserver.observe(canvas);
    const draw = () => {
      drawWaveChart();
      if (!document.hidden) {
        window.requestAnimationFrame(draw);
      } else {
        window.setTimeout(() => window.requestAnimationFrame(draw), 700);
      }
    };
    window.requestAnimationFrame(draw);
  }

  function drawWaveChart() {
    const canvas = els.waveCanvas;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const rect = canvas.getBoundingClientRect();
    const width = rect.width;
    const height = rect.height;
    if (width <= 0 || height <= 0) return;
    ctx.clearRect(0, 0, width, height);

    const padding = { left: 34, right: 42, top: 10, bottom: 20 };
    const plotW = width - padding.left - padding.right;
    const plotH = height - padding.top - padding.bottom;
    const now = Date.now();
    const windowMs = 30_000;
    const points = state.wavePoints.filter((point) => now - point.t <= windowMs);
    const threshold = Number(state.meter.thresholdDb || 65);
    const average = state.meter.averageDb;
    const values = points.map((point) => point.db);
    if (Number.isFinite(average)) values.push(Number(average));
    values.push(threshold);
    const rawMin = Math.min(30, ...values);
    const rawMax = Math.max(80, ...values) + 6;
    const yMin = Math.floor(rawMin / 10) * 10;
    const yMax = Math.ceil(rawMax / 10) * 10;
    const yFor = (db) => padding.top + plotH * (1 - (db - yMin) / (yMax - yMin));
    const xFor = (t) => padding.left + plotW * (1 - (now - t) / windowMs);

    ctx.font = "9px Inter, 'Segoe UI', 'Microsoft YaHei', sans-serif";
    ctx.lineWidth = 1;

    for (let index = 0; index <= 4; index += 1) {
      const value = yMin + ((yMax - yMin) * index) / 4;
      const y = yFor(value);
      ctx.strokeStyle = "#edf1f5";
      ctx.beginPath();
      ctx.moveTo(padding.left, y);
      ctx.lineTo(padding.left + plotW, y);
      ctx.stroke();
      ctx.fillStyle = "#9aa7b7";
      ctx.textAlign = "right";
      ctx.fillText(String(Math.round(value)), padding.left - 6, y + 3);
    }

    ctx.textAlign = "left";
    ctx.fillStyle = "#9aa7b7";
    ctx.fillText("30 秒前", padding.left, height - 4);
    ctx.textAlign = "right";
    ctx.fillText("现在", padding.left + plotW, height - 4);

    if (points.length >= 2) {
      const gradient = ctx.createLinearGradient(0, padding.top, 0, padding.top + plotH);
      gradient.addColorStop(0, "rgba(13, 122, 130, 0.18)");
      gradient.addColorStop(1, "rgba(13, 122, 130, 0.01)");

      ctx.beginPath();
      points.forEach((point, index) => {
        const x = xFor(point.t);
        const y = yFor(point.db);
        if (index === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      const lastX = xFor(points[points.length - 1].t);
      const firstX = xFor(points[0].t);
      ctx.lineTo(lastX, padding.top + plotH);
      ctx.lineTo(firstX, padding.top + plotH);
      ctx.closePath();
      ctx.fillStyle = gradient;
      ctx.fill();

      ctx.beginPath();
      points.forEach((point, index) => {
        const x = xFor(point.t);
        const y = yFor(point.db);
        if (index === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.strokeStyle = "#0d7a82";
      ctx.lineWidth = 2;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      ctx.stroke();

      const last = points[points.length - 1];
      ctx.beginPath();
      ctx.arc(xFor(last.t), yFor(last.db), 3, 0, Math.PI * 2);
      ctx.fillStyle = "#0d7a82";
      ctx.fill();
      ctx.beginPath();
      ctx.arc(xFor(last.t), yFor(last.db), 5.5, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(13, 122, 130, .13)";
      ctx.fill();
    } else {
      ctx.fillStyle = "#9aa7b7";
      ctx.textAlign = "center";
      ctx.fillText("等待音频数据", padding.left + plotW / 2, padding.top + plotH / 2);
    }

    const thresholdY = yFor(threshold);
    ctx.setLineDash([5, 4]);
    ctx.strokeStyle = "#d97706";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(padding.left, thresholdY);
    ctx.lineTo(padding.left + plotW, thresholdY);
    ctx.stroke();
    ctx.fillStyle = "#b45309";
    ctx.textAlign = "left";
    ctx.fillText(`阈值 ${threshold.toFixed(0)}`, padding.left + plotW + 6, thresholdY + 3);

    if (Number.isFinite(average)) {
      const averageY = yFor(Number(average));
      ctx.strokeStyle = "#2563eb";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(padding.left, averageY);
      ctx.lineTo(padding.left + plotW, averageY);
      ctx.stroke();
      let averageLabelY = averageY;
      if (Math.abs(averageY - thresholdY) < 12) {
        averageLabelY = clamp(
          averageY + (averageY < thresholdY ? -11 : 11),
          padding.top + 8,
          padding.top + plotH - 2,
        );
      }
      ctx.fillStyle = "#2563eb";
      ctx.textAlign = "left";
      ctx.fillText(
        `均值 ${Number(average).toFixed(0)}`,
        padding.left + plotW + 6,
        averageLabelY + 3,
      );
    }
    ctx.setLineDash([]);
  }

  /* ---------------- Render ---------------- */

  function meterTone() {
    const meter = state.meter;
    const status = state.status;
    if (meter.suspended || state.callActive) {
      return { label: "通话中", tone: "warn" };
    }
    if (!meter.aecEnabled && meter.systemPlaying) {
      return { label: "电脑播放中", tone: "warn" };
    }
    if (meter.error) return { label: "麦克风异常", tone: "danger" };
    if (!meter.ready && meter.capturing) return { label: "音频启动中", tone: "warn" };
    if (status && !status.connected) {
      return { label: meter.capturing ? "本地采集" : "等待连接", tone: "warn" };
    }
    if (status && status.desiredMonitoring === false) {
      return { label: "采集已暂停", tone: "warn" };
    }
    if (!meter.capturing) return { label: "已暂停", tone: "warn" };
    const average = meter.averageDb;
    const threshold = Number(meter.thresholdDb || state.status?.thresholdDb || 65);
    if (average !== null && Number(average) >= threshold) {
      return { label: "噪音超限", tone: "danger" };
    }
    return { label: "采集中", tone: "good" };
  }

  function renderMeter() {
    const meter = state.meter;
    const status = state.status;
    const threshold = Number(meter.thresholdDb || status?.thresholdDb || 65);
    const suspended = Boolean(meter.suspended || state.callActive);
    const ignoringSystem = Boolean(!meter.aecEnabled && meter.systemPlaying);
    const average = suspended || ignoringSystem ? null : meter.averageDb;
    const current = suspended || ignoringSystem ? null : meter.currentDb;
    const peak = suspended || ignoringSystem ? null : meter.peakDb;
    const over = average !== null && Number(average) >= threshold;
  const rollingOver15s = meter.rolling15sDb !== null && Number(meter.rolling15sDb) >= threshold;

    els.deviceName.textContent = status?.deviceName || "现场采集节点";
    els.currentDb.textContent = formatDb(current);
    els.averageDb.textContent = formatDb(average);
    els.peakDb.textContent = formatDb(peak);
    els.thresholdDb.textContent = threshold.toFixed(1);

    const ratio = current === null ? 0 : clamp((Number(current) - 30) / 80, 0, 1);
    const thresholdRatio = clamp((threshold - 30) / 80, 0, 1);
    els.levelFill.style.width = `${ratio * 100}%`;
    els.levelFill.classList.toggle("is-over", over);
    els.levelThreshold.style.left = `${thresholdRatio * 100}%`;
    els.monitorCard.classList.toggle("is-over", over);

    const remaining =
      !suspended && !ignoringSystem && meter.capturing && meter.ready
        ? Math.max(1, Math.ceil((1 - clamp(meter.progress || 0, 0, 1)) * 30))
        : null;
    els.nextReport.textContent = remaining === null ? "--" : String(remaining);
    els.windowFill.style.width = `${clamp((meter.progress || 0) * 100, 0, 100)}%`;
    els.windowHint.textContent = suspended
      ? "通话中，分贝监测暂停"
      : ignoringSystem
        ? "已忽略电脑播放的声音"
        : !meter.capturing
          ? "采集已暂停"
          : `已录制 ${Math.min(30, Math.round((meter.progress || 0) * 30))} / 30 秒`;

    const tone = meterTone();
    els.meterState.className = `state-badge ${tone.tone}`;
    els.meterStateText.textContent = tone.label;

    if (over && meter.capturing && !suspended && !ignoringSystem) {
      els.noiseBanner.hidden = false;
      els.noiseTitle.textContent = "环境声音超标";
      els.noiseText.textContent = `平均 ${formatDb(average)} dB，阈值 ${threshold.toFixed(1)} dB。请保持安静！`;
    } else {
      els.noiseBanner.hidden = true;
    }

    if (meter.error && !suspended) {
      els.errorBanner.hidden = false;
      els.errorText.textContent = meter.error;
    } else {
      els.errorBanner.hidden = true;
    }

    const rolling15 = meter.rolling15sDb;
    if (rolling15 !== null && Number(rolling15) >= threshold) {
      els.currentDb.classList.add("db-warning-15s");
    } else {
      els.currentDb.classList.remove("db-warning-15s");
    }
    if (over && !suspended && !ignoringSystem && !state.lastOver) {
      state.lastOver = true;
      overLimitAlarm();
    } else if (!over && average !== null && Number(average) < threshold - 2) {
      state.lastOver = false;
    }

    if (current !== null) {
      const now = Date.now();
      state.wavePoints.push({ t: now, db: Number(current) });
      state.wavePoints = state.wavePoints.filter((point) => now - point.t <= 32_000);
      if (state.wavePoints.length > 240) state.wavePoints.shift();
    }
  }

  function renderStatus() {
    const status = state.status;
    if (!status) return;
    const connected = Boolean(status.connected);
    els.connectionPill.classList.toggle("is-online", connected);
    els.connectionPill.classList.toggle("is-offline", !connected);
    els.connectionText.textContent = connected ? "服务已连接" : "服务离线";

    els.networkState.classList.toggle("is-online", connected);
    els.networkState.classList.toggle("is-offline", !connected);
    els.networkStateText.textContent = connected ? "已连接" : "离线";
    els.serverUrl.textContent = status.serverUrl || "--";
    els.serverUrl.title = status.serverUrl || "";
    els.deviceId.textContent = status.deviceId || "--";
    els.lastHeartbeat.textContent = status.lastHeartbeatAt
      ? formatRelative(status.lastHeartbeatAt)
      : "尚未连接";

    if (status.lastError && !connected) {
      els.lastError.hidden = false;
      els.lastError.textContent = status.lastError;
    } else {
      els.lastError.hidden = true;
    }

    if (status.deviceName) els.deviceName.textContent = status.deviceName;
    const noticeTitle = (status.notificationTitle || "").trim();
    const noticeContent = (status.notificationContent || "").trim();
    const hasNotice = Boolean(status.notificationEnabled && (noticeTitle || noticeContent));
    els.notificationCard.classList.toggle("is-empty", !hasNotice);
    els.notificationTitle.textContent = hasNotice
      ? noticeTitle || "通知"
      : "暂无通知";
    els.notificationContent.textContent = hasNotice ? noticeContent : "";
    els.meterState.className = `state-badge ${meterTone().tone}`;
    els.meterStateText.textContent = meterTone().label;
    renderMeter();
  }

  function updateTimes() {
    if (state.status?.lastHeartbeatAt) {
      els.lastHeartbeat.textContent = formatRelative(state.status.lastHeartbeatAt);
    }
    if (state.call && state.callStartedAt) {
      els.callTimer.textContent = formatDuration(
        (Date.now() - state.callStartedAt) / 1000,
      );
    }
  }

  /* ---------------- Call records ---------------- */

  function renderCallRecords() {
    els.callRecords.innerHTML = "";
    if (!state.callRecords.length) {
      const empty = document.createElement("div");
      empty.className = "empty-calls";
      empty.textContent = "暂无通话记录";
      els.callRecords.appendChild(empty);
      return;
    }
    for (const record of state.callRecords) {
      const row = document.createElement("div");
      row.className = "call-record";

      const icon = document.createElement("div");
      icon.className = `call-record-icon ${
        record.mode === "broadcast" ? "broadcast" : ""
      }`;
      icon.innerHTML =
        record.mode === "broadcast"
          ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M11 5 6 9H3v6h3l5 4V5Z"/><path d="M15 9a4 4 0 0 1 0 6M18 6a8 8 0 0 1 0 12"/></svg>'
          : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M22 16.92v3a2 2 0 0 1-2.18 2 19.8 19.8 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.12 4.18 2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72c.13.96.36 1.9.7 2.81a2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45c.91.34 1.85.57 2.81.7A2 2 0 0 1 22 16.92Z"/></svg>';

      const main = document.createElement("div");
      main.className = "call-record-main";
      const title = document.createElement("div");
      title.className = "call-record-title";
      title.textContent =
        record.mode === "broadcast" ? "远程广播" : "双向对讲";
      const duration = document.createElement("span");
      duration.textContent = formatDuration(record.durationSecs);
      title.appendChild(duration);
      const meta = document.createElement("div");
      meta.className = "call-record-meta";
      meta.textContent = formatCallTime(record.startedAt);
      main.append(title, meta);

      const action = document.createElement("button");
      action.type = "button";
      action.className = "call-record-action";
      action.textContent = record.hasAudio ? "播放" : "无录音";
      action.disabled = !record.hasAudio;
      action.dataset.callId = record.id;
      action.addEventListener("click", () => void playCallRecord(record.id));

      row.append(icon, main, action);
      els.callRecords.appendChild(row);
    }
  }

  async function loadCallRecords() {
    if (isPreview) {
      state.callRecords = [];
      renderCallRecords();
      return;
    }
    const config = state.config;
    if (!config?.deviceId || !config?.deviceToken) return;
    try {
      const query = new URLSearchParams({
        deviceId: config.deviceId,
        limit: "5",
      });
      const response = await fetch(apiUrl(`/api/device/calls?${query}`), {
        headers: { "X-Device-Token": config.deviceToken },
      });
      if (!response.ok) {
        state.callRecords = [];
        renderCallRecords();
        return;
      }
      state.callRecords = await response.json();
      renderCallRecords();
    } catch {
      // Keep the previous list when the network is unavailable.
    }
  }

  async function playCallRecord(callId) {
    const config = state.config;
    if (!config) return;
    try {
      const response = await fetch(apiUrl(`/api/device/calls/${encodeURIComponent(callId)}/audio`), {
        headers: { "X-Device-Token": config.deviceToken },
      });
      if (!response.ok) {
        let message = "无法读取通话录音";
        try {
          const body = await response.json();
          if (body?.error) message = body.error;
        } catch {
          // Ignore parse errors.
        }
        throw new Error(message);
      }
      const blob = await response.blob();
      if (state.callAudioUrl) URL.revokeObjectURL(state.callAudioUrl);
      state.callAudioUrl = URL.createObjectURL(blob);
      els.callAudio.src = state.callAudioUrl;
      els.callAudio.hidden = false;
      await els.callAudio.play();
    } catch (error) {
      toast(error instanceof Error ? error.message : "播放失败", "error");
    }
  }

  function scheduleCallRecordRefresh() {
    window.setTimeout(() => void loadCallRecords(), 1800);
  }

  /* ---------------- Signal state & WebRTC ---------------- */

  function setSignalState(connected, label) {
    els.signalState.classList.toggle("is-online", connected);
    els.signalState.textContent = label || (connected ? "已连接" : "未连接");
    if (!state.call && !state.incomingOffer) {
      els.intercomTitle.textContent = connected
        ? "等待管理端呼叫"
        : isPreview
          ? "预览模式"
          : "正在连接通话通道";
      els.intercomDesc.textContent = isPreview
        ? "浏览器预览不启用远程通话。"
        : connected
          ? "管理员可从后台发起双向对讲或单向广播。"
          : "通话通道正在自动重连。";
    }
  }

  function autoAnswerUi() {
    els.autoAnswerToggle.classList.toggle("is-on", state.autoAnswer);
    els.autoAnswerToggle.setAttribute("aria-checked", state.autoAnswer ? "true" : "false");
  }

  function sendSignal(payload) {
    if (!state.ws || state.ws.readyState !== WebSocket.OPEN) return false;
    try {
      state.ws.send(JSON.stringify(payload));
      return true;
    } catch {
      return false;
    }
  }

  function connectSignaling() {
    const generation = ++state.wsGeneration;
    if (state.wsReconnectTimer) {
      window.clearTimeout(state.wsReconnectTimer);
      state.wsReconnectTimer = null;
    }
    if (state.ws) {
      try {
        state.ws.close();
      } catch {
        // Ignore.
      }
      state.ws = null;
    }
    if (isPreview) {
      setSignalState(false, "预览");
      return;
    }
    if (!state.config?.serverUrl || !state.config?.deviceToken) {
      setSignalState(false, "未配置");
      return;
    }

    let socket;
    try {
      socket = new WebSocket(signalingUrl(state.config));
    } catch {
      setSignalState(false, "地址无效");
      scheduleSignalingReconnect(generation);
      return;
    }
    state.ws = socket;
    socket.addEventListener("open", () => {
      if (generation !== state.wsGeneration) return;
      setSignalState(true, "已连接");
    });
    socket.addEventListener("message", (event) => {
      if (generation !== state.wsGeneration) return;
      let message;
      try {
        message = JSON.parse(event.data);
      } catch {
        return;
      }
      handleSignalMessage(message);
    });
    socket.addEventListener("close", () => {
      if (generation !== state.wsGeneration) return;
      endCall(false);
      state.ws = null;
      setSignalState(false, "未连接");
      scheduleSignalingReconnect(generation);
    });
    socket.addEventListener("error", () => {
      try {
        socket.close();
      } catch {
        // Ignore.
      }
    });
  }

  function scheduleSignalingReconnect(generation) {
    if (isPreview) return;
    if (state.wsReconnectTimer) window.clearTimeout(state.wsReconnectTimer);
    state.wsReconnectTimer = window.setTimeout(() => {
      if (generation === state.wsGeneration) connectSignaling();
    }, 3200);
  }

  function handleSignalMessage(message) {
    if (!message || typeof message.type !== "string") return;
    if (message.type === "ready") {
      setSignalState(true, "已连接");
      return;
    }
    if (message.type === "hangup") {
      if (state.call && message.callId === state.call.callId) {
        endCall(false);
        toast("管理端已结束通话", "info");
      }
      if (state.incomingOffer && message.callId === state.incomingOffer.callId) {
        clearIncomingOffer();
      }
      return;
    }
    if (message.type !== "signal") return;
    const callId = message.callId;
    const payload = message.payload || {};
    if (!callId) return;

    if (payload.kind === "offer" && payload.sdp) {
      if (state.call) {
        sendReject(callId, "busy");
        return;
      }
      const offer = {
        callId,
        mode: message.mode === "broadcast" ? "broadcast" : "intercom",
        sdp: payload.sdp,
      };
      if (state.autoAnswer) {
        void acceptOffer(offer);
      } else {
        showIncomingOffer(offer);
      }
      return;
    }

    if (payload.kind === "ice" && payload.candidate) {
      if (state.call && state.call.callId === callId && state.call.pc) {
        if (state.remoteDescriptionSet) {
          void state.call.pc.addIceCandidate(payload.candidate).catch(() => undefined);
        } else {
          state.iceQueue.push(payload.candidate);
        }
      } else if (state.incomingOffer && state.incomingOffer.callId === callId) {
        state.iceQueue.push(payload.candidate);
      }
    }
  }

  function showIncomingOffer(offer) {
    clearIncomingTimeout();
    state.incomingOffer = offer;
    state.iceQueue = [];
    els.incomingTitle.textContent =
      offer.mode === "broadcast" ? "管理端发起远程广播" : "管理端发起双向对讲";
    els.incomingDesc.textContent =
      offer.mode === "broadcast"
        ? "接听后播放管理端音频，不会开启现场麦克风。"
        : "接听后开启现场麦克风，与管理端进行双向通话。";
    els.incomingModal.hidden = false;
    beep(700, 0.2);
    window.setTimeout(() => beep(940, 0.18), 240);
    state.incomingTimer = window.setTimeout(() => {
      if (state.incomingOffer && state.incomingOffer.callId === offer.callId) {
        sendReject(offer.callId, "timeout");
        clearIncomingOffer();
        toast("来电已超时", "info");
      }
    }, 30000);
  }

  function clearIncomingTimeout() {
    if (state.incomingTimer) {
      window.clearTimeout(state.incomingTimer);
      state.incomingTimer = null;
    }
  }

  function clearIncomingOffer() {
    clearIncomingTimeout();
    state.incomingOffer = null;
    els.incomingModal.hidden = true;
    if (!state.call) {
      els.intercomTitle.textContent = "等待管理端呼叫";
      els.intercomDesc.textContent = "支持双向对讲与单向广播。";
    }
  }

  function setCallActive(active) {
    state.callActive = active;
    if (isDesktop) {
      void tauri.core.invoke("set_call_active", { active }).catch((error) => {
        console.warn("Could not update call monitoring state", error);
      });
    }
  }

  function sendReject(callId, reason) {
    sendSignal({
      type: "signal",
      callId,
      to: "admin",
      payload: { kind: "reject", reason },
    });
  }

  function createCallAudioGuard(localStream) {
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) return null;
    try {
      const context = new AudioContextClass();
      const tracks = localStream.getAudioTracks();
      let active = true;
      let sensor = null;
      let animation = 0;
      let ducked = false;
      let userMuted = false;
      let lastLoud = 0;
      const apply = () => {
        tracks.forEach((track) => {
          track.enabled = !(userMuted || ducked);
        });
      };
      const guard = {
        attachRemote(remoteStream) {
          if (sensor || !active) return;
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
          } catch (error) {
            console.warn("anti-feedback guard unavailable", error);
          }
        },
        setUserMuted(muted) {
          userMuted = muted;
          apply();
        },
        resume() {
          void context.resume().catch(() => undefined);
        },
        close() {
          active = false;
          if (animation) cancelAnimationFrame(animation);
          try {
            if (sensor) sensor.disconnect();
          } catch (error) {
            // Ignore.
          }
          ducked = false;
          userMuted = false;
          apply();
          void context.close().catch(() => undefined);
        },
      };
      void context.resume().catch(() => undefined);
      return guard;
    } catch (error) {
      console.warn("anti-feedback guard unavailable", error);
      return null;
    }
  }

  async function acceptOffer(offer) {
    if (state.call) {
      sendReject(offer.callId, "busy");
      return;
    }
    clearIncomingOffer();
    const mode = offer.mode === "broadcast" ? "broadcast" : "intercom";
    const call = {
      callId: offer.callId,
      mode,
      pc: null,
      stream: null,
      micStream: null,
      guard: null,
      muted: false,
    };
    state.call = call;
    setCallActive(true);
    state.iceQueue = [];
    state.remoteDescriptionSet = false;
    state.callStartedAt = Date.now();
    els.callTimer.textContent = "00:00";
    els.callMode.textContent = mode === "broadcast" ? "远程广播中" : "正在接通";
    els.activeCall.hidden = false;
    els.unlockAudio.hidden = true;
    els.muteCall.hidden = mode === "broadcast";
    els.muteCall.classList.remove("is-muted");
    els.muteCall.querySelector("span").textContent = "静音";
    els.intercomTitle.textContent =
      mode === "broadcast" ? "正在接收广播" : "双向对讲进行中";
    els.intercomDesc.textContent =
      mode === "broadcast"
        ? "正在播放管理端广播音频。"
        : "已启用回声消除与防啸叫；同场使用时请适当降低扬声器音量。";

    try {
      if (mode === "intercom") {
        if (!window.isSecureContext) {
          throw new Error("系统安全策略禁止使用麦克风");
        }
        if (!navigator.mediaDevices?.getUserMedia) {
          throw new Error("当前运行环境不支持麦克风");
        }
        call.stream = await navigator.mediaDevices.getUserMedia({
          audio: {
            echoCancellation: true,
            noiseSuppression: true,
            autoGainControl: true,
            channelCount: 1,
          },
          video: false,
        });
        call.micStream = call.stream;
        call.guard = createCallAudioGuard(call.micStream);
      }

      const pc = new RTCPeerConnection({
        iceServers: defaultIceServers(),
        iceCandidatePoolSize: 4,
      });
      call.pc = pc;
      if (call.stream) {
        call.stream.getTracks().forEach((track) => pc.addTrack(track, call.stream));
      }

      pc.onicecandidate = (event) => {
        if (!event.candidate) return;
        sendSignal({
          type: "signal",
          callId: call.callId,
          to: "admin",
          payload: { kind: "ice", candidate: event.candidate.toJSON() },
        });
      };
      pc.ontrack = (event) => {
        const stream = event.streams[0] || new MediaStream([event.track]);
        call.guard?.attachRemote(stream);
        els.remoteAudio.srcObject = stream;
        const playPromise = els.remoteAudio.play();
        if (playPromise && typeof playPromise.catch === "function") {
          playPromise.catch(() => {
            els.unlockAudio.hidden = false;
          });
        }
      };
      pc.onconnectionstatechange = () => {
        if (!state.call || state.call.pc !== pc) return;
        if (pc.connectionState === "connected") {
          els.callMode.textContent =
            mode === "broadcast" ? "远程广播中" : "双向对讲中";
        } else if (pc.connectionState === "failed") {
          toast("通话连接中断", "error");
          endCall(true);
        }
      };

      await pc.setRemoteDescription({ type: "offer", sdp: offer.sdp });
      state.remoteDescriptionSet = true;
      for (const candidate of state.iceQueue.splice(0)) {
        await pc.addIceCandidate(candidate).catch(() => undefined);
      }
      const answer = await pc.createAnswer();
      await pc.setLocalDescription(answer);
      sendSignal({
        type: "signal",
        callId: call.callId,
        to: "admin",
        payload: { kind: "answer", sdp: pc.localDescription?.sdp },
      });
    } catch (error) {
      sendReject(offer.callId, "microphone-unavailable");
      endCall(false);
      toast(error instanceof Error ? error.message : "无法建立通话", "error");
    }
  }


  // --- TOTP (RFC 6238) ---
  const BASE32_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

  function base32Decode(encoded) {
    const bytes = [];
    let bits = 0, value = 0;
    for (const ch of encoded.toUpperCase()) {
      const idx = BASE32_CHARS.indexOf(ch);
      if (idx === -1) continue;
      value = (value << 5) | idx;
      bits += 5;
      if (bits >= 8) {
        bytes.push((value >>> (bits - 8)) & 0xFF);
        bits -= 8;
      }
    }
    return new Uint8Array(bytes);
  }

  async function totpCode(secret) {
    if (!secret) return "------";
    const key = base32Decode(secret);
    const counter = Math.floor(Date.now() / 30000);
    const counterBuf = new ArrayBuffer(8);
    new DataView(counterBuf).setUint32(4, counter);
    const cryptoKey = await crypto.subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-1" }, false, ["sign"]);
    const sig = new Uint8Array(await crypto.subtle.sign("HMAC", cryptoKey, counterBuf));
    const offset = sig[sig.length - 1] & 0x0F;
    const code = ((sig[offset] & 0x7F) << 24 | sig[offset+1] << 16 | sig[offset+2] << 8 | sig[offset+3]) % 1000000;
    return String(code).padStart(6, "0");
  }

  // War mode + TOTP state
  state.warMode = false;
  state.totpSecret = "";

  function updateWarMode() {
    document.body.classList.toggle("war-mode", state.warMode);
  }

  function showTotpVerification(onSuccess) {
    const modal = $("totp-modal");
    if (!modal) { onSuccess(); return; }
    modal.hidden = false;
    $("totp-input").value = "";
    $("totp-error").hidden = true;
    const display = $("totp-display");
    const refresh = async () => {
      display.textContent = await totpCode(state.totpSecret);
    };
    refresh();
    const interval = setInterval(refresh, 5000);
    const cleanup = () => { clearInterval(interval); modal.hidden = true; };
    $("totp-confirm").onclick = async () => {
      const code = $("totp-input").value.trim();
      if (!code) return;
      // Verify locally first (offline capable)
      const expected = await totpCode(state.totpSecret);
      // Also check ±1 step
      const key = base32Decode(state.totpSecret);
      const now = Math.floor(Date.now() / 30000);
      let valid = false;
      for (const offset of [-1, 0, 1]) {
        const cb = new ArrayBuffer(8);
        new DataView(cb).setUint32(4, now + offset);
        const ck = await crypto.subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-1" }, false, ["sign"]);
        const sig = new Uint8Array(await crypto.subtle.sign("HMAC", ck, cb));
        const o = sig[sig.length-1] & 0x0F;
        const c = ((sig[o] & 0x7F) << 24 | sig[o+1] << 16 | sig[o+2] << 8 | sig[o+3]) % 1000000;
        if (String(c).padStart(6, "0") === code) { valid = true; break; }
      }
      if (valid) {
        cleanup();
        onSuccess();
      } else {
        $("totp-error").textContent = "验证码错误";
        $("totp-error").hidden = false;
      }
    };
    $("totp-cancel").onclick = () => { cleanup(); };
  }


  function endCall(sendHangup = true) {
    const call = state.call;
    if (!call) {
      clearIncomingOffer();
      return;
    }
    if (sendHangup) {
      sendSignal({ type: "hangup", callId: call.callId, to: "admin" });
    }
    if (call.pc) {
      call.pc.onicecandidate = null;
      call.pc.ontrack = null;
      call.pc.onconnectionstatechange = null;
      try {
        call.pc.close();
      } catch {
        // Ignore.
      }
    }
    if (call.guard) {
      call.guard.close();
      call.guard = null;
    }
    if (call.micStream) {
      call.micStream.getTracks().forEach((track) => track.stop());
    }
    if (call.stream) {
      call.stream.getTracks().forEach((track) => track.stop());
    }
    els.remoteAudio.srcObject = null;
    els.activeCall.hidden = true;
    els.unlockAudio.hidden = true;
    els.muteCall.hidden = false;
    state.call = null;
    state.callStartedAt = null;
    setCallActive(false);
    state.iceQueue = [];
    state.remoteDescriptionSet = false;
    els.intercomTitle.textContent = "等待管理端呼叫";
    els.intercomDesc.textContent = "支持双向对讲与单向广播。";
    scheduleCallRecordRefresh();
  }

  function toggleMute() {
    const call = state.call;
    if (!call || !call.stream) return;
    call.muted = !call.muted;
    if (call.guard) {
      call.guard.setUserMuted(call.muted);
    } else {
      call.stream.getAudioTracks().forEach((track) => {
        track.enabled = !call.muted;
      });
    }
    els.muteCall.classList.toggle("is-muted", call.muted);
    els.muteCall.querySelector("span").textContent = call.muted ? "取消静音" : "静音";
  }

  function defaultIceServers() {
    const configured = state.status?.iceServers;
    if (Array.isArray(configured) && configured.length) return configured;
    return [
      { urls: "stun:stun.l.google.com:19302" },
      { urls: "stun:stun1.l.google.com:19302" },
    ];
  }

  function setAutoAnswer(value) {
    state.autoAnswer = value;
    localStorage.setItem("soundguard.autoAnswer", value ? "true" : "false");
    autoAnswerUi();
    if (value && state.incomingOffer) {
      const offer = state.incomingOffer;
      void acceptOffer(offer);
    }
  }

  function toggleAutoAnswer() {
    if (state.autoAnswer) {
      if (isDesktop) {
        openAuth("auto-answer-off");
      } else {
        setAutoAnswer(false);
      }
      return;
    }
    setAutoAnswer(true);
  }

  /* ---------------- Status / settings / exit ---------------- */

  function applyStatus(status) {
    state.status = status;
    if (status) {
      if (status.serverUrl) {
        state.config = { ...(state.config || {}), serverUrl: status.serverUrl };
      }
      if (status.deviceId) {
        state.config = { ...(state.config || {}), deviceId: status.deviceId };
      }
      if (typeof status.thresholdDb === "number") {
        state.meter.thresholdDb = status.thresholdDb;
      }
      if (typeof status.autoRecordEvidence === "boolean") {
        state.meter.autoRecordEvidence = status.autoRecordEvidence;
      }
    }
    renderStatus();
  }

  function applyMeter(meter) {
    state.meter = { ...state.meter, ...meter };
    renderMeter();
  }

  function openAuth(action = "settings") {
    state.authAction = action;
    if (action === "auto-answer-off") {
      els.settingsAuthTitle.textContent = "关闭自动接听";
      els.settingsAuthCopy.textContent = "关闭自动接听需要本地密码，请输入首次配置时设置的密码。";
    } else {
      els.settingsAuthTitle.textContent = "本地密码验证";
      els.settingsAuthCopy.textContent =
        "设置中包含设备令牌和麦克风校准信息，请输入首次配置时设置的本地密码。";
    }
    els.settingsPin.value = "";
    els.settingsAuthError.hidden = true;
    els.settingsAuthModal.hidden = false;
    window.setTimeout(() => els.settingsPin.focus(), 100);
  }

  function openSettingsAuth() {
    openAuth("settings");
  }

  function closeSettingsAuth() {
    els.settingsAuthModal.hidden = true;
    els.settingsPin.value = "";
    els.settingsAuthError.hidden = true;
  }

  async function verifySettingsPin() {
    const pin = els.settingsPin.value;
    if (!pin) {
      els.settingsAuthError.hidden = false;
      els.settingsAuthError.textContent = "请输入本地密码";
      return;
    }
    if (!isDesktop) {
      closeSettingsAuth();
      if (state.authAction === "auto-answer-off") {
        setAutoAnswer(false);
        toast("自动接听已关闭", "success");
      } else {
        openSettings();
      }
      return;
    }
    els.settingsAuthConfirm.disabled = true;
    els.settingsAuthError.hidden = true;
    try {
      await tauri.core.invoke("verify_pin", { pin });
      closeSettingsAuth();
      if (state.authAction === "auto-answer-off") {
        setAutoAnswer(false);
        toast("自动接听已关闭", "success");
      } else {
        openSettings();
      }
    } catch (error) {
      els.settingsAuthError.hidden = false;
      els.settingsAuthError.textContent =
        typeof error === "string" ? error : error?.message || "密码验证失败";
    } finally {
      els.settingsAuthConfirm.disabled = false;
    }
  }

  function openSettings() {
    if (!state.config) return;
    els.serverUrlInput.value = state.config.serverUrl || "";
    els.deviceIdInput.value = state.config.deviceId || "";
    els.deviceTokenInput.value = state.config.deviceToken || "";
    const calibration = Number(state.config.calibrationOffsetDb ?? 90);
    els.calibrationRange.value = String(calibration);
    els.calibrationNumber.value = String(calibration);
    els.calibrationValue.textContent = calibration.toFixed(1);
    els.settingsError.hidden = true;
    els.settingsBackdrop.hidden = false;
    els.settingsDrawer.hidden = false;
    window.setTimeout(() => els.serverUrlInput.focus(), 100);
  }

  function closeSettings() {
    els.settingsBackdrop.hidden = true;
    els.settingsDrawer.hidden = true;
    els.settingsError.hidden = true;
  }

  function setCalibration(value) {
    const numeric = clamp(Number(value) || 90, 40, 140);
    const rounded = Math.round(numeric * 10) / 10;
    els.calibrationRange.value = String(rounded);
    els.calibrationNumber.value = String(rounded);
    els.calibrationValue.textContent = rounded.toFixed(1);
  }

  async function saveSettings(event) {
    event.preventDefault();
    if (!state.config) return;
    const nextConfig = {
      serverUrl: els.serverUrlInput.value.trim().replace(/\/+$/, ""),
      deviceId: els.deviceIdInput.value.trim(),
      deviceToken: els.deviceTokenInput.value.trim(),
      calibrationOffsetDb: clamp(Number(els.calibrationNumber.value) || 90, 40, 140),
    };
    els.settingsSave.disabled = true;
    els.settingsError.hidden = true;
    try {
      const saved = isDesktop
        ? await tauri.core.invoke("save_config", { config: nextConfig })
        : nextConfig;
      state.config = saved;
      state.wavePoints = [];
      if (isPreview) {
        state.status = {
          ...(state.status || {}),
          serverUrl: saved.serverUrl,
          deviceId: saved.deviceId,
        };
        renderStatus();
      }
      endCall(true);
      closeSettings();
      connectSignaling();
      window.setTimeout(() => void loadCallRecords(), 1500);
      toast("设置已保存，正在重新连接", "success");
    } catch (error) {
      els.settingsError.hidden = false;
      els.settingsError.textContent =
        typeof error === "string" ? error : error?.message || "保存失败";
    } finally {
      els.settingsSave.disabled = false;
    }
  }

  function openCloseModal() {
    els.closeModal.hidden = false;
    els.exitPin.value = "";
    els.exitError.hidden = true;
    window.setTimeout(() => els.exitPin.focus(), 100);
  }

  function closeCloseModal() {
    els.closeModal.hidden = true;
    els.exitPin.value = "";
    els.exitError.hidden = true;
  }

  async function requestExit() {
    const pin = els.exitPin.value;
    if (!pin) {
      els.exitError.hidden = false;
      els.exitError.textContent = "请输入密码";
      return;
    }
    if (!isDesktop) {
      els.exitError.hidden = false;
      els.exitError.textContent = "浏览器预览模式无法关闭客户端";
      return;
    }
    els.confirmClose.disabled = true;
    els.cancelClose.disabled = true;
    els.exitError.hidden = true;
    try {
      await tauri.core.invoke("request_exit", { pin });
    } catch (error) {
      els.exitError.hidden = false;
      els.exitError.textContent =
        typeof error === "string" ? error : error?.message || "关闭失败";
      els.confirmClose.disabled = false;
      els.cancelClose.disabled = false;
    }
  }

  async function retryAudio() {
    if (!isDesktop) {
      toast("预览模式正在使用模拟数据", "info");
      return;
    }
    try {
      await tauri.core.invoke("retry_audio");
      toast("正在重新初始化麦克风", "info");
    } catch (error) {
      toast(typeof error === "string" ? error : "麦克风初始化失败", "error");
    }
  }

  /* ---------------- Preview ---------------- */

  function mockState() {
    const config = {
      serverUrl: "https://db.gzmath.cn",
      deviceId: "",
      deviceToken: "",
      calibrationOffsetDb: 90,
    };
    const now = Date.now();
    const status = {
      connected: true,
      desiredMonitoring: true,
      monitoring: true,
      serverUrl: config.serverUrl,
      deviceId: config.deviceId,
      deviceName: "未命名设备",
      appVersion: "0.1.0-preview",
      thresholdDb: 65,
      offlineTimeoutSecs: 45,
      autoRecordEvidence: true,
      evidenceSeconds: 10,
      lastHeartbeatAt: new Date().toISOString(),
      lastReportAt: new Date(now - 12000).toISOString(),
      lastError: null,
      reportsSent: 0,
      pendingReports: 0,
      iceServers: [],
    };
    return { config, status, version: "0.1.0-preview" };
  }

  function startPreview() {
    const mock = mockState();
    state.config = mock.config;
    state.status = mock.status;
    state.meter.thresholdDb = mock.status.thresholdDb;
    renderStatus();
    autoAnswerUi();
    setSignalState(false, "预览");
    els.callRecords.innerHTML = '<div class="empty-calls">暂无通话记录</div>';
    let tick = 0;
    state.previewTimer = window.setInterval(() => {
      tick += 1;
      const now = Date.now();
      const base = 49 + Math.sin(now / 4300) * 5.5 + Math.sin(now / 1500) * 1.8;
      const spike = tick % 41 > 36 ? 15 : 0;
      const db = base + spike + (Math.random() - 0.5) * 2.2;
      applyMeter({
        currentDb: Number(db.toFixed(1)),
        averageDb: Number((db - 2.1).toFixed(1)),
        peakDb: Number((db + 5.8).toFixed(1)),
        progress: (now % 30000) / 30000,
        capturing: true,
        ready: true,
        error: null,
        sourceRate: 48000,
        channels: 2,
        thresholdDb: 65,
      });
    }, 420);
  }

  /* ---------------- Events ---------------- */

  function bindEvents() {
    els.setupForm.addEventListener("submit", (event) => void submitSetup(event));
    els.settingsButton.addEventListener("click", openSettingsAuth);
    els.openSettings.addEventListener("click", openSettingsAuth);
    els.settingsAuthCancel.addEventListener("click", closeSettingsAuth);
    els.settingsAuthConfirm.addEventListener("click", () => void verifySettingsPin());
    els.settingsPin.addEventListener("keydown", (event) => {
      if (event.key === "Enter") void verifySettingsPin();
    });
    els.settingsAuthModal.addEventListener("click", (event) => {
      if (event.target === els.settingsAuthModal) closeSettingsAuth();
    });

    els.settingsClose.addEventListener("click", closeSettings);
    els.settingsCancel.addEventListener("click", closeSettings);
    els.settingsBackdrop.addEventListener("click", closeSettings);
    els.settingsForm.addEventListener("submit", (event) => void saveSettings(event));
    els.retryAudio.addEventListener("click", () => void retryAudio());
    els.errorRetry.addEventListener("click", () => void retryAudio());
    els.calibrationRange.addEventListener("input", () =>
      setCalibration(els.calibrationRange.value),
    );
    els.calibrationNumber.addEventListener("input", () =>
      setCalibration(els.calibrationNumber.value),
    );
    els.toggleToken.addEventListener("click", () => {
      const show = els.deviceTokenInput.type === "password";
      els.deviceTokenInput.type = show ? "text" : "password";
      els.toggleToken.textContent = show ? "隐藏" : "显示";
    });

    els.closeButton.addEventListener("click", openCloseModal);
    els.cancelClose.addEventListener("click", closeCloseModal);
    els.confirmClose.addEventListener("click", () => void requestExit());
    els.exitPin.addEventListener("keydown", (event) => {
      if (event.key === "Enter") void requestExit();
    });
    els.closeModal.addEventListener("click", (event) => {
      if (event.target === els.closeModal) closeCloseModal();
    });

    els.alertVolume.addEventListener("input", () => {
      state.alertVolume = Number(els.alertVolume.value);
      localStorage.setItem("soundguard.alertVolume", String(state.alertVolume));
    });
    if (els.alertVolume) els.alertVolume.value = String(state.alertVolume);
    els.autoAnswerToggle.addEventListener("click", toggleAutoAnswer);
    els.acceptCall.addEventListener("click", () => {
      if (state.incomingOffer) void acceptOffer(state.incomingOffer);
    });
    els.declineCall.addEventListener("click", () => {
      if (state.incomingOffer) {
        sendReject(state.incomingOffer.callId, "declined");
        clearIncomingOffer();
      }
    });
    els.endCall.addEventListener("click", () => {
      endCall(true);
      toast("通话已结束", "info");
    });
    els.muteCall.addEventListener("click", toggleMute);
    els.unlockAudio.addEventListener("click", () => {
      const play = els.remoteAudio.play();
      if (play && typeof play.then === "function") {
        play
          .then(() => {
            els.unlockAudio.hidden = true;
            state.call?.guard?.resume();
          })
          .catch(() => toast("无法播放声音，请检查输出设备", "error"));
      }
    });
    els.refreshCalls.addEventListener("click", () => void loadCallRecords());
    els.githubButton.addEventListener("click", () => {
      if (isDesktop) {
        void tauri.core.invoke("open_github");
      } else {
        window.open("https://github.com/Cheerimy-Studio", "_blank", "noopener");
      }
    });

    document.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      if (!els.closeModal.hidden) closeCloseModal();
      else if (!els.settingsAuthModal.hidden) closeSettingsAuth();
      else if (!els.settingsDrawer.hidden) closeSettings();
    });
  }

  async function bindDesktopEvents() {
    if (!isDesktop) return;
    await tauri.event.listen("status", (event) => applyStatus(event.payload));
    await tauri.event.listen("meter", (event) => applyMeter(event.payload));
    await tauri.event.listen("remote-alert", (event) => {
      const alert = event.payload || {};
      toast(
        `${alert.title || "服务端已生成告警"}${
          alert.db ? ` · ${Number(alert.db).toFixed(1)} dB` : ""
        }`,
        "alert",
        6000,
      );
      beep(600, 0.24);
      window.setTimeout(() => beep(760, 0.2), 260);
    });
    await tauri.event.listen("close-requested", () => {
      openCloseModal();
      toast("关闭需要本地密码", "info");
    });
    await tauri.event.listen("config-changed", (event) => {
      state.config = event.payload;
      state.wavePoints = [];
      connectSignaling();
      void loadCallRecords();
    });
    await tauri.event.listen("call-record", () => scheduleCallRecordRefresh());
  }

  function startDesktopRuntime() {
    if (!isDesktop) return;
    void tauri.core.invoke("set_call_active", { active: false }).catch(() => undefined);
    connectSignaling();
    void loadCallRecords();
  }

  function showSetup(config) {
    state.setupRequired = true;
    els.setupServerUrl.value = config?.serverUrl || "https://db.gzmath.cn";
    els.setupDeviceId.value = config?.deviceId || "";
    els.setupDeviceToken.value = config?.deviceToken || "";
    els.setupPassword.value = "";
    els.setupConfirmPassword.value = "";
    els.setupError.hidden = true;
    els.setupScreen.hidden = false;
    window.setTimeout(() => {
      if (els.setupServerUrl.value) els.setupDeviceId.focus();
      else els.setupServerUrl.focus();
    }, 120);
  }

  function hideSetup() {
    state.setupRequired = false;
    els.setupScreen.hidden = true;
    els.setupPassword.value = "";
    els.setupConfirmPassword.value = "";
  }

  async function submitSetup(event) {
    event.preventDefault();
    if (!isDesktop) return;
    const password = els.setupPassword.value;
    const confirmPassword = els.setupConfirmPassword.value;
    if ([...password].length < 8 || [...password].length > 128) {
      els.setupError.hidden = false;
      els.setupError.textContent = "本地密码需要 8–128 个字符";
      return;
    }
    if (password !== confirmPassword) {
      els.setupError.hidden = false;
      els.setupError.textContent = "两次输入的本地密码不一致";
      return;
    }
    els.setupSubmit.disabled = true;
    els.setupError.hidden = true;
    try {
      const saved = await tauri.core.invoke("setup_client", {
        config: {
          serverUrl: els.setupServerUrl.value.trim().replace(/\/+$/, ""),
          deviceId: els.setupDeviceId.value.trim(),
          deviceToken: els.setupDeviceToken.value.trim(),
          calibrationOffsetDb: Number(state.config?.calibrationOffsetDb ?? 90),
        },
        password,
        confirmPassword,
      });
      state.config = saved;
      hideSetup();
      toast("首次配置完成，正在连接服务端", "success");
      startDesktopRuntime();
    } catch (error) {
      els.setupError.hidden = false;
      els.setupError.textContent =
        typeof error === "string" ? error : error?.message || "保存配置失败";
    } finally {
      els.setupSubmit.disabled = false;
    }
  }

  const CONSENT_KEY = "soundguard.consent.v1";

  function consentAccepted() {
    try {
      return localStorage.getItem(CONSENT_KEY) === "accepted";
    } catch {
      return true;
    }
  }

  function askForConsent(onAccepted) {
    const modal = $("consent-modal");
    if (!modal) {
      onAccepted();
      return;
    }
    modal.hidden = false;
    const accept = () => {
      try {
        localStorage.setItem(CONSENT_KEY, "accepted");
      } catch {
        // Storage may be unavailable; continue for this session.
      }
      modal.hidden = true;
      $("consent-accept").removeEventListener("click", accept);
      $("consent-exit").removeEventListener("click", exit);
      onAccepted();
    };
    const exit = () => {
      modal.hidden = true;
      $("consent-accept").removeEventListener("click", accept);
      $("consent-exit").removeEventListener("click", exit);
      if (isDesktop) {
        // Declining the notice must terminate the whole app, not just the page.
        void tauri.core.invoke("exit_now").catch(() => window.close());
      }
    };
    $("consent-accept").addEventListener("click", accept);
    $("consent-exit").addEventListener("click", exit);
  }

  function withConsent(next) {
    if (consentAccepted()) {
      next();
      return;
    }
    askForConsent(next);
  }

  async function boot() {
    bindEvents();
    document.addEventListener("contextmenu", (event) => event.preventDefault());
    autoAnswerUi();
    setupChart();

    if (isDesktop) {
      try {
        const initial = await tauri.core.invoke("get_client_state");
        state.config = initial.config;
        state.status = initial.status;
        if (initial.status?.thresholdDb) {
          state.meter.thresholdDb = initial.status.thresholdDb;
        }
        renderStatus();
        await bindDesktopEvents();
        withConsent(() => {
          if (initial.needsSetup) {
            showSetup(initial.config);
          } else {
            startDesktopRuntime();
          }
        });
      } catch (error) {
        console.error(error);
        toast(
          typeof error === "string" ? error : "无法读取客户端状态，请重启应用",
          "error",
          7000,
        );
      }
    } else {
      withConsent(startPreview);
    }

    window.setInterval(updateTimes, 1000);
    window.setInterval(() => void loadCallRecords(), 30_000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => void boot());
  } else {
    void boot();
  }
})();
