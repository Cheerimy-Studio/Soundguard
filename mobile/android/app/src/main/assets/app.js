(() => {
  "use strict";

  const LS_SERVER = "soundguard.mobile.server";
  const LS_TOKEN = "soundguard.mobile.token";
  const LS_DEVICE = "soundguard.mobile.deviceId";
  const LS_USERNAME = "soundguard.mobile.username";

  const state = {
    server: "",
    token: "",
    devices: [],
    device: null,
    settings: null,
    overview: null,
    overviewSyncedAt: Date.now(),
    selectedDeviceId: null,
    user: null,
    points: [],
    liveLatest: null,
    rangeMinutes: 0,
    history: [],
    rangeThreshold: null,
    overEvents: [],
    historyLoading: false,
    historyTimer: null,
    noticeOpen: false,
    ws: null,
    wsConnected: false,
    wsAttempt: 0,
    reconnectTimer: null,
    recordTab: "calls",
    calls: [],
    alerts: [],
    profileGeneration: 0,
    overTimer: null,
    audioUrl: null,
    audioTitle: "",
  };

  const call = {
    active: false,
    ending: false,
    connected: false,
    callId: null,
    deviceId: null,
    mode: null,
    pc: null,
    local: null,
    remote: null,
    pendingIce: [],
    remoteDescription: false,
    startedIso: null,
    startedAt: null,
    timer: null,
    timeout: null,
    recorder: null,
    chunks: [],
    audioContext: null,
    sources: [],
    mime: "audio/webm",
    muted: false,
  };

  const $ = (id) => document.getElementById(id);

  class HttpError extends Error {
    constructor(status, message) {
      super(message);
      this.name = "HttpError";
      this.status = status;
    }
  }

  function esc(value) {
    return String(value ?? "").replace(/[&<>"']/g, (char) => ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    })[char]);
  }

  function normalizeServer(value) {
    let server = String(value || "").trim();
    if (!server) return "";
    if (!/^https?:\/\//i.test(server)) server = `http://${server}`;
    return server.replace(/\/+$/, "");
  }

  function formatTime(value) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    return date.toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function formatDuration(seconds) {
    const total = Math.max(0, Math.round(Number(seconds) || 0));
    const minutes = Math.floor(total / 60);
    const rest = total % 60;
    return `${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`;
  }

  function number(value, digits = 1) {
    if (value === null || value === undefined || value === "") return "--";
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed.toFixed(digits) : "--";
  }

  function toast(message, timeout = 3200) {
    const element = $("toast");
    if (!element) return;
    element.textContent = message;
    element.classList.remove("hidden");
    window.clearTimeout(toast._timer);
    toast._timer = window.setTimeout(() => element.classList.add("hidden"), timeout);
  }

  function setConnection(connected) {
    state.wsConnected = connected;
    $("connDot").classList.toggle("online", connected);
    if (!connected && state.device && state.device.online) {
      $("subStatus").textContent = "实时通道正在重连…";
    }
  }

  function setLoginError(message) {
    $("loginError").textContent = message || "";
  }

  function showBoot() {
    $("bootView").classList.remove("hidden");
    $("loginView").classList.add("hidden");
    $("appView").classList.add("hidden");
  }

  function showLogin(message = "") {
    $("bootView").classList.add("hidden");
    $("appView").classList.add("hidden");
    $("loginView").classList.remove("hidden");
    setLoginError(message);
    const storedServer = state.server || localStorage.getItem(LS_SERVER) || "";
    $("serverInput").value = storedServer;
    $("serverStep").classList.remove("hidden");
    $("credentialsStep").classList.add("hidden");
    $("serverStatus").textContent = "";
    $("serverStatus").className = "server-status";
    $("serverConfirm").textContent = "";
    $("usernameInput").value = localStorage.getItem(LS_USERNAME) || "admin";
    $("passwordInput").value = "";
    $("checkServerBtn").disabled = false;
    $("checkServerBtn").textContent = "检测连接";
    $("loginBtn").disabled = false;
    $("loginBtn").textContent = "登录";
    window.setTimeout(() => {
      if (storedServer) $("checkServerBtn").focus();
      else $("serverInput").focus();
    }, 120);
  }

  async function checkServer() {
    const server = normalizeServer($("serverInput").value);
    if (!server) {
      setLoginError("请输入服务器地址");
      $("serverInput").focus();
      return;
    }
    const button = $("checkServerBtn");
    button.disabled = true;
    button.textContent = "正在检测…";
    $("serverStatus").textContent = "正在连接服务器…";
    $("serverStatus").className = "server-status";
    setLoginError("");
    try {
      const controller = new AbortController();
      const timer = window.setTimeout(() => controller.abort(), 8000);
      let response;
      try {
        response = await fetch(`${server}/api/health`, {
          cache: "no-store",
          signal: controller.signal,
        });
      } finally {
        window.clearTimeout(timer);
      }
      if (!response.ok) throw new Error(`服务器返回 ${response.status}`);
      const health = await response.json();
      state.server = server;
      localStorage.setItem(LS_SERVER, server);
      $("serverStatus").textContent = `连接正常 · ${health.name || "声垣"} v${health.version || ""}`;
      $("serverStatus").className = "server-status ok";
      $("serverConfirm").textContent = server;
      $("serverStep").classList.add("hidden");
      $("credentialsStep").classList.remove("hidden");
      $("usernameInput").value = localStorage.getItem(LS_USERNAME) || "admin";
      $("passwordInput").value = "";
      window.setTimeout(() => $("passwordInput").focus(), 100);
    } catch (error) {
      state.server = "";
      $("serverStatus").textContent =
        error && error.name === "AbortError"
          ? "连接超时，请检查地址和网络"
          : "无法连接服务器，请检查地址和网络";
      $("serverStatus").className = "server-status error";
    } finally {
      button.disabled = false;
      button.textContent = "检测连接";
    }
  }

  function changeServer() {
    $("credentialsStep").classList.add("hidden");
    $("serverStep").classList.remove("hidden");
    setLoginError("");
    $("serverInput").focus();
  }

  function showApp() {
    $("bootView").classList.add("hidden");
    $("loginView").classList.add("hidden");
    $("appView").classList.remove("hidden");
  }

  async function request(path, options = {}) {
    const headers = new Headers(options.headers || {});
    if (options.body && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }
    if (state.token) headers.set("Authorization", `Bearer ${state.token}`);

    let response;
    try {
      response = await fetch(`${state.server}/api${path}`, {
        cache: "no-store",
        signal: AbortSignal.timeout(20000),
        ...options,
        headers,
      });
    } catch (error) {
      throw new HttpError(0, "无法连接到服务器，请检查网络或服务器地址");
    }

    if (!response.ok) {
      let message = `请求失败（${response.status}）`;
      try {
        const body = await response.json();
        if (body && body.error) message = body.error;
      } catch (error) {
        // Keep the status-based message.
      }
      if (response.status === 401) {
        expireSession("登录已过期，请重新输入用户名和密码");
      }
      throw new HttpError(response.status, message);
    }
    if (response.status === 204) return null;
    return response.json();
  }

  async function requestBlob(path) {
    let response;
    try {
      response = await fetch(`${state.server}/api${path}`, {
        cache: "no-store",
        signal: AbortSignal.timeout(30000),
        headers: state.token ? { Authorization: `Bearer ${state.token}` } : {},
      });
    } catch (error) {
      throw new HttpError(0, "无法下载录音，请检查网络");
    }
    if (!response.ok) {
      if (response.status === 401) {
        expireSession("登录已过期，请重新输入用户名和密码");
      }
      throw new HttpError(response.status, `录音加载失败（${response.status}）`);
    }
    return response.blob();
  }

  function expireSession(message = "请重新登录") {
    state.token = "";
    state.user = null;
    localStorage.removeItem(LS_TOKEN);
    closeSocket();
    try {
      if (window.Android) window.Android.stopMonitor();
    } catch (error) {
      // Ignore native bridge errors.
    }
    if (call.active) endCall(false);
    showLogin(message);
  }

  function saveSessionLocally() {
    if (state.server) localStorage.setItem(LS_SERVER, state.server);
    if (state.token) localStorage.setItem(LS_TOKEN, state.token);
    if (state.user?.username) localStorage.setItem(LS_USERNAME, state.user.username);
    if (state.selectedDeviceId) localStorage.setItem(LS_DEVICE, state.selectedDeviceId);
  }

  function saveNativeSession() {
    if (!window.Android || !state.device) return;
    try {
      window.Android.saveSession(
        state.server,
        state.token,
        state.device.id,
        state.device.name,
        Number(state.settings?.thresholdDb ?? 65),
      );
      window.Android.startMonitor();
    } catch (error) {
      console.warn("native session save failed", error);
    }
  }

  function updateBackgroundCard() {
    const stateBadge = $("bgState");
    const batteryBtn = $("batteryBtn");
    const notifyBtn = $("notifyBtn");
    if (!window.Android) {
      stateBadge.textContent = "前台运行";
      stateBadge.className = "badge idle";
      batteryBtn.textContent = "仅安卓 App 支持后台提醒";
      batteryBtn.disabled = true;
      notifyBtn.classList.add("hidden");
      return;
    }
    try {
      const notifications = window.Android.notificationsEnabled();
      const ignoring = window.Android.isIgnoringBatteryOptimizations();
      if (!notifications) {
        stateBadge.textContent = "通知未授权";
        stateBadge.className = "badge danger";
        notifyBtn.classList.remove("hidden");
      } else if (!ignoring) {
        stateBadge.textContent = "建议允许后台";
        stateBadge.className = "badge warn";
        notifyBtn.classList.add("hidden");
      } else {
        stateBadge.textContent = "已开启";
        stateBadge.className = "badge ok";
        notifyBtn.classList.add("hidden");
      }
      batteryBtn.disabled = false;
      batteryBtn.textContent = ignoring ? "后台运行已允许" : "允许后台运行";
    } catch (error) {
      stateBadge.textContent = "检查失败";
      stateBadge.className = "badge idle";
    }
  }

  async function login() {
    const server = state.server;
    const username = $("usernameInput").value.trim();
    const password = $("passwordInput").value;
    if (!server) {
      showLogin("请先检测服务器连接");
      return;
    }
    if (!username) {
      setLoginError("请输入用户名");
      $("usernameInput").focus();
      return;
    }
    if (!password) {
      setLoginError("请输入密码");
      $("passwordInput").focus();
      return;
    }

    const button = $("loginBtn");
    button.disabled = true;
    button.textContent = "正在登录…";
    setLoginError("");

    try {
      let response;
      try {
        response = await fetch(`${server}/api/auth/login`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ username, password, remember: true }),
        });
      } catch (error) {
        throw new Error("无法连接到服务器，请检查地址和网络");
      }
      if (!response.ok) {
        let message = response.status === 401 ? "用户名或密码错误" : `登录失败（${response.status}）`;
        try {
          const body = await response.json();
          if (body && body.error) message = body.error;
        } catch (error) {
          // Keep default.
        }
        throw new Error(message);
      }
      const data = await response.json();
      state.server = server;
      state.token = data.token;
      state.user = data.user || null;
      localStorage.setItem(LS_SERVER, server);
      localStorage.setItem(LS_TOKEN, data.token);
      localStorage.setItem(LS_USERNAME, username);
      setLoginError("");
      await enterApp();
    } catch (error) {
      setLoginError(error.message || "登录失败");
      button.disabled = false;
      button.textContent = "登录";
    }
  }

  async function enterApp() {
    if (!state.user) {
      state.user = await request("/auth/me");
      if (state.user?.username) localStorage.setItem(LS_USERNAME, state.user.username);
    }
    const devices = await request("/devices");
    if (!devices.devices || devices.devices.length === 0) {
      throw new Error("后台还没有设备，请先在网页端添加设备");
    }
    state.devices = devices.devices;
    const stored = localStorage.getItem(LS_DEVICE);
    const target = state.devices.find((device) => device.id === stored) || state.devices[0];
    state.selectedDeviceId = target.id;
    showApp();
    renderDeviceSelect();
    await loadOverview();
    connectSocket();
    updateBackgroundCard();
  }

  function renderDeviceSelect() {
    const select = $("deviceSelect");
    select.innerHTML = state.devices
      .map((device) => `<option value="${esc(device.id)}">${esc(device.name)}${device.online ? "" : "（离线）"}</option>`)
      .join("");
    if (state.selectedDeviceId) select.value = state.selectedDeviceId;
  }

  async function loadOverview() {
    const deviceId = state.selectedDeviceId;
    if (!deviceId) return;
    const generation = ++state.profileGeneration;
    const overview = await request(`/overview?deviceId=${encodeURIComponent(deviceId)}`);
    if (generation !== state.profileGeneration) return;

    state.overview = overview;
      state.overviewSyncedAt = Date.now();
    state.devices = overview.devices || state.devices;
    const selected = (overview.devices || []).find((device) => device.id === deviceId) || overview.device;
    state.device = selected;
    state.settings = overview.settings;
    state.liveLatest = null;
    state.points = [];
    renderDeviceSelect();
    renderAnnouncement();
    renderDashboard();
    drawChart();
    void refreshWarRanks();
    saveSessionLocally();
    saveNativeSession();
    updateBackgroundCard();
    loadRecords();
  }

  async function refreshOverview() {
    if (!state.token || !state.selectedDeviceId) return;
    try {
      const overview = await request(`/overview?deviceId=${encodeURIComponent(state.selectedDeviceId)}`);
      state.overview = overview;
      state.overviewSyncedAt = Date.now();
      state.devices = overview.devices || state.devices;
      const selected = (overview.devices || []).find((device) => device.id === state.selectedDeviceId);
      if (selected) state.device = selected;
      state.settings = overview.settings;
      renderDeviceSelect();
      renderAnnouncement();
      renderDashboard();
      void refreshWarRanks();
      saveNativeSession();
      updateBackgroundCard();
    } catch (error) {
      if (!(error instanceof HttpError) || error.status !== 401) {
        console.warn("overview refresh failed", error);
      }
    }
  }

  function renderAnnouncement() {
    const settings = state.settings;
    const card = $("announcementCard");
    const superAdmin = ["super_admin","update_admin"].includes(state.user?.role);
    $("editNoticeBtn").classList.toggle("hidden", !superAdmin);
    if (!settings) {
      card.classList.add("hidden");
      return;
    }
    const enabled = Boolean(settings.notificationEnabled);
    const title = (settings.notificationTitle || "").trim();
    const content = (settings.notificationContent || "").trim();
    const hasNotice = enabled && (title || content);
    if (!hasNotice && !superAdmin) {
      card.classList.add("hidden");
      return;
    }
    $("announceTitle").textContent = hasNotice ? title || "通知" : "暂无公告";
    $("announceContent").textContent = hasNotice ? content : "";
    card.classList.remove("hidden");
  }

  function openNoticeEditor() {
    const settings = state.settings || {};
    $("noticeEnabled").checked = Boolean(settings.notificationEnabled);
    $("noticeTitleInput").value = settings.notificationTitle || "";
    $("noticeContentInput").value = settings.notificationContent || "";
    $("noticeError").textContent = "";
    $("noticeModal").classList.remove("hidden");
    window.setTimeout(() => $("noticeTitleInput").focus(), 100);
  }

  function closeNoticeEditor() {
    $("noticeModal").classList.add("hidden");
  }

  async function saveNotice() {
    const button = $("noticeSave");
    button.disabled = true;
    button.textContent = "正在保存…";
    $("noticeError").textContent = "";
    try {
      const result = await request("/notifications", {
        method: "PUT",
        body: JSON.stringify({
          enabled: $("noticeEnabled").checked,
          title: $("noticeTitleInput").value.trim(),
          content: $("noticeContentInput").value.trim(),
        }),
      });
      if (result?.settings) state.settings = result.settings;
      renderAnnouncement();
      closeNoticeEditor();
      toast("公告已保存");
    } catch (error) {
      $("noticeError").textContent = error.message || "公告保存失败";
    } finally {
      button.disabled = false;
      button.textContent = "保存公告";
    }
  }

  function windowActive(start, end, minute) {
    const parse = v => { const match = /^(\d{1,2}):(\d{2})$/.exec(v || ""); return match && +match[1]<24 && +match[2]<60 ? +match[1]*60 + +match[2] : NaN; };
    const a=parse(start), b=parse(end);
    if (!Number.isFinite(a) && !Number.isFinite(b)) return true;
    if (Number.isFinite(a) && !Number.isFinite(b)) return minute>=a;
    if (!Number.isFinite(a) && Number.isFinite(b)) return minute<b;
    return a===b || (a<b ? minute>=a && minute<b : minute>=a || minute<b);
  }
  // The native collector answers with PCMU/8000. Force the same codec here so
  // Chrome does not keep Opus on the sending side of an asymmetric negotiation.
  function preferPcmu(pc) {
    try {
      const transceiver = pc.getTransceivers().find((item) => item.sender?.track?.kind === "audio");
      const codecs = (window.RTCRtpSender && RTCRtpSender.getCapabilities)
        ? RTCRtpSender.getCapabilities("audio")?.codecs || []
        : [];
      const pcmu = codecs.filter((codec) => /PCMU\/8000/i.test(codec.mimeType || ""));
      if (transceiver && pcmu.length) transceiver.setCodecPreferences(pcmu);
    } catch (error) {
      console.warn("codec preference failed", error);
    }
  }
  function currentPolicy() {
    const settings=state.settings || {};
    const now = (Date.parse(state.overview?.serverTime) || Date.now()) + (Date.now()-state.overviewSyncedAt)
      + Number(state.overview?.serverUtcOffsetSecs || 0)*1000;
    const date=new Date(now), minute=date.getUTCHours()*60+date.getUTCMinutes();
    return { capture: windowActive(settings.captureStart,settings.captureEnd,minute),
      war: !!settings.warModeEnabled && windowActive(settings.warModeStart,settings.warModeEnd,minute) };
  }
  function renderDashboard() {
    const device = state.device;
    if (!device) return;
    const policy = currentPolicy();
    const planStart = state.settings?.captureStart || "";
    const planEnd = state.settings?.captureEnd || "";
    const planText = planStart || planEnd
      ? `计划 ${planStart ? `每日 ${planStart} 自动开启` : "每日 00:00 起采集"}${planEnd ? ` · ${planEnd} 自动关闭` : " · 不自动关闭"} · 服务器时区`
      : "";
    $("policySummary").textContent = planText;
    $("policySummary").classList.toggle("hidden", !planText);
    document.body.classList.toggle("war-mode", policy.war);
    const live = state.liveLatest;
    const suspended = Boolean(live ? live.suspended : device.liveSuspended);
    const ignoringSystem = Boolean(
      live
        ? live.systemPlaying && !live.aecEnabled
        : device.liveSystemPlaying && !device.liveAecEnabled,
    );
    const muted = suspended || ignoringSystem;
    const threshold = Number(state.settings?.thresholdDb ?? 65);
    const rolling15 = muted ? null : (live ? (live.rolling15sDb ?? null) : (device.liveRolling15sDb ?? null));
    const current = muted ? null : (live ? live.currentDb : device.currentDb);
    state.rolling15sDb = rolling15;
    const average = muted ? null : (live ? live.averageDb : device.liveAvgDb);
    const peak = muted ? null : (live ? live.peakDb : device.peakDb);

    $("currentDb").textContent = number(current);
    const dbEl = $("currentDb");
    if (!policy.war && rolling15 !== null && Number(rolling15) >= threshold) {
      dbEl.style.color = "#d97706";
    } else {
      dbEl.style.color = "";
    }
    $("avgDb").textContent = number(average);
    $("peakDb").textContent = number(peak);
    $("thresholdDb").textContent = number(threshold);
    $("alertCount").textContent = String(state.overview?.alertCountToday ?? 0);

    let badgeText = "离线";
    let badgeClass = "idle";
    if (!device.online) {
      badgeText = "离线";
    } else if (suspended) {
      badgeText = "通话中";
      badgeClass = "warn";
    } else if (!device.desiredMonitoring) {
      badgeText = "已暂停";
    } else if (!policy.capture) {
      badgeText = "计划待机";
    } else if (device.monitoring) {
      badgeText = policy.war ? "分贝战争" : "监测中";
      badgeClass = "ok";
    } else {
      badgeText = "等待采集";
      badgeClass = "warn";
    }
    const badge = $("statusBadge");
    badge.textContent = badgeText;
    badge.className = `badge ${badgeClass}`;

    const level = current === null || current === undefined || current === "" ? Number.NaN : Number(current);
    const percent = Number.isFinite(level) ? Math.max(0, Math.min(100, ((level - 30) / 70) * 100)) : 0;
    const fill = $("levelFill");
    fill.style.width = `${percent}%`;
    fill.classList.toggle("over", Number.isFinite(level) && level >= threshold);
    $("thresholdMark").style.left = `${Math.max(0, Math.min(100, ((threshold - 30) / 70) * 100))}%`;

    $("captureBtn").textContent = device.desiredMonitoring ? "暂停采集" : "开始采集";

    let status = "";
    if (!device.online) {
      status = "设备离线，等待重新连接";
    } else if (suspended) {
      status = "通话中，分贝监测暂停";
    } else if (device.captureError) {
      status = `采集异常：${device.captureError}`;
    } else if (live && live.systemPlaying) {
      status = "检测到电脑播放声，已自动忽略";
    } else if (!state.wsConnected) {
      status = "实时通道正在重连…";
    }
    if (!status && (state.settings?.captureStart || state.settings?.captureEnd)) {
      status = state.settings.captureStart
        ? `每日 ${state.settings.captureStart} 自动开启${state.settings.captureEnd ? ` · ${state.settings.captureEnd} 自动关闭` : " · 不自动关闭"} · 服务器时区`
        : `每日 00:00 起采集 · ${state.settings.captureEnd} 自动关闭 · 服务器时区`;
    }
    $("subStatus").textContent = status;

    $("callBtn").disabled = call.active;
    $("broadcastBtn").disabled = call.active;
    $("callBtn").textContent = call.active && call.mode === "intercom" ? "通话进行中" : "双向对讲";
    $("broadcastBtn").textContent = call.active && call.mode === "broadcast" ? "广播进行中" : "远程广播";
  }

  async function refreshWarRanks() {
    const card = $("warRankCard");
    if (!card) return;
    if (!currentPolicy().war) {
      card.classList.add("hidden");
      return;
    }
    try {
      const result = await request("/war-mode/leaderboard");
      const rows = Array.isArray(result?.leaderboard) ? result.leaderboard : [];
      $("warRanks").innerHTML = rows.length
        ? rows
            .map(
              (row, index) =>
                `<li><span class="rank-name"><span class="rank-no">${index + 1}</span><span>${esc(row.name)}</span></span><strong>${number(row.avgDb)} dB</strong></li>`,
            )
            .join("")
        : `<li class="muted">等待有效采集数据</li>`;
      card.classList.remove("hidden");
    } catch (error) {
      if (!(error instanceof HttpError) || error.status !== 401) {
        card.classList.add("hidden");
      }
    }
  }

  function addLivePoint(live) {
    if (!live || live.deviceId !== state.selectedDeviceId) return;
    if (
      !live.suspended
      && !(live.systemPlaying && !live.aecEnabled)
      && Number.isFinite(Number(live.currentDb))
    ) {
      const timestamp = Date.parse(live.recordedAt) || Date.now();
      state.points.push({
        t: timestamp,
        current: Number(live.currentDb),
        average: Number.isFinite(Number(live.averageDb)) ? Number(live.averageDb) : null,
      });
      const cutoff = timestamp - 62000;
      state.points = state.points.filter((point) => point.t >= cutoff).slice(-180);
      if (state.rangeMinutes === 0) {
        renderOverList();
        drawChart();
      }
    }
  }

  function drawChart() {
    const canvas = $("chart");
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    if (rect.width <= 0) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const width = Math.round(rect.width);
    const height = 240;
    if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
    }
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const padLeft = 38;
    const padRight = 10;
    const padTop = 14;
    const padBottom = 26;
    const plotWidth = Math.max(10, width - padLeft - padRight);
    const plotHeight = height - padTop - padBottom;
    const now = Date.now();
    const historyMode = state.rangeMinutes > 0;
    const windowMs = historyMode ? state.rangeMinutes * 60_000 : 60_000;
    const windowStart = now - windowMs;
    const threshold = Number(
      historyMode ? state.rangeThreshold ?? state.settings?.thresholdDb ?? 65 : state.settings?.thresholdDb ?? 65,
    );
    const liveLimit = 60;
    const points = historyMode
      ? state.history
          .map((reading, index) => ({
            t: Date.parse(reading.recordedAt),
            index,
            current: Number(reading.avgDb),
            average: Number(reading.peakDb),
            over: Boolean(reading.overThreshold) || Number(reading.avgDb) >= threshold,
          }))
          .filter((point) => Number.isFinite(point.t) && Number.isFinite(point.current))
      : state.points
          .filter((point) => point.t >= windowStart - 1000)
          .slice(-liveLimit)
          .map((point, index) => ({
            ...point,
            index,
            over: point.average !== null && point.average >= threshold,
          }));

    const peakValues = points.map((point) => (historyMode ? point.average : point.current)).filter(Number.isFinite);
    const minDb = 30;
    const maxDb = Math.max(
      100,
      Math.ceil((Math.max(threshold + 15, ...peakValues, 85) + 5) / 10) * 10,
    );
    const x = (point) =>
      historyMode
        ? padLeft + ((point.t - windowStart) / windowMs) * plotWidth
        : padLeft + ((point.index ?? 0) / (liveLimit - 1)) * plotWidth;
    const y = (db) => padTop + ((maxDb - db) / (maxDb - minDb)) * plotHeight;

    ctx.font = "12px -apple-system, 'Segoe UI', 'Noto Sans SC', 'SoundGuard CJK', sans-serif";
    ctx.textBaseline = "middle";
    ctx.strokeStyle = "#e8edee";
    ctx.lineWidth = 1;

    const gridStep = maxDb > 120 ? 20 : 20;
    for (let value = 40; value <= maxDb; value += gridStep) {
      const lineY = Math.round(y(value)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(padLeft, lineY);
      ctx.lineTo(width - padRight, lineY);
      ctx.stroke();
      ctx.fillStyle = "#8b9b9d";
      ctx.textAlign = "right";
      ctx.fillText(String(value), padLeft - 7, lineY);
    }

    ctx.textAlign = "center";
    ctx.fillStyle = "#8b9b9d";
    if (historyMode) {
      const minutes = state.rangeMinutes;
      ctx.fillText(`${minutes} 分钟前`, padLeft + 26, height - 10);
      ctx.fillText(`${Math.round(minutes / 2)} 分钟前`, padLeft + plotWidth / 2, height - 10);
    } else {
      ctx.fillText("开始", padLeft + 12, height - 10);
      ctx.fillText("30 秒前", padLeft + plotWidth / 2, height - 10);
    }
    ctx.fillText("现在", width - padRight - 15, height - 10);

    if (threshold >= minDb && threshold <= maxDb) {
      ctx.save();
      ctx.strokeStyle = "#bd4438";
      ctx.globalAlpha = 0.62;
      ctx.setLineDash([5, 5]);
      ctx.beginPath();
      ctx.moveTo(padLeft, y(threshold));
      ctx.lineTo(width - padRight, y(threshold));
      ctx.stroke();
      ctx.restore();
    }

    if (points.length === 0) {
      ctx.fillStyle = "#8b9b9d";
      ctx.font = "14px -apple-system, 'Segoe UI', 'Noto Sans SC', 'SoundGuard CJK', sans-serif";
      ctx.textAlign = "center";
      ctx.fillText(
        historyMode ? (state.historyLoading ? "正在读取历史数据…" : "该区间暂无数据") : "等待实时数据",
        padLeft + plotWidth / 2,
        padTop + plotHeight / 2,
      );
      return;
    }

    ctx.save();
    ctx.beginPath();
    ctx.rect(padLeft, padTop, plotWidth, plotHeight);
    ctx.clip();

    const drawLine = (values, color, width, dashed, fill) => {
      let segment = [];
      const flush = () => {
        if (segment.length === 0) return;
        ctx.beginPath();
        segment.forEach((point, index) => {
          const px = x(point);
          const py = y(point.value);
          if (index === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        });
        ctx.strokeStyle = color;
        ctx.lineWidth = width;
        ctx.lineJoin = "round";
        ctx.lineCap = "round";
        if (dashed) ctx.setLineDash([4, 4]);
        else ctx.setLineDash([]);
        ctx.stroke();
        ctx.setLineDash([]);
        if (fill && segment.length > 1) {
          ctx.lineTo(x(segment[segment.length - 1]), padTop + plotHeight);
          ctx.lineTo(x(segment[0]), padTop + plotHeight);
          ctx.closePath();
          ctx.fillStyle = fill;
          ctx.fill();
        }
        segment = [];
      };
      for (const point of values) {
        if (segment.length > 0 && point.t - segment[segment.length - 1].t > (historyMode ? 90_000 : 5000)) {
          flush();
        }
        segment.push(point);
      }
      flush();
    };

    drawLine(
      points.map((point) => ({ ...point, t: point.t, value: point.current })),
      "#0e7c86",
      2,
      false,
      null,
    );

    if (historyMode) {
      drawLine(
        points.map((point) => ({ ...point, t: point.t, value: point.average })).filter((point) => Number.isFinite(point.value)),
        "#a07a38",
        1.5,
        true,
        null,
      );
    } else {
      drawLine(
        points
          .filter((point) => point.average !== null)
          .map((point) => ({ ...point, t: point.t, value: point.average })),
        "#86989b",
        1.5,
        true,
        null,
      );
    }

    for (const point of points) {
      if (!point.over) continue;
      ctx.beginPath();
      ctx.arc(x(point), y(point.current), 3.2, 0, Math.PI * 2);
      ctx.fillStyle = "#bd4438";
      ctx.fill();
    }

    const last = points[points.length - 1];
    if (last) {
      ctx.beginPath();
      ctx.arc(x(last.t), y(last.current), 3.5, 0, Math.PI * 2);
      ctx.fillStyle = "#0e7c86";
      ctx.fill();
    }
    ctx.restore();
  }

  function rangeLabel(minutes) {
    return minutes === 0 ? "实时" : `最近 ${minutes} 分钟`;
  }

  async function setRange(minutes) {
    state.rangeMinutes = minutes;
    for (const button of document.querySelectorAll("#rangeTabs button")) {
      button.classList.toggle("active", Number(button.dataset.minutes) === minutes);
    }
    $("chartCaption").textContent = rangeLabel(minutes);
    $("legendPrimary").textContent = minutes === 0 ? "当前" : "平均";
    $("legendSecondary").textContent = minutes === 0 ? "平均" : "峰值";
    if (minutes === 0) {
      state.history = [];
      state.rangeThreshold = null;
      renderOverList();
      drawChart();
      return;
    }
    await loadHistory(minutes);
  }

  async function loadHistory(minutes) {
    if (!state.selectedDeviceId) return;
    state.historyLoading = true;
    $("chartCaption").textContent = "读取中…";
    drawChart();
    try {
      const to = new Date();
      const from = new Date(to.getTime() - minutes * 60_000);
      const params = new URLSearchParams({
        from: from.toISOString(),
        to: to.toISOString(),
        deviceId: state.selectedDeviceId,
        maxPoints: "2000",
      });
      const data = await request(`/readings?${params.toString()}`);
      if (state.rangeMinutes !== minutes) return;
      state.history = Array.isArray(data.readings) ? data.readings : [];
      state.rangeThreshold = Number(data.thresholdDb ?? state.settings?.thresholdDb ?? 65);
      state.overEvents = state.history
        .filter((reading) => reading.overThreshold || Number(reading.avgDb) >= state.rangeThreshold)
        .map((reading) => ({
          t: reading.recordedAt,
          db: Number(reading.avgDb),
        }));
      $("chartCaption").textContent = rangeLabel(minutes);
      renderOverList();
      drawChart();
    } catch (error) {
      $("chartCaption").textContent = rangeLabel(minutes);
      toast(error.message || "历史数据读取失败");
    } finally {
      state.historyLoading = false;
    }
  }

  function renderOverList() {
    const container = $("overList");
    const items = $("overItems");
    let events = state.overEvents;
    if (state.rangeMinutes === 0) {
      const threshold = Number(state.settings?.thresholdDb ?? 65);
      events = state.points
        .filter((point) => point.average !== null && point.average >= threshold)
        .slice(-12)
        .map((point) => ({ t: new Date(point.t).toISOString(), db: point.average }));
    }
    if (!events || events.length === 0) {
      container.classList.add("hidden");
      items.innerHTML = "";
      return;
    }
    container.classList.remove("hidden");
    items.innerHTML = events
      .slice(-24)
      .reverse()
      .map(
        (event) => `
          <div class="over-list-item">
            <span>${esc(formatTime(event.t))}</span>
            <strong>${number(event.db)} dB</strong>
          </div>`,
      )
      .join("");
  }

  function connectSocket() {
    if (!state.token || !state.server) return;
    if (state.ws && (state.ws.readyState === WebSocket.OPEN || state.ws.readyState === WebSocket.CONNECTING)) {
      return;
    }
    window.clearTimeout(state.reconnectTimer);
    state.reconnectTimer = null;
    const url = `${state.server.replace(/^http/, "ws")}/api/ws?token=${encodeURIComponent(state.token)}`;
    let socket;
    try {
      socket = new WebSocket(url);
    } catch (error) {
      scheduleReconnect();
      return;
    }
    state.ws = socket;

    socket.onopen = () => {
      state.wsAttempt = 0;
      setConnection(true);
      renderDashboard();
    };
    socket.onmessage = (event) => {
      let message;
      try {
        message = JSON.parse(event.data);
      } catch (error) {
        return;
      }
      handleSocketMessage(message);
    };
    socket.onerror = () => {
      try {
        socket.close();
      } catch (error) {
        // Ignore.
      }
    };
    socket.onclose = () => {
      if (state.ws === socket) {
        if (call.active) void endCall(false);
        state.ws = null;
        setConnection(false);
        renderDashboard();
        scheduleReconnect();
      }
    };
  }

  function scheduleReconnect() {
    if (!state.token) return;
    window.clearTimeout(state.reconnectTimer);
    const delay = Math.min(15000, 2000 + state.wsAttempt * 1500);
    state.wsAttempt += 1;
    state.reconnectTimer = window.setTimeout(() => connectSocket(), delay);
  }

  function closeSocket() {
    window.clearTimeout(state.reconnectTimer);
    state.reconnectTimer = null;
    if (state.ws) {
      const socket = state.ws;
      state.ws = null;
      try {
        socket.close();
      } catch (error) {
        // Ignore.
      }
    }
    setConnection(false);
  }

  function sendSocket(payload) {
    if (!state.ws || state.ws.readyState !== WebSocket.OPEN) return false;
    state.ws.send(JSON.stringify(payload));
    return true;
  }

  function effectiveSettings(base, device) {
    const result = { ...base };
    for (const key of ["thresholdDb", "alertCooldownSecs", "offlineTimeoutSecs", "autoRecordEvidence"]) {
      if (device?.settings?.[key] != null) result[key] = device.settings[key];
    }
    return result;
  }

  function handleSocketMessage(message) {
    if (!message || !message.type) return;
    switch (message.type) {
      case "snapshot": {
        const data = message.data || {};
        state.devices = data.devices || state.devices;
        if (data.settings) {
          state.globalSettings = data.settings;
          state.settings = effectiveSettings(data.settings, state.device);
          renderAnnouncement();
        }
        const selected = state.devices.find((device) => device.id === state.selectedDeviceId);
        if (selected) state.device = selected;
        renderDeviceSelect();
        renderDashboard();
        break;
      }
      case "live": {
        const live = message.data;
        if (!live || live.deviceId !== state.selectedDeviceId) break;
        state.liveLatest = live;
        state.device = {
          ...state.device,
          ...(live.device || {}),
          currentDb: live.currentDb,
          liveAvgDb: live.averageDb,
          peakDb: live.peakDb ?? state.device?.peakDb ?? null,
          liveSuspended: live.suspended,
          liveSystemPlaying: live.systemPlaying,
          liveAecEnabled: live.aecEnabled,
        };
        addLivePoint(live);
        renderDashboard();
        break;
      }
      case "device": {
        const updated = message.data;
        if (!updated) break;
        state.devices = state.devices.map((device) => (device.id === updated.id ? updated : device));
        if (updated.id === state.selectedDeviceId) {
          state.device = updated;
          state.settings = effectiveSettings(state.globalSettings || state.settings, updated);
          renderDashboard();
        }
        break;
      }
      case "devices": {
        if (Array.isArray(message.data)) {
          state.devices = message.data;
          const selected = state.devices.find((device) => device.id === state.selectedDeviceId);
          if (selected) state.device = selected;
          renderDeviceSelect();
          renderDashboard();
        }
        break;
      }
      case "settings": {
        if (message.data) {
          state.globalSettings = message.data;
          state.settings = effectiveSettings(message.data, state.device);
          renderAnnouncement();
          renderDashboard();
          drawChart();
        }
        break;
      }
      case "alert": {
        const alert = message.data;
        if (!alert || alert.deviceId !== state.selectedDeviceId) break;
        if (["noise", "offline", "tamper"].includes(alert.kind) && state.overview) {
          state.seenAlerts ||= new Set();
          if (!state.seenAlerts.has(alert.id)) {
            state.seenAlerts.add(alert.id);
            if (new Date(alert.createdAt).toDateString() === new Date().toDateString()) {
              state.overview.alertCountToday = (state.overview.alertCountToday || 0) + 1;
            }
          }
        }
        if (alert.kind === "noise") {
          showOverBanner(alert.message || `平均 ${number(alert.db)} dB，超过设定阈值`);
          loadRecords();
        }
        renderDashboard();
        break;
      }
      case "call-record": {
        if (message.data && message.data.deviceId === state.selectedDeviceId) {
          loadRecords();
        }
        break;
      }
      case "signal":
      case "hangup":
      case "signal-error": {
        handleCallMessage(message);
        break;
      }
      default:
        break;
    }
  }

  function showOverBanner(text) {
    if (currentPolicy().war) return;
    $("overBannerText").textContent = text || "请及时关注现场情况";
    $("overBanner").classList.remove("hidden");
    window.clearTimeout(state.overTimer);
    state.overTimer = window.setTimeout(() => $("overBanner").classList.add("hidden"), 12000);
  }

  async function toggleCapture() {
    if (!state.device) return;
    const button = $("captureBtn");
    const next = !state.device.desiredMonitoring;
    button.disabled = true;
    try {
      const result = await request("/device/control", {
        method: "POST",
        body: JSON.stringify({ deviceId: state.device.id, monitoring: next }),
      });
      state.device = { ...state.device, ...(result.device || {}) };
      if (result.settings) state.settings = result.settings;
      renderDashboard();
      toast(next ? (currentPolicy().capture ? "已开启采集" : "已启用，等待计划时段") : "已暂停采集");
    } catch (error) {
      toast(error.message || "操作失败");
    } finally {
      button.disabled = false;
    }
  }

  async function switchDevice(deviceId) {
    if (!deviceId || deviceId === state.selectedDeviceId) return;
    if (call.active) {
      await endCall(true, "设备已切换，通话结束");
    }
    closePlayer();
    state.selectedDeviceId = deviceId;
    state.device = state.devices.find((device) => device.id === deviceId) || null;
    state.liveLatest = null;
    state.points = [];
    localStorage.setItem(LS_DEVICE, deviceId);
    renderDeviceSelect();
    try {
      await loadOverview();
      await setRange(0);
      toast(`已切换到 ${state.device?.name || "设备"}`);
    } catch (error) {
      toast(error.message || "设备加载失败");
    }
  }

  async function loadRecords() {
    if (!state.selectedDeviceId) return;
    const deviceId = state.selectedDeviceId;
    const generation = state.profileGeneration;
    const [callsResult, alertsResult] = await Promise.allSettled([
      request(`/calls?deviceId=${encodeURIComponent(deviceId)}&limit=10`),
      request(`/alerts?deviceId=${encodeURIComponent(deviceId)}&limit=30`),
    ]);
    if (generation !== state.profileGeneration) return;
    if (callsResult.status === "fulfilled" && Array.isArray(callsResult.value)) {
      state.calls = callsResult.value;
    }
    if (alertsResult.status === "fulfilled" && Array.isArray(alertsResult.value)) {
      state.alerts = alertsResult.value.filter((alert) => alert.evidenceFile);
    }
    renderRecords();
  }

  function renderRecords() {
    const list = $("recordList");
    const tab = state.recordTab;
    if (tab === "calls") {
      if (state.calls.length === 0) {
        list.innerHTML = '<p class="empty">暂无通话录音</p>';
        return;
      }
      list.innerHTML = state.calls.map((record) => {
        const mode = record.mode === "broadcast" ? "广播录音" : "双向对讲";
        const meta = `${formatTime(record.startedAt)} · ${formatDuration(record.durationSecs)}`;
        return `
          <div class="record">
            <button class="play-btn" data-kind="call" data-id="${esc(record.id)}"
              data-title="${esc(`${mode} ${formatTime(record.startedAt)}`)}"
              ${record.hasAudio ? "" : "disabled"} aria-label="播放${esc(mode)}">▶</button>
            <div class="record-main">
              <strong>${esc(mode)}</strong>
              <span>${esc(meta)}</span>
            </div>
          </div>`;
      }).join("");
    } else {
      if (state.alerts.length === 0) {
        list.innerHTML = '<p class="empty">暂无告警录音</p>';
        return;
      }
      list.innerHTML = state.alerts.map((alert) => {
        const meta = `${formatTime(alert.createdAt)} · ${number(alert.db)} dB · ${alert.evidenceDurationSecs || 30} 秒`;
        return `
          <div class="record">
            <button class="play-btn" data-kind="alert" data-id="${esc(alert.id)}"
              data-title="${esc(alert.title || "告警录音")}"
              aria-label="播放告警录音">▶</button>
            <div class="record-main">
              <strong>${esc(alert.title || "环境声音告警")}</strong>
              <span>${esc(meta)}</span>
            </div>
          </div>`;
      }).join("");
    }
  }

  function setRecordTab(tab) {
    state.recordTab = tab;
    $("tabCalls").classList.toggle("active", tab === "calls");
    $("tabAlerts").classList.toggle("active", tab === "alerts");
    renderRecords();
  }

  async function playRecording(kind, id, title) {
    closePlayer();
    const bar = $("audioBar");
    const player = $("player");
    $("audioTitle").textContent = title || "正在加载录音…";
    bar.classList.remove("hidden");
    try {
      const path = kind === "call"
        ? `/calls/${encodeURIComponent(id)}/audio`
        : `/alerts/${encodeURIComponent(id)}/evidence`;
      const blob = await requestBlob(path);
      state.audioUrl = URL.createObjectURL(blob);
      state.audioTitle = title || "录音播放";
      $("audioTitle").textContent = state.audioTitle;
      player.src = state.audioUrl;
      try {
        await player.play();
      } catch (error) {
        // The user can press play in the native control.
      }
    } catch (error) {
      toast(error.message || "录音加载失败");
      bar.classList.add("hidden");
    }
  }

  function closePlayer() {
    const player = $("player");
    if (player) {
      player.pause();
      player.removeAttribute("src");
      player.load();
    }
    if (state.audioUrl) {
      URL.revokeObjectURL(state.audioUrl);
      state.audioUrl = null;
    }
    $("audioBar").classList.add("hidden");
  }

  function newCallId() {
    if (window.crypto && typeof window.crypto.randomUUID === "function") {
      return window.crypto.randomUUID();
    }
    return `mobile-${Date.now()}-${Math.random().toString(16).slice(2)}`;
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

  let pendingCallMode = null;

  function setAndroidCommunicationMode(active) {
    if (!window.Android || typeof window.Android.setCommunicationMode !== "function") return;
    try {
      window.Android.setCommunicationMode(active);
    } catch (error) {
      console.warn("audio mode", error);
    }
  }

  window.__resumePendingCall = () => {
    const mode = pendingCallMode;
    pendingCallMode = null;
    if (mode) void startCall(mode);
  };

  window.__pendingCallPermissionDenied = () => {
    pendingCallMode = null;
    toast("未获得麦克风权限，请在系统设置中允许");
  };

  async function startCall(mode = "intercom") {
    if (call.active) return;
    if (!state.device) return;
    if (!state.device.online) {
      toast("设备当前离线，暂时无法通话");
      return;
    }
    if (!state.ws || state.ws.readyState !== WebSocket.OPEN) {
      toast("实时通道未连接，请稍后重试");
      return;
    }
    if (window.Android && typeof window.Android.hasMicrophonePermission === "function"
        && !window.Android.hasMicrophonePermission()) {
      pendingCallMode = mode;
      try {
        window.Android.requestMicrophonePermission();
      } catch (error) {
        pendingCallMode = null;
      }
      toast("请允许麦克风权限，授权后将自动开始");
      return;
    }
    pendingCallMode = null;
    closePlayer();
    resetCallFields();
    call.active = true;
    call.deviceId = state.device.id;
    call.mode = mode;
    call.callId = newCallId();
    $("callModeLabel").textContent = mode === "broadcast" ? "远程广播" : "双向对讲";
    $("callOverlay").setAttribute("aria-label", mode === "broadcast" ? "远程广播" : "双向对讲");
    $("remoteAudio").muted = mode === "broadcast";
    $("callDevice").textContent = state.device.name;
    $("callState").textContent = mode === "broadcast" ? "正在建立广播通道…" : "正在连接现场设备…";
    $("callTimer").textContent = "00:00";
    $("muteBtn").textContent = "静音";
    $("muteBtn").disabled = true;
    $("hangupBtn").disabled = false;
    $("callOverlay").classList.remove("hidden");
    renderDashboard();

    call.requestTimeout = window.setTimeout(() => {
      if (call.active && !call.local) {
        toast("未获得麦克风权限，请在系统设置中允许");
        void endCall(false);
      }
    }, 15000);

    try {
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        throw new Error("当前环境不支持麦克风，请使用安卓 App");
      }
      setAndroidCommunicationMode(true);
      let stream;
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
      } catch (error) {
        if (error && error.name === "NotAllowedError") throw error;
        stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      }
      if (!call.active) {
        stream.getTracks().forEach((track) => track.stop());
        return;
      }
      window.clearTimeout(call.requestTimeout);
      call.requestTimeout = null;
      call.micStream = stream;
      call.guard = createCallAudioGuard(stream);
      call.local = stream;
      const iceServers = state.overview?.iceServers || [];
      const pc = new RTCPeerConnection({ iceServers, iceCandidatePoolSize: 4 });
      call.pc = pc;
      call.local.getTracks().forEach((track) => pc.addTrack(track, call.local));
      preferPcmu(pc);
      pc.onicecandidate = (event) => {
        if (!event.candidate || !call.active) return;
        sendSocket({
          type: "signal",
          callId: call.callId,
          deviceId: call.deviceId,
          mode: call.mode,
          to: "device",
          payload: { kind: "ice", candidate: event.candidate.toJSON() },
        });
      };
      pc.ontrack = (event) => {
        const remote = event.streams[0] || new MediaStream([event.track]);
        call.remote = remote;
        const audio = $("remoteAudio");
        audio.srcObject = remote;
        audio.muted = call.mode === "broadcast";
        $("unlockCallAudio").classList.add("hidden");
        audio.play().then(() => {
          $("unlockCallAudio").classList.add("hidden");
          call.guard?.resume();
        }).catch(() => {
          $("unlockCallAudio").classList.remove("hidden");
        });
        if (call.connected && !call.recorder) void startRecording();
      };
      pc.onconnectionstatechange = () => {
        if (!call.active) return;
        if (pc.connectionState === "connected") {
          if (!call.connected) {
            call.connected = true;
            call.startedIso = new Date().toISOString();
            call.startedAt = Date.now();
            $("callState").textContent = call.mode === "broadcast" ? "广播中" : "通话中";
            $("muteBtn").disabled = false;
            window.clearTimeout(call.timeout);
            startCallTimer();
            const audio = $("remoteAudio");
            if (call.mode !== "broadcast" && audio.paused) {
              audio.play().catch(() => $("unlockCallAudio").classList.remove("hidden"));
            }
            void startRecording();
          }
        } else if (pc.connectionState === "failed") {
          toast("语音连接中断");
          void endCall(true, "连接中断");
        }
      };

      $("callState").textContent = "等待现场接听…";
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      const sent = sendSocket({
        type: "signal",
        callId: call.callId,
        deviceId: call.deviceId,
        mode: call.mode,
        to: "device",
        payload: { kind: "offer", sdp: pc.localDescription?.sdp },
      });
      if (!sent) throw new Error("信令通道未连接");
      call.timeout = window.setTimeout(() => {
        if (call.active && !call.connected) {
          toast("设备未响应，请确认采集端在线并允许接听");
          void endCall(true, "未接通");
        }
      }, 25000);
    } catch (error) {
      let message = "对讲启动失败";
      if (error && error.name === "NotAllowedError") {
        message = "未获得麦克风权限，请在系统设置中允许";
      } else if (error && error.name === "NotFoundError") {
        message = "未找到可用麦克风";
      } else if (error && error.message) {
        message = error.message;
      }
      toast(message);
      void endCall(false);
    }
  }

  function handleCallMessage(message) {
    if (!call.active) return;
    if (message.deviceId && message.deviceId !== call.deviceId) return;
    if (message.type === "signal-error") {
      if (!message.callId || message.callId === call.callId) {
        toast(message.message || "对讲通道建立失败");
        void endCall(false);
      }
      return;
    }
    if (message.callId && message.callId !== call.callId) return;
    if (message.type === "hangup") {
      if (message.from === "device" || !message.from) {
        toast("现场已结束通话");
        void endCall(false);
      }
      return;
    }
    const payload = message.payload || {};
    if (!call.pc) return;
    if (payload.kind === "reject") {
      toast(payload.reason === "calls-disabled" ? "现场已关闭远程接听" : "现场拒绝了本次通话");
      void endCall(false);
      return;
    }
    if (payload.kind === "answer" && payload.sdp) {
      call.pc.setRemoteDescription({ type: "answer", sdp: payload.sdp })
        .then(async () => {
          call.remoteDescription = true;
          for (const candidate of call.pendingIce) {
            try {
              await call.pc.addIceCandidate(candidate);
            } catch (error) {
              // Ignore stale candidates.
            }
          }
          call.pendingIce = [];
          if (call.active && !call.connected) $("callState").textContent = "正在建立语音…";
        })
        .catch(() => {
          toast("语音连接建立失败");
          void endCall(false);
        });
      return;
    }
    if (payload.kind === "ice" && payload.candidate) {
      if (call.remoteDescription) {
        call.pc.addIceCandidate(payload.candidate).catch(() => {});
      } else {
        call.pendingIce.push(payload.candidate);
      }
    }
  }

  function startCallTimer() {
    window.clearInterval(call.timer);
    call.timer = window.setInterval(() => {
      if (!call.startedAt) return;
      const elapsed = Math.max(0, Math.floor((Date.now() - call.startedAt) / 1000));
      $("callTimer").textContent = formatDuration(elapsed);
    }, 1000);
  }

  async function startRecording() {
    if (!window.MediaRecorder || !call.local) return;
    try {
      const AudioContextClass = window.AudioContext || window.webkitAudioContext;
      if (!AudioContextClass) return;
      const context = new AudioContextClass({ sampleRate: 48000 });
      if (context.state === "suspended") await context.resume();
      const destination = context.createMediaStreamDestination();
      const localSource = context.createMediaStreamSource(call.local);
      localSource.connect(destination);
      const sources = [localSource];
      if (call.remote) {
        const remoteSource = context.createMediaStreamSource(call.remote);
        remoteSource.connect(destination);
        sources.push(remoteSource);
      }
      const mime = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : "audio/webm";
      const recorder = new MediaRecorder(destination.stream, {
        mimeType: mime,
        audioBitsPerSecond: 192000,
      });
      call.chunks = [];
      const chunks = call.chunks;
      let recordingBytes = 0;
      recorder.ondataavailable = (event) => {
        if (event.data && event.data.size > 0) {
          chunks.push(event.data);
          recordingBytes += event.data.size;
          if (recordingBytes >= 8 * 1024 * 1024 && recorder.state !== "inactive") {
            recorder.stop();
            toast("通话较长，本次录音已达到容量上限；语音连接继续保持");
          }
        }
      };
      recorder.start(1000);
      call.recorder = recorder;
      call.audioContext = context;
      call.sources = sources;
      call.mime = mime;
    } catch (error) {
      console.warn("call recording unavailable", error);
    }
  }

  function toggleMute() {
    if (!call.local) return;
    call.muted = !call.muted;
    if (call.guard) {
      call.guard.setUserMuted(call.muted);
    } else {
      call.local.getAudioTracks().forEach((track) => {
        track.enabled = !call.muted;
      });
    }
    $("muteBtn").textContent = call.muted ? "取消静音" : "静音";
  }

  async function endCall(sendHangup = true, message = "") {
    if (!call.active || call.ending) return;
    call.ending = true;
    const recordingServer = state.server;
    const recordingToken = state.token;
    const snapshot = {
      callId: call.callId,
      deviceId: call.deviceId,
      mode: call.mode,
      pc: call.pc,
      local: call.local,
      micStream: call.micStream,
      guard: call.guard,
      recorder: call.recorder,
      chunks: call.chunks,
      audioContext: call.audioContext,
      sources: call.sources,
      mime: call.mime,
      connected: call.connected,
      startedIso: call.startedIso,
    };

    if (sendHangup && snapshot.callId) {
      sendSocket({
        type: "hangup",
        callId: snapshot.callId,
        deviceId: snapshot.deviceId,
        to: "device",
      });
    }
    window.clearTimeout(call.timeout);
    window.clearTimeout(call.requestTimeout);
    window.clearInterval(call.timer);
    $("callOverlay").classList.add("hidden");
    $("unlockCallAudio").classList.add("hidden");
    $("remoteAudio").srcObject = null;
    $("remoteAudio").muted = false;
    setAndroidCommunicationMode(false);

    let blob = snapshot.chunks.length > 0
      ? new Blob(snapshot.chunks, { type: snapshot.mime || "audio/webm" }) : null;
    if (snapshot.recorder && snapshot.recorder.state !== "inactive") {
      blob = await new Promise((resolve) => {
        let settled = false;
        const finish = () => {
          if (settled) return;
          settled = true;
          resolve(snapshot.chunks.length > 0
            ? new Blob(snapshot.chunks, { type: snapshot.mime || "audio/webm" })
            : null);
        };
        snapshot.recorder.onstop = finish;
        try {
          snapshot.recorder.stop();
        } catch (error) {
          finish();
        }
        window.setTimeout(finish, 1500);
      });
    }
    try {
      if (snapshot.pc) snapshot.pc.close();
    } catch (error) {
      // Ignore.
    }
    if (snapshot.guard) snapshot.guard.close();
    if (snapshot.micStream) snapshot.micStream.getTracks().forEach((track) => track.stop());
    if (snapshot.local) snapshot.local.getTracks().forEach((track) => track.stop());
    snapshot.sources.forEach((source) => {
      try {
        source.disconnect();
      } catch (error) {
        // Ignore.
      }
    });
    try {
      if (snapshot.audioContext && snapshot.audioContext.state !== "closed") {
        await snapshot.audioContext.close();
      }
    } catch (error) {
      // Ignore.
    }

    resetCallFields();
    renderDashboard();
    if (message) toast(message);

    if (snapshot.connected && snapshot.startedIso && snapshot.deviceId) {
      let audioBase64;
      let audioMime;
      if (blob && blob.size > 0) {
        try {
          audioBase64 = await blobToBase64(blob);
          audioMime = blob.type || snapshot.mime || "audio/webm";
        } catch (error) {
          console.warn("recording encode failed", error);
        }
      }
      try {
        const response = await fetch(`${recordingServer}/api/calls`, {
          method: "POST",
          signal: AbortSignal.timeout(30000),
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${recordingToken}` },
          body: JSON.stringify({
            deviceId: snapshot.deviceId,
            mode: snapshot.mode === "broadcast" ? "broadcast" : "intercom",
            startedAt: snapshot.startedIso,
            endedAt: new Date().toISOString(),
            status: "completed",
            audioBase64,
            audioMime,
          }),
        });
        if (!response.ok) throw new Error(`通话记录保存失败（${response.status}）`);
        if (recordingToken === state.token && snapshot.deviceId === state.selectedDeviceId) loadRecords();
      } catch (error) {
        console.warn("call record upload failed", error);
      }
    }
  }

  function resetCallFields() {
    call.active = false;
    call.ending = false;
    call.connected = false;
    call.callId = null;
    call.deviceId = null;
    call.mode = null;
    call.pc = null;
    call.local = null;
    call.micStream = null;
    call.guard = null;
    call.remote = null;
    call.pendingIce = [];
    call.remoteDescription = false;
    call.startedIso = null;
    call.startedAt = null;
    call.timer = null;
    call.timeout = null;
    call.requestTimeout = null;
    call.recorder = null;
    call.chunks = [];
    call.audioContext = null;
    call.sources = [];
    call.muted = false;
  }

  function blobToBase64(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = String(reader.result || "");
        resolve(result.includes(",") ? result.split(",")[1] : result);
      };
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(blob);
    });
  }

  function logout() {
    if (!window.confirm("退出登录后，后台超标通知将停止。确定退出吗？")) return;
    if (call.active) void endCall(true);
    closeSocket();
    closePlayer();
    try {
      if (window.Android) window.Android.clearSession();
    } catch (error) {
      // Ignore.
    }
    localStorage.removeItem(LS_TOKEN);
    localStorage.removeItem(LS_DEVICE);
    state.token = "";
    state.user = null;
    state.devices = [];
    state.device = null;
    state.settings = null;
    state.globalSettings = null;
    state.seenAlerts = new Set();
    state.overview = null;
      state.overviewSyncedAt = Date.now();
    state.selectedDeviceId = null;
    state.liveLatest = null;
    state.points = [];
    state.history = [];
    state.overEvents = [];
    state.rangeMinutes = 0;
    closeNoticeEditor();
    showLogin("");
  }

  const CONSENT_KEY = "soundguard.consent.v1";

  function consentAccepted() {
    try {
      return localStorage.getItem(CONSENT_KEY) === "accepted";
    } catch (error) {
      return true;
    }
  }

  function askForConsent(next) {
    const view = $("consentView");
    if (!view) {
      next();
      return;
    }
    view.classList.remove("hidden");
    const accept = () => {
      try {
        localStorage.setItem(CONSENT_KEY, "accepted");
      } catch (error) {
        // Storage may be unavailable; continue for this session.
      }
      cleanup();
      next();
    };
    const decline = () => {
      cleanup();
      if (window.Android && typeof window.Android.clearSession === "function") {
        try { window.Android.clearSession(); } catch (error) { /* Ignore. */ }
      }
      window.location.reload();
    };
    const cleanup = () => {
      view.classList.add("hidden");
      $("consentAcceptBtn").removeEventListener("click", accept);
      $("consentExitBtn").removeEventListener("click", decline);
    };
    $("consentAcceptBtn").addEventListener("click", accept);
    $("consentExitBtn").addEventListener("click", decline);
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
    if (document.fonts) document.fonts.ready.then(() => drawChart());
    withConsent(() => {
      void continueBoot();
    });
  }

  async function continueBoot() {
    const server = normalizeServer(localStorage.getItem(LS_SERVER) || "");
    const token = localStorage.getItem(LS_TOKEN) || "";
    if (!server || !token) {
      showLogin("");
      return;
    }
    state.server = server;
    state.token = token;
    showBoot();
    try {
      await enterApp();
    } catch (error) {
      if (error instanceof HttpError && error.status === 401) {
        showLogin("登录已过期，请重新输入用户名和密码");
      } else {
        state.bootError = error;
        showLogin(error.message || "无法连接服务器");
      }
    }
  }

  function bindEvents() {
    $("checkServerBtn").addEventListener("click", checkServer);
    $("changeServerBtn").addEventListener("click", changeServer);
    $("loginBtn").addEventListener("click", login);
    $("serverInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") checkServer();
    });
    $("usernameInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") $("passwordInput").focus();
    });
    $("passwordInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") login();
    });
    $("deviceSelect").addEventListener("change", (event) => {
      void switchDevice(event.target.value);
    });
    $("captureBtn").addEventListener("click", toggleCapture);
    $("callBtn").addEventListener("click", () => void startCall("intercom"));
    $("broadcastBtn").addEventListener("click", () => void startCall("broadcast"));
    $("hangupBtn").addEventListener("click", () => void endCall(true));
    $("unlockCallAudio").addEventListener("click", () => {
      const audio = $("remoteAudio");
      audio.play().then(() => {
        $("unlockCallAudio").classList.add("hidden");
        call.guard?.resume();
      }).catch(() => {
        toast("仍无法播放声音，请检查系统媒体音量");
      });
    });
    $("muteBtn").addEventListener("click", toggleMute);
    $("refreshRecords").addEventListener("click", loadRecords);
    $("tabCalls").addEventListener("click", () => setRecordTab("calls"));
    $("tabAlerts").addEventListener("click", () => setRecordTab("alerts"));
    $("closePlayer").addEventListener("click", closePlayer);
    $("editNoticeBtn").addEventListener("click", openNoticeEditor);
    $("noticeCancel").addEventListener("click", closeNoticeEditor);
    $("noticeSave").addEventListener("click", saveNotice);
    $("noticeModal").addEventListener("click", (event) => {
      if (event.target === $("noticeModal")) closeNoticeEditor();
    });
    $("rangeTabs").addEventListener("click", (event) => {
      const button = event.target.closest("button[data-minutes]");
      if (!button) return;
      void setRange(Number(button.dataset.minutes));
    });
    $("batteryBtn").addEventListener("click", () => {
      if (!window.Android) return;
      try {
        window.Android.openBatterySettings();
      } catch (error) {
        toast("请在系统设置中允许后台运行");
      }
      window.setTimeout(updateBackgroundCard, 1200);
    });
    $("notifyBtn").addEventListener("click", () => {
      if (!window.Android) return;
      try {
        window.Android.requestNotificationPermission();
      } catch (error) {
        toast("请在系统设置中开启通知");
      }
      window.setTimeout(updateBackgroundCard, 1500);
    });
    $("logoutBtn").addEventListener("click", logout);
    $("recordList").addEventListener("click", (event) => {
      const button = event.target.closest(".play-btn");
      if (!button || button.disabled) return;
      void playRecording(button.dataset.kind, button.dataset.id, button.dataset.title);
    });
    $("player").addEventListener("ended", () => {
      $("audioTitle").textContent = `${state.audioTitle} · 播放完毕`;
    });
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible" && state.token) {
        connectSocket();
        updateBackgroundCard();
        refreshOverview();
      }
    });
    window.addEventListener("online", () => {
      if (state.token) connectSocket();
    });
    window.addEventListener("resize", drawChart);
    if (window.ResizeObserver) {
      new ResizeObserver(drawChart).observe($("chart"));
    }
    window.setInterval(() => {
      if (document.visibilityState === "visible" && state.token) refreshOverview();
    }, 60000);
    window.setInterval(() => {
      if (document.visibilityState === "visible" && state.token) void refreshWarRanks();
    }, 15000);
    window.setInterval(() => {
      if (document.visibilityState === "visible" && state.token && state.rangeMinutes > 0) {
        void loadHistory(state.rangeMinutes);
      }
    }, 30000);
    window.addEventListener("beforeunload", () => {
      closeSocket();
      if (state.audioUrl) URL.revokeObjectURL(state.audioUrl);
    });
  }

  boot();
})();
