import { useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Bell,
  Check,
  CheckCheck,
  Download,
  Inbox,
  LoaderCircle,
  Mail,
  Play,
  Sparkles,
  ShieldAlert,
  Volume2,
  WifiOff,
  X,
} from "lucide-react";
import { api } from "../api";
import {
  alertKindMeta,
  downloadBlob,
  emailStatusMeta,
  formatDb,
  formatFullDateTime,
  formatRelative,
} from "../lib/format";
import type { Alert } from "../types";

type FilterKey = "all" | "pending" | "noise" | "safety" | "system";

const actionableKinds: Alert["kind"][] = ["noise", "offline", "tamper"];

function isActionable(alert: Alert): boolean {
  return actionableKinds.includes(alert.kind);
}

interface AlertsPageProps {
  alerts: Alert[];
  onAcknowledge: (id: string) => Promise<void>;
  onNotify: (message: string, tone?: "success" | "error" | "info") => void;
  onNavigate: (page: "overview" | "history" | "alerts" | "devices" | "settings") => void;
}

function kindIcon(kind: Alert["kind"]) {
  switch (kind) {
    case "noise":
      return <Volume2 size={18} />;
    case "offline":
      return <WifiOff size={18} />;
    case "tamper":
    case "stopped":
      return <ShieldAlert size={18} />;
    case "recovered":
      return <Check size={18} />;
    case "report":
      return <Mail size={18} />;
    default:
      return <Bell size={18} />;
  }
}

function dateGroupLabel(value: string): string {
  const date = new Date(value);
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const day = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const diff = Math.round((start.getTime() - day.getTime()) / 86400000);
  if (diff === 0) return "今天";
  if (diff === 1) return "昨天";
  return date.toLocaleDateString("zh-CN", { year: "numeric", month: "long", day: "numeric" });
}

function EvidencePlayer({
  alert,
  onNotify,
}: {
  alert: Alert;
  onNotify: AlertsPageProps["onNotify"];
}) {
  const [url, setUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  const [denoising, setDenoising] = useState(false);
  const [denoised, setDenoised] = useState(false);
  const urlRef = useRef<string | null>(null);

  function swapSource(blob: Blob) {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    const next = URL.createObjectURL(blob);
    urlRef.current = next;
    setUrl(next);
  }

  async function loadDenoised() {
    if (denoising) return;
    setDenoising(true);
    setFailed(false);
    try {
      await api.denoiseEvidence(alert.id);
      const blob = await api.evidenceDenoised(alert.id);
      swapSource(blob);
      setDenoised(true);
      onNotify("NVIDIA 云端降噪完成", "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "云端降噪失败", "error");
    } finally {
      setDenoising(false);
    }
  }

  async function load() {
    if (loading) return;
    setLoading(true);
    setFailed(false);
    try {
      const blob = await api.evidence(alert.id);
      swapSource(blob);
      setDenoised(false);
    } catch (error) {
      setFailed(true);
      onNotify(error instanceof Error ? error.message : "证据加载失败", "error");
    } finally {
      setLoading(false);
    }
  }

  async function download() {
    try {
      const blob = await api.evidence(alert.id);
      downloadBlob(blob, alert.evidenceFile ?? `evidence-${alert.id}.wav`);
      onNotify("证据录音已下载", "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "下载失败", "error");
    }
  }

  if (!alert.evidenceFile) return null;

  return (
    <div className="evidence-box">
      <div className="evidence-head">
        <div>
          <span className="evidence-label">现场录音证据</span>
          <span>{alert.evidenceDurationSecs ?? 10} 秒 · WAV</span>
        </div>
        <div className="evidence-actions">
          {!url && (
            <button type="button" className="ghost-button small" onClick={() => void load()} disabled={loading}>
              {loading ? <LoaderCircle size={13} className="spin" /> : failed ? <X size={13} /> : <Play size={13} />}
              {failed ? "重试" : "在线回放"}
            </button>
          )}
          {url && !denoised && (
            <button type="button" className="ghost-button small" onClick={() => void loadDenoised()} disabled={denoising}>
              {denoising ? <LoaderCircle size={13} className="spin" /> : <Sparkles size={13} />}
              {denoising ? "云端降噪中…" : "AI 降噪试听"}
            </button>
          )}
          {url && denoised && (
            <button type="button" className="ghost-button small" onClick={() => void load()} disabled={loading}>
              <Play size={13} />
              原始音频
            </button>
          )}
          <button type="button" className="ghost-button small" onClick={() => void download()}>
            <Download size={13} />
            下载
          </button>
        </div>
      </div>
      {url && (
        <audio
          controls
          autoPlay
          src={url}
          className="evidence-audio"
          onEnded={() => undefined}
        />
      )}
    </div>
  );
}

export function AlertsPage({ alerts, onAcknowledge, onNotify, onNavigate }: AlertsPageProps) {
  const [filter, setFilter] = useState<FilterKey>("all");
  const [acking, setAcking] = useState<string | null>(null);

  const counts = useMemo(
    () => ({
      all: alerts.length,
      pending: alerts.filter((alert) => isActionable(alert) && !alert.acknowledged).length,
      noise: alerts.filter((alert) => alert.kind === "noise").length,
      safety: alerts.filter((alert) =>
        ["offline", "recovered", "tamper", "stopped"].includes(alert.kind),
      ).length,
      system: alerts.filter((alert) => ["control", "report", "system"].includes(alert.kind)).length,
    }),
    [alerts],
  );

  const filtered = useMemo(() => {
    switch (filter) {
      case "pending":
        return alerts.filter((alert) => isActionable(alert) && !alert.acknowledged);
      case "noise":
        return alerts.filter((alert) => alert.kind === "noise");
      case "safety":
        return alerts.filter((alert) =>
          ["offline", "recovered", "tamper", "stopped"].includes(alert.kind),
        );
      case "system":
        return alerts.filter((alert) => ["control", "report", "system"].includes(alert.kind));
      case "all":
      default:
        return alerts;
    }
  }, [alerts, filter]);

  const groups = useMemo(() => {
    const map = new Map<string, Alert[]>();
    for (const alert of filtered) {
      const label = dateGroupLabel(alert.createdAt);
      const list = map.get(label) ?? [];
      list.push(alert);
      map.set(label, list);
    }
    return [...map.entries()];
  }, [filtered]);

  const todayCount = alerts.filter(
    (alert) => isActionable(alert) && dateGroupLabel(alert.createdAt) === "今天",
  ).length;
  const acknowledgedCount = alerts.filter(
    (alert) => isActionable(alert) && alert.acknowledged,
  ).length;
  const evidenceCount = alerts.filter((alert) => alert.evidenceFile).length;

  async function acknowledge(id: string) {
    if (acking) return;
    setAcking(id);
    try {
      await onAcknowledge(id);
    } finally {
      setAcking(null);
    }
  }

  return (
    <div className="alerts-page">
      <section className="alerts-summary">
        <div className="panel summary-card">
          <div className="summary-icon rose">
            <Bell size={20} />
          </div>
          <div>
            <span>待处理告警</span>
            <strong>{counts.pending}</strong>
            <small>需要管理员确认</small>
          </div>
        </div>
        <div className="panel summary-card">
          <div className="summary-icon amber">
            <AlertTriangle size={20} />
          </div>
          <div>
            <span>今日新增</span>
            <strong>{todayCount}</strong>
            <small>含噪音、离线与安全事件</small>
          </div>
        </div>
        <div className="panel summary-card">
          <div className="summary-icon cyan">
            <Volume2 size={20} />
          </div>
          <div>
            <span>录音证据</span>
            <strong>{evidenceCount}</strong>
            <small>超限现场音频已留存</small>
          </div>
        </div>
        <div className="panel summary-card">
          <div className="summary-icon emerald">
            <CheckCheck size={20} />
          </div>
          <div>
            <span>已确认</span>
            <strong>{acknowledgedCount}</strong>
            <small>历史事件保持审计可追溯</small>
          </div>
        </div>
      </section>

      <section className="panel alerts-panel">
        <div className="alerts-toolbar">
          <div>
            <div className="eyebrow">告警记录</div>
            <h3>告警事件流</h3>
          </div>
          <div className="filter-tabs">
            {(
              [
                ["all", "全部"],
                ["pending", "待处理"],
                ["noise", "噪音"],
                ["safety", "设备安全"],
                ["system", "系统"],
              ] as [FilterKey, string][]
            ).map(([key, label]) => (
              <button
                key={key}
                type="button"
                className={filter === key ? "is-active" : ""}
                onClick={() => setFilter(key)}
              >
                {label}
                <span>{counts[key]}</span>
              </button>
            ))}
          </div>
        </div>

        {filtered.length === 0 ? (
          <div className="empty-state large">
            <div className="empty-icon">
              <Inbox size={26} />
            </div>
            <strong>{filter === "pending" ? "没有待处理告警" : "当前分类暂无记录"}</strong>
            <p>系统会把噪音超限、设备离线、安全验证等事件记录在这里。</p>
            <button type="button" className="ghost-button" onClick={() => onNavigate("overview")}>
              返回实时总览
            </button>
          </div>
        ) : (
          <div className="alert-groups">
            {groups.map(([label, items]) => (
              <div className="alert-group" key={label}>
                <div className="alert-group-head">
                  <span>{label}</span>
                  <em>{items.length} 条</em>
                </div>
                {items.map((alert) => {
                  const kind = alertKindMeta(alert.kind);
                  const mail = emailStatusMeta(alert.emailStatus);
                  return (
                    <article
                      key={alert.id}
                      className={`alert-card severity-${alert.severity} ${alert.acknowledged ? "is-acked" : ""}`}
                    >
                      <div className="alert-rail" />
                      <div className={`alert-icon ${alert.severity}`}>{kindIcon(alert.kind)}</div>
                      <div className="alert-body">
                        <div className="alert-toprow">
                          <div className="alert-title-row">
                            <h4>{alert.title}</h4>
                            <span className={`kind-chip kind-${alert.kind}`}>{kind.label}</span>
                            {alert.db !== null && <span className="db-chip">{formatDb(alert.db)} dB</span>}
                          </div>
                          <div className="alert-time">
                            <span>{formatFullDateTime(alert.createdAt)}</span>
                            <em>{formatRelative(alert.createdAt)}</em>
                          </div>
                        </div>
                        <p className="alert-message">{alert.message}</p>
                        {alert.emailError && alert.emailStatus === "failed" && (
                          <div className="alert-email-error">{alert.emailError}</div>
                        )}
                        <div className="alert-foot">
                          <div className="alert-meta">
                            <span className={`email-chip ${mail.tone}`}>
                              <Mail size={11} />
                              {mail.label}
                            </span>
                            {alert.acknowledged && (
                              <span className="acked-chip">
                                <CheckCheck size={11} />
                                已确认
                                {alert.acknowledgedAt ? ` · ${formatRelative(alert.acknowledgedAt)}` : ""}
                              </span>
                            )}
                          </div>
                          <div className="alert-actions">
                            {!alert.acknowledged && isActionable(alert) && (
                              <button
                                type="button"
                                className="ack-button"
                                disabled={acking === alert.id}
                                onClick={() => void acknowledge(alert.id)}
                              >
                                {acking === alert.id ? (
                                  <LoaderCircle size={13} className="spin" />
                                ) : (
                                  <Check size={13} />
                                )}
                                标记已处理
                              </button>
                            )}
                          </div>
                        </div>
                        <EvidencePlayer alert={alert} onNotify={onNotify} />
                      </div>
                    </article>
                  );
                })}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
