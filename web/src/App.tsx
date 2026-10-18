import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Bell,
  ChevronRight,
  LayoutDashboard,
  LineChart,
  LogOut,
  RefreshCw,
  Server,
  Settings as SettingsIcon,
  UsersRound,
  Waves,
  Wifi,
  WifiOff,
} from "lucide-react";
import { effectiveSettings } from "./lib/settings";
import { api, adminWebSocketUrl, clearToken, getToken } from "./api";
import { Login } from "./components/Login";
import { CallDock } from "./components/Intercom";
import { OverviewPage } from "./pages/OverviewPage";
import { HistoryPage } from "./pages/HistoryPage";
import { AlertsPage } from "./pages/AlertsPage";
import { DevicesPage } from "./pages/DevicesPage";
import { SettingsPage } from "./pages/SettingsPage";
import { UsersPage } from "./pages/UsersPage";
import { useIntercom } from "./hooks/useIntercom";
import { deviceStatusMeta, downloadBlob, formatRelative } from "./lib/format";
import type {
  Alert,
  Device,
  LiveMessage,
  Overview,
  Reading,
  Settings,
  User,
} from "./types";

type PageKey = "overview" | "history" | "alerts" | "devices" | "settings" | "users";

const NAV_ITEMS: { key: PageKey; label: string; icon: typeof LayoutDashboard; hint: string }[] = [
  { key: "overview", label: "实时总览", icon: LayoutDashboard, hint: "设备状态" },
  { key: "history", label: "历史数据", icon: LineChart, hint: "声学记录" },
  { key: "alerts", label: "告警中心", icon: Bell, hint: "事件处置" },
  { key: "devices", label: "设备连接", icon: Server, hint: "设备与策略" },
  { key: "settings", label: "系统设置", icon: SettingsIcon, hint: "全局默认" },
  { key: "users", label: "用户管理", icon: UsersRound, hint: "角色与权限" },
];

const PAGE_META: Record<PageKey, { title: string; subtitle: string }> = {
  overview: { title: "实时总览", subtitle: "环境声学状态与设备运行概览" },
  history: { title: "历史数据", subtitle: "回溯分贝波动，导出监测记录" },
  alerts: { title: "告警中心", subtitle: "噪音、离线与安全事件统一处置" },
  devices: { title: "设备连接", subtitle: "接入新设备，管理每台设备的独立令牌" },
  settings: { title: "系统设置", subtitle: "全局默认策略、通知与数据留存" },
  users: { title: "用户管理", subtitle: "超级管理员与设备管理员" },
};

interface ToastState {
  id: number;
  message: string;
  tone: "success" | "error" | "info";
}

function isToday(value: string): boolean {
  const date = new Date(value);
  const now = new Date();
  return (
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  );
}

const actionableAlertKinds = new Set(["noise", "offline", "tamper"]);

function mergeDevice(list: Device[], device: Device): Device[] {
  const exists = list.some((item) => item.id === device.id);
  return exists
    ? list.map((item) => (item.id === device.id ? device : item))
    : [...list, device];
}

export default function App() {
  const [token, setTokenState] = useState<string | null>(() => getToken());
  const [page, setPage] = useState<PageKey>("overview");
  const [overview, setOverview] = useState<Overview | null>(null);
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [liveReadings, setLiveReadings] = useState<Reading[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(() =>
    localStorage.getItem("soundguard.deviceId"),
  );
  const [signalingByDevice, setSignalingByDevice] = useState<Record<string, boolean>>({});
  const [currentUser, setCurrentUser] = useState<User | null>(null);
  const [userLoading, setUserLoading] = useState(Boolean(token));
  const [loading, setLoading] = useState(Boolean(token));
  const [loadError, setLoadError] = useState("");
  const [wsConnected, setWsConnected] = useState(false);
  const [clock, setClock] = useState(new Date());
  const [toast, setToast] = useState<ToastState | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  const wsRef = useRef<WebSocket | null>(null);
  const signalHandlers = useRef(new Set<(message: LiveMessage) => void>());
  const lastReadingRef = useRef<string | null>(null);
  const toastTimerRef = useRef<number | null>(null);
  const bootstrappedRef = useRef(false);
  const selectedRef = useRef<string | null>(
    localStorage.getItem("soundguard.deviceId"),
  );
  const overviewRef = useRef<Overview | null>(null);
  const loadGeneration = useRef(0);
  const loadDeviceRef = useRef<(id: string | null) => Promise<void>>(async () => {});
  const hangupRef = useRef<() => void>(() => {});
  const globalSettingsRef = useRef<Settings | null>(null);

  const notify = useCallback((message: string, tone: ToastState["tone"] = "info") => {
    if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current);
    setToast({ id: Date.now(), message, tone });
    toastTimerRef.current = window.setTimeout(() => setToast(null), 4200);
  }, []);

  const registerSignalHandler = useCallback((handler: (message: LiveMessage) => void) => {
    signalHandlers.current.add(handler);
    return () => {
      signalHandlers.current.delete(handler);
    };
  }, []);

  const sendSignal = useCallback((message: unknown): boolean => {
    const socket = wsRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return false;
    socket.send(JSON.stringify(message));
    return true;
  }, []);

  const iceServers = useMemo<RTCIceServer[]>(
    () =>
      overview?.iceServers?.length
        ? overview.iceServers
        : [{ urls: "stun:stun.l.google.com:19302" }],
    [overview?.iceServers],
  );

  const intercom = useIntercom({
    iceServers,
    sendSignal,
    registerSignalHandler,
    deviceId: selectedDeviceId,
  });

  hangupRef.current = intercom.hangup;

  const signalingConnected = selectedDeviceId
    ? Boolean(signalingByDevice[selectedDeviceId])
    : false;

  const applyLoadedOverview = useCallback((next: Overview) => {
    const id = next.device.id;
    selectedRef.current = id;
    setSelectedDeviceId(id);
    localStorage.setItem("soundguard.deviceId", id);
    setOverview(next);
    overviewRef.current = next;
    setSignalingByDevice((previous) => ({
      ...previous,
      [id]: next.signalingConnected,
    }));
  }, []);

  const loadForDevice = useCallback(
    async (deviceId: string | null) => {
      const generation = ++loadGeneration.current;
      const sessionToken = getToken();
      let next: Overview;
      try {
        next = await api.overview(deviceId);
      } catch (error) {
        if (
          deviceId &&
          error instanceof Error &&
          "status" in error &&
          [403, 404].includes((error as { status: number }).status)
        ) {
          selectedRef.current = null;
          localStorage.removeItem("soundguard.deviceId");
          next = await api.overview();
        } else {
          throw error;
        }
      }
      if (generation !== loadGeneration.current || sessionToken !== getToken()) return;
      applyLoadedOverview(next);
      const now = new Date();
      const [nextAlerts, readings] = await Promise.all([
        api.alerts(next.device.id, 200),
        api.readings(new Date(now.getTime() - 30 * 60 * 1000), now, next.device.id, 120),
      ]);
      if (generation !== loadGeneration.current || sessionToken !== getToken()) return;
      setAlerts(nextAlerts);
      const sorted = readings.readings.sort(
        (a, b) => new Date(a.recordedAt).getTime() - new Date(b.recordedAt).getTime(),
      );
      setLiveReadings(sorted);
      lastReadingRef.current =
        sorted.at(-1)?.recordedAt ?? next.device.lastReadingAt ?? null;
    },
    [applyLoadedOverview],
  );

  loadDeviceRef.current = loadForDevice;

  const handleSelectDevice = useCallback(
    (deviceId: string) => {
      if (!deviceId || deviceId === selectedRef.current) return;
      intercom.hangup();
      selectedRef.current = deviceId;
      setSelectedDeviceId(deviceId);
      localStorage.setItem("soundguard.deviceId", deviceId);
      setAlerts([]);
      setLiveReadings([]);
      lastReadingRef.current = null;
      void loadForDevice(deviceId).catch((error) => {
        notify(error instanceof Error ? error.message : "切换设备失败", "error");
      });
    },
    [intercom, loadForDevice, notify],
  );

  const handleDevicesUpdated = useCallback(async () => {
    try {
      const current = selectedRef.current;
      if (current) {
        await loadForDevice(current);
      }
    } catch {
      // The selected device may have been removed; the devices page chooses a new one.
    }
  }, [loadForDevice]);

  const handleLiveMessage = useCallback(
    (message: LiveMessage) => {
      switch (message.type) {
        case "snapshot": {
          globalSettingsRef.current = message.data.settings;
          const selected =
            message.data.devices.find((device) => device.id === selectedRef.current) ??
            message.data.devices[0];
          if (selected) {
            if (selected.id !== selectedRef.current) {
              hangupRef.current();
              setAlerts([]); setLiveReadings([]); lastReadingRef.current = null;
              void loadDeviceRef.current(selected.id).catch(() => undefined);
            }
            selectedRef.current = selected.id;
            setSelectedDeviceId(selected.id);
            localStorage.setItem("soundguard.deviceId", selected.id);
          }
          setSignalingByDevice((previous) => {
            const next = { ...previous };
            for (const [deviceId, connected] of Object.entries(message.data.signaling)) {
              next[deviceId] = connected;
            }
            return next;
          });
          setOverview((previous) => {
            if (!previous) return previous;
            const nextSelected =
              message.data.devices.find((device) => device.id === selectedRef.current) ??
              selected;
            const next = {
              ...previous,
              devices: message.data.devices,
              device: nextSelected ?? previous.device,
              settings: nextSelected ? effectiveSettings(message.data.settings, nextSelected) : previous.settings,
              serverTime: message.data.serverTime,
            };
            overviewRef.current = next;
            return next;
          });
          break;
        }
        case "device": {
          const device = message.data;
          setOverview((previous) => {
            if (!previous) return previous;
            const next = {
              ...previous,
              devices: mergeDevice(previous.devices, device),
              device: device.id === previous.device.id ? device : previous.device,
              settings: device.id === previous.device.id ? effectiveSettings(globalSettingsRef.current ?? previous.settings, device) : previous.settings,
            };
            overviewRef.current = next;
            return next;
          });
          if (device.id !== selectedRef.current) break;
          break;
        }
        case "live": {
          const live = message.data;
          if (live.deviceId !== selectedRef.current) break;
          setOverview((previous) => {
            if (!previous) return previous;
            const nextDevice = {
              ...previous.device,
              ...live.device,
              currentDb: live.currentDb,
              liveAvgDb: live.averageDb,
              peakDb: live.peakDb ?? previous.device.peakDb,
              liveSuspended: live.suspended,
              liveSystemPlaying: live.systemPlaying,
              liveAecEnabled: live.aecEnabled,
            };
            const next = {
              ...previous,
              device: nextDevice,
              devices: mergeDevice(previous.devices, nextDevice),
            };
            overviewRef.current = next;
            return next;
          });
          if (
            live.capturing
            && !live.suspended
            && !(live.systemPlaying && !live.aecEnabled)
            && live.currentDb !== null
          ) {
            const recordedAt = live.recordedAt;
            if (lastReadingRef.current !== recordedAt) {
              lastReadingRef.current = recordedAt;
              const reading: Reading = {
                deviceId: live.deviceId,
                recordedAt,
                avgDb: live.currentDb,
                peakDb: live.peakDb ?? live.currentDb,
                minDb: live.currentDb,
                overThreshold:
                  live.averageDb !== null &&
                  live.averageDb >= (overviewRef.current?.settings.thresholdDb ?? 65),
                durationSecs: 1,
                sampleCount: 1,
              };
              setLiveReadings((previous) => {
                if (previous.some((item) => item.recordedAt === recordedAt)) return previous;
                return [...previous, reading]
                  .sort(
                    (a, b) =>
                      new Date(a.recordedAt).getTime() - new Date(b.recordedAt).getTime(),
                  )
                  .slice(-1800);
              });
            }
          }
          break;
        }
        case "alert": {
          const alert = message.data;
          if (alert.deviceId && alert.deviceId !== selectedRef.current) break;
          setOverview((previous) => {
            if (!previous) return previous;
            const exists = previous.alerts.some((item) => item.id === alert.id);
            const nextAlerts = exists
              ? previous.alerts.map((item) => (item.id === alert.id ? alert : item))
              : [alert, ...previous.alerts].slice(0, 6);
            const next = {
              ...previous,
              alerts: nextAlerts,
              alertCountToday:
                !exists && isToday(alert.createdAt) && actionableAlertKinds.has(alert.kind)
                  ? previous.alertCountToday + 1
                  : previous.alertCountToday,
            };
            overviewRef.current = next;
            return next;
          });
          setAlerts((previous) => {
            const exists = previous.some((item) => item.id === alert.id);
            return exists
              ? previous.map((item) => (item.id === alert.id ? alert : item))
              : [alert, ...previous].slice(0, 500);
          });
          break;
        }
        case "settings": {
          globalSettingsRef.current = message.data;
          setOverview((previous) => {
            if (!previous) return previous;
            const next = { ...previous, settings: effectiveSettings(message.data, previous.device) };
            overviewRef.current = next;
            return next;
          });
          break;
        }
        case "signaling": {
          const deviceId = message.data.deviceId;
          if (deviceId) {
            setSignalingByDevice((previous) => ({
              ...previous,
              [deviceId]: Boolean(message.data.connected),
            }));
          }
          break;
        }
        case "devices": {
          const nextSelected = message.data.find((d) => d.id === selectedRef.current) ?? message.data[0];
          if (nextSelected && nextSelected.id !== selectedRef.current) {
            hangupRef.current();
            selectedRef.current = nextSelected.id;
            setSelectedDeviceId(nextSelected.id);
            localStorage.setItem("soundguard.deviceId", nextSelected.id);
            setAlerts([]); setLiveReadings([]); lastReadingRef.current = null;
            void loadDeviceRef.current(nextSelected.id).catch(() => undefined);
          }
          setOverview((previous) => {
            if (!previous) return previous;
            const current = message.data.find(
              (device) => device.id === previous.device.id,
            );
            const nextDevice = current ?? message.data[0] ?? previous.device;
            const next = {
              ...previous,
              devices: message.data,
              device: nextDevice,
            };
            overviewRef.current = next;
            return next;
          });
          break;
        }
        case "signal":
        case "hangup":
        case "signal-error": {
          signalHandlers.current.forEach((handler) => handler(message));
          break;
        }
        default:
          break;
      }
    },
    [],
  );

  useEffect(() => {
    const timer = window.setInterval(() => setClock(new Date()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const handleLogout = () => {
      ++loadGeneration.current;
      intercom.hangup();
      overviewRef.current = null;
      setWsConnected(false);
      setTokenState(null);
      setOverview(null);
      setAlerts([]);
      setLiveReadings([]);
      setLoading(false);
    };
    window.addEventListener("soundguard:logout", handleLogout);
    return () => window.removeEventListener("soundguard:logout", handleLogout);
  }, [intercom.hangup]);

  useEffect(() => {
    if (!token) {
      setCurrentUser(null);
      setUserLoading(false);
      return;
    }
    let cancelled = false;
    setUserLoading(true);
    api
      .me()
      .then((user) => {
        if (cancelled) return;
        setCurrentUser(user);
        if (user.role !== "super_admin") {
          setPage((current) =>
            current === "settings" || current === "users" ? "overview" : current,
          );
        }
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setUserLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [token]);

  useEffect(() => {
    if (!token) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setLoadError("");
    loadForDevice(selectedRef.current)
      .then(() => {
        if (!cancelled) bootstrappedRef.current = true;
      })
      .catch((error) => {
        if (cancelled) return;
        if (error instanceof Error && !("status" in error && (error as { status: number }).status === 401)) {
          setLoadError(error.message);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [token, reloadKey, loadForDevice]);

  useEffect(() => {
    if (!token) return;
    let cancelled = false;
    let reconnectTimer: number | null = null;
    let attempts = 0;

    const refreshSelected = () => {
      const id = selectedRef.current;
      if (!id) return;
      void Promise.all([api.overview(id), api.alerts(id, 200)])
        .then(([nextOverview, nextAlerts]) => {
          if (cancelled || id !== selectedRef.current) return;
          setOverview(nextOverview);
          overviewRef.current = nextOverview;
          setAlerts(nextAlerts);
        })
        .catch(() => undefined);
    };

    const connect = () => {
      if (cancelled) return;
      const socket = new WebSocket(adminWebSocketUrl());
      wsRef.current = socket;
      socket.onopen = () => {
        if (cancelled) return;
        attempts = 0;
        setWsConnected(true);
        if (bootstrappedRef.current) refreshSelected();
      };
      socket.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data) as LiveMessage;
          handleLiveMessage(message);
        } catch {
          // Ignore malformed signaling frames.
        }
      };
      socket.onclose = () => {
        if (cancelled) return;
        hangupRef.current();
        setWsConnected(false);
        wsRef.current = null;
        const delay = Math.min(15000, 900 * 2 ** attempts);
        attempts += 1;
        reconnectTimer = window.setTimeout(connect, delay);
      };
      socket.onerror = () => {
        socket.close();
      };
    };

    connect();
    return () => {
      cancelled = true;
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      wsRef.current?.close();
      wsRef.current = null;
      setWsConnected(false);
    };
  }, [token, handleLiveMessage]);

  useEffect(() => {
    if (!token) return;
    const timer = window.setInterval(() => {
      if (document.hidden) return;
      const id = selectedRef.current;
      if (!id) return;
      void api
        .overview(id)
        .then((next) => {
          if (id !== selectedRef.current) return;
          setOverview(next);
          overviewRef.current = next;
        })
        .catch(() => undefined);
      void api
        .alerts(id, 200)
        .then((next) => { if (id === selectedRef.current) setAlerts(next); })
        .catch(() => undefined);
    }, 60_000);
    return () => window.clearInterval(timer);
  }, [token]);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current);
    };
  }, []);

  const handleLogin = useCallback((nextToken: string, user: User) => {
    setTokenState(nextToken);
    setCurrentUser(user);
    setPage("overview");
  }, []);

  const handleLogout = useCallback(() => {
    ++loadGeneration.current;
    intercom.hangup();
    clearToken();
    overviewRef.current = null;
    setWsConnected(false);
    setTokenState(null);
    setOverview(null);
    setCurrentUser(null);
    setAlerts([]);
    setLiveReadings([]);
    selectedRef.current = null;
    localStorage.removeItem("soundguard.deviceId");
    notify("已安全退出管理端", "info");
  }, [notify, intercom.hangup]);

  const handleControl = useCallback(
    async (monitoring: boolean) => {
      const deviceId = selectedRef.current;
      if (!deviceId) return;
      try {
        const result = await api.controlDevice(monitoring, deviceId);
        if (selectedRef.current !== deviceId) return;
        setOverview((previous) => {
          if (!previous) return previous;
          const next = {
            ...previous,
            device: result.device,
            devices: mergeDevice(previous.devices, result.device),
            settings: result.settings,
          };
          overviewRef.current = next;
          return next;
        });
        notify(monitoring ? "已下发恢复采集指令" : "已下暂停采集指令", "success");
      } catch (error) {
        notify(error instanceof Error ? error.message : "操作失败", "error");
        throw error;
      }
    },
    [notify],
  );

  const handleAcknowledge = useCallback(
    async (id: string) => {
      try {
        const updated = await api.acknowledgeAlert(id);
        setAlerts((previous) => previous.map((item) => (item.id === id ? updated : item)));
        setOverview((previous) =>
          previous
            ? {
                ...previous,
                alerts: previous.alerts.map((item) =>
                  item.id === id ? updated : item,
                ),
              }
            : previous,
        );
        notify("告警已确认", "success");
      } catch (error) {
        notify(error instanceof Error ? error.message : "确认失败", "error");
      }
    },
    [notify],
  );

  const handleSaveSettings = useCallback(
    async (settings: Settings) => {
      const result = await api.updateSettings(settings);
      const deviceId = selectedRef.current;
      if (deviceId) {
        const refreshed = await api.overview(deviceId);
        setOverview(refreshed);
        overviewRef.current = refreshed;
      }
      notify("设置已保存并同步到所有设备", "success");
      return result.settings;
    },
    [notify],
  );

  const handleRefresh = useCallback(() => {
    setReloadKey((value) => value + 1);
    notify("正在重新同步数据", "info");
  }, [notify]);

  const handleExportRecent = useCallback(async () => {
    if (!selectedRef.current) return;
    try {
      const to = new Date();
      const from = new Date(to.getTime() - 24 * 3600 * 1000);
      const blob = await api.exportReadings(from, to, selectedRef.current);
      downloadBlob(blob, `soundguard-24h-${to.toISOString().slice(0, 10)}.csv`);
      notify("24 小时数据已导出", "success");
    } catch (error) {
      notify(error instanceof Error ? error.message : "导出失败", "error");
    }
  }, [notify]);

  if (!token) {
    return <Login onAuthenticated={handleLogin} />;
  }

  if (userLoading && !currentUser) {
    return (
      <div className="boot-screen">
        <div className="brand-mark boot-mark">
          <Waves size={26} />
        </div>
        <div className="boot-title">正在验证账号权限</div>
        <div className="boot-bar"><span /></div>
        <div className="boot-sub">加载角色与设备范围…</div>
      </div>
    );
  }

  if ((loading || userLoading) && !overview) {
    return (
      <div className="boot-screen">
        <div className="brand-mark boot-mark">
          <Waves size={26} />
        </div>
        <div className="boot-title">正在建立安全连接</div>
        <div className="boot-bar">
          <span />
        </div>
        <div className="boot-sub">同步设备状态与声学数据…</div>
      </div>
    );
  }

  if (loadError && !overview) {
    return (
      <div className="boot-screen boot-error">
        <div className="boot-error-icon">
          <WifiOff size={25} />
        </div>
        <div className="boot-title">无法连接监测服务</div>
        <p className="boot-sub">{loadError}</p>
        <div className="boot-actions">
          <button className="primary-button" type="button" onClick={handleRefresh}>
            <RefreshCw size={16} />
            重试连接
          </button>
          <button className="ghost-button" type="button" onClick={handleLogout}>
            返回登录
          </button>
        </div>
      </div>
    );
  }

  if (!overview) return null;

  const status = deviceStatusMeta(overview.device.status);
  const meta = PAGE_META[page];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-brand">
          <img src="/logo-64.png" alt="声垣" className="brand-logo" width={40} height={40} />
          <div>
            <div className="brand-name">声垣</div>
          </div>
        </div>

        <nav className="sidebar-nav">
          <div className="nav-section-label">监测控制台</div>
          {NAV_ITEMS.filter((item) =>
            currentUser?.role === "super_admin"
              || (item.key !== "settings" && item.key !== "users"),
          ).map((item) => {
            const Icon = item.icon;
            const active = page === item.key;
            return (
              <button
                key={item.key}
                type="button"
                className={`nav-item ${active ? "is-active" : ""}`}
                onClick={() => setPage(item.key)}
              >
                <span className="nav-icon">
                  <Icon size={18} />
                </span>
                <span className="nav-copy">
                  <strong>{item.label}</strong>
                  <small>{item.hint}</small>
                </span>
                {item.key === "alerts" && overview.alertCountToday > 0 && (
                  <span className="nav-badge">
                    {overview.alertCountToday > 99 ? "99+" : overview.alertCountToday}
                  </span>
                )}
                {active && <span className="nav-active-mark" />}
              </button>
            );
          })}
        </nav>

        <div className="sidebar-bottom">
          <button type="button" className="logout-link" onClick={handleLogout}>
            <LogOut size={16} />
            退出登录
          </button>
          <span className="version-label">© 2026 Cheerimy Studio</span>
        </div>
      </aside>

      <div className="app-main">
        <header className="topbar">
          <div className="topbar-title">
            <div className="eyebrow">{meta.subtitle}</div>
            <h1>{meta.title}</h1>
          </div>
          <div className="topbar-right">
            {currentUser && (
              <div className="account-chip" title={currentUser.role === "super_admin" ? "超级管理员" : "设备管理员"}>
                <UsersRound size={14} />
                <span>{currentUser.username}</span>
              </div>
            )}
            <div className="device-switcher">
              <Server size={14} />
              <select
                value={overview.device.id}
                onChange={(event) => handleSelectDevice(event.target.value)}
                aria-label="切换设备"
              >
                {overview.devices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="live-clock">
              <span>
                {clock.toLocaleDateString("zh-CN", {
                  month: "2-digit",
                  day: "2-digit",
                  weekday: "short",
                })}
              </span>
              <strong>{clock.toLocaleTimeString("zh-CN", { hour12: false })}</strong>
            </div>
            <div
              className={`connection-badge ${
                wsConnected ? "is-online" : "is-offline"
              }`}
            >
              {wsConnected ? <Wifi size={14} /> : <WifiOff size={14} />}
              {wsConnected ? "实时已连接" : "实时重连中"}
            </div>
            <button
              type="button"
              className="icon-button"
              onClick={handleRefresh}
              aria-label="刷新数据"
              title="刷新数据"
            >
              <RefreshCw size={17} />
            </button>
          </div>
        </header>

        {!overview.device.online && (
          <div className="offline-banner">
            <WifiOff size={16} />
            <div className="offline-banner-copy">
              <span>{status.detail}</span>
              {overview.device.lastSeen && (
                <>
                  <span className="meta-separator" aria-hidden="true">·</span>
                  <span>最后心跳 {formatRelative(overview.device.lastSeen)}</span>
                </>
              )}
            </div>
            <button type="button" onClick={() => setPage("devices")}>
              设备详情
            </button>
          </div>
        )}

        <main className="page-content">
          {page === "overview" && (
            <OverviewPage
              overview={overview}
              alerts={alerts}
              liveReadings={liveReadings}
              intercom={intercom}
              signalingConnected={signalingConnected}
              onControl={handleControl}
              onAcknowledge={handleAcknowledge}
              onNavigate={setPage}
              onExport={handleExportRecent}
            />
          )}
          {page === "history" && (
            <HistoryPage
              threshold={overview.settings.thresholdDb}
              deviceId={overview.device.id}
              onNotify={notify}
            />
          )}
          {page === "alerts" && (
            <AlertsPage
              alerts={alerts}
              onAcknowledge={handleAcknowledge}
              onNotify={notify}
              onNavigate={setPage}
            />
          )}
          {page === "devices" && (
            <DevicesPage
              devices={overview.devices}
              selectedDeviceId={overview.device.id}
              canManage={Boolean(currentUser)}
              onSelectDevice={handleSelectDevice}
              onDevicesUpdated={handleDevicesUpdated}
              onNotify={notify}
            />
          )}
          {page === "settings" && currentUser?.role === "super_admin" && (
            <SettingsPage
              overview={overview}
              deviceId={overview.device.id}
              onSave={handleSaveSettings}
              onNotify={notify}
            />
          )}
          {page === "users" && currentUser?.role === "super_admin" && (
            <UsersPage
              devices={overview.devices}
              currentUser={currentUser}
              onNotify={notify}
            />
          )}
        </main>

        <nav className="mobile-nav">
          {NAV_ITEMS.filter((item) =>
            currentUser?.role === "super_admin"
              || (item.key !== "settings" && item.key !== "users"),
          ).map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.key}
                type="button"
                className={page === item.key ? "is-active" : ""}
                onClick={() => setPage(item.key)}
              >
                <Icon size={19} />
                <span>{item.label.slice(0, 2)}</span>
              </button>
            );
          })}
        </nav>
      </div>

      <CallDock controller={intercom} deviceName={overview.device.name} />

      {toast && (
        <div className={`toast toast-${toast.tone}`} key={toast.id}>
          <span className="toast-dot" />
          {toast.message}
        </div>
      )}
    </div>
  );
}
