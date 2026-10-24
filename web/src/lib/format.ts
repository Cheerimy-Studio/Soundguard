import type { AlertKind, DeviceStatus, EmailStatus } from "../types";

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function formatDb(value: number | null | undefined, decimals = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "--";
  return value.toFixed(decimals);
}

export function parseDate(value: string | null | undefined): Date | null {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function formatTime(value: string | null | undefined): string {
  const date = parseDate(value);
  if (!date) return "--";
  return date.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export function formatDateTime(value: string | null | undefined): string {
  const date = parseDate(value);
  if (!date) return "--";
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export function formatFullDateTime(value: string | null | undefined): string {
  const date = parseDate(value);
  if (!date) return "--";
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

export function formatRelative(value: string | null | undefined): string {
  const date = parseDate(value);
  if (!date) return "暂无记录";
  const diff = Math.round((Date.now() - date.getTime()) / 1000);
  if (diff < 10) return "刚刚";
  if (diff < 60) return `${diff} 秒前`;
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)} 天前`;
  return formatDateTime(value);
}

export function formatDuration(seconds: number | null | undefined): string {
  if (!seconds || seconds <= 0) return "00:00";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}

export type StatusTone = "online" | "paused" | "offline" | "warning" | "error" | "waiting";

export function deviceStatusMeta(status: DeviceStatus): {
  label: string;
  tone: StatusTone;
  detail: string;
} {
  switch (status) {
    case "monitoring":
      return { label: "实时采集中", tone: "online", detail: "设备正在采集并上报环境声音" };
    case "paused":
      return { label: "采集已暂停", tone: "paused", detail: "设备在线，但管理员已暂停采集" };
    case "starting":
      return { label: "正在启动", tone: "waiting", detail: "设备已连接，等待采集器就绪" };
    case "error":
      return { label: "采集异常", tone: "error", detail: "设备在线，但麦克风采集发生错误" };
    case "offline_alert":
      return { label: "离线告警", tone: "error", detail: "设备超过离线判定时间未上报" };
    case "offline":
      return { label: "设备离线", tone: "offline", detail: "设备暂未上报心跳，等待重连" };
    case "stopped":
      return { label: "已授权关闭", tone: "offline", detail: "设备已通过密码验证正常关闭" };
    case "waiting":
    default:
      return { label: "等待接入", tone: "waiting", detail: "尚未收到该设备的心跳" };
  }
}

export function alertKindMeta(kind: AlertKind): { label: string; short: string } {
  switch (kind) {
    case "noise":
      return { label: "噪音超限", short: "噪音" };
    case "offline":
      return { label: "设备离线", short: "离线" };
    case "recovered":
      return { label: "恢复在线", short: "恢复" };
    case "stopped":
      return { label: "授权关闭", short: "关闭" };
    case "tamper":
      return { label: "安全事件", short: "安全" };
    case "control":
      return { label: "远程操作", short: "操作" };
    case "report":
      return { label: "数据日报", short: "日报" };
    case "system":
    default:
      return { label: "系统消息", short: "系统" };
  }
}

export function emailStatusMeta(status: EmailStatus): { label: string; tone: string } {
  switch (status) {
    case "sent":
      return { label: "邮件已发送", tone: "success" };
    case "dry_run":
      return { label: "本地预览", tone: "info" };
    case "pending":
      return { label: "邮件发送中", tone: "pending" };
    case "failed":
      return { label: "邮件发送失败", tone: "danger" };
    case "disabled":
    default:
      return { label: "未配置邮件", tone: "muted" };
  }
}

export function noiseTone(db: number | null | undefined, threshold: number): "quiet" | "moderate" | "loud" | "over" {
  if (db === null || db === undefined) return "quiet";
  if (db >= threshold) return "over";
  if (db >= threshold - 10) return "loud";
  if (db >= 45) return "moderate";
  return "quiet";
}

export function toneColor(tone: "quiet" | "moderate" | "loud" | "over"): string {
  switch (tone) {
    case "over":
      return "#fb7185";
    case "loud":
      return "#fbbf24";
    case "moderate":
      return "#22d3ee";
    case "quiet":
    default:
      return "#34d399";
  }
}

export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
