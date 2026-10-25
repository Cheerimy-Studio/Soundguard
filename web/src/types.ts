export type DeviceStatus =
  | "monitoring"
  | "paused"
  | "starting"
  | "error"
  | "offline"
  | "offline_alert"
  | "stopped"
  | "waiting";

export interface DeviceSettings {
  thresholdDb: number | null;
  alertCooldownSecs: number | null;
  offlineTimeoutSecs: number | null;
  autoRecordEvidence: boolean | null;
}

export interface NvidiaKeyState {
  configured: boolean;
  masked: string;
}

export type UserRole = "super_admin" | "update_admin" | "device_admin";

export interface User {
  id: string;
  username: string;
  email: string;
  role: UserRole;
  deviceIds: string[];
  disabled: boolean;
  createdAt: string;
  lastLoginAt: string | null;
}

export interface Device {
  id: string;
  createdAt?: string | null;
  name: string;
  settings: DeviceSettings;
  desiredMonitoring: boolean;
  monitoring: boolean;
  currentDb: number | null;
  peakDb: number | null;
  lastSeen: string | null;
  lastReadingAt: string | null;
  lastStopAt: string | null;
  lastHighAlertAt: string | null;
  expectedOffline: boolean;
  offlineAlertActive: boolean;
  appVersion: string | null;
  captureError: string | null;
  liveAvgDb: number | null;
  liveRolling15sDb: number | null;
  liveSuspended: boolean;
  liveSystemPlaying: boolean;
  liveAecEnabled: boolean;
  online: boolean;
  status: DeviceStatus;
}

export interface Settings {
  thresholdDb: number;
  alertRecipients: string[];
  reportRecipients: string[];
  dailyReportEnabled: boolean;
  dailyReportTime: string;
  denoiseMode: string;
  voiceEnhance: boolean;
  warModeEnabled: boolean;
  warModeStart: string;
  warModeEnd: string;
  fixedPasswordHash: string;
  captureStart: string;
  captureEnd: string;
  offlineTimeoutSecs: number;
  alertCooldownSecs: number;
  evidenceSeconds: number;
  autoRecordEvidence: boolean;
  retentionDays: number;
  notificationEnabled: boolean;
  notificationTitle: string;
  notificationContent: string;
}

export interface Stats {
  avgDb: number | null;
  peakDb: number | null;
  minDb: number | null;
  overCount: number;
  sampleCount: number;
}

export interface Reading {
  deviceId: string;
  recordedAt: string;
  avgDb: number;
  peakDb: number;
  minDb: number;
  overThreshold: boolean;
  durationSecs: number;
  sampleCount: number;
}

export type AlertKind =
  | "noise"
  | "offline"
  | "recovered"
  | "stopped"
  | "tamper"
  | "control"
  | "report"
  | "system";

export type EmailStatus = "pending" | "sent" | "dry_run" | "disabled" | "failed";

export interface Alert {
  id: string;
  deviceId: string;
  kind: AlertKind;
  severity: "critical" | "warning" | "info";
  title: string;
  message: string;
  createdAt: string;
  acknowledged: boolean;
  acknowledgedAt: string | null;
  db: number | null;
  evidenceFile: string | null;
  evidenceDurationSecs: number | null;
  emailStatus: EmailStatus;
  emailError: string | null;
}

export interface Overview {
  devices: Device[];
  device: Device;
  settings: Settings;
  stats: Stats;
  alerts: Alert[];
  alertCountToday: number;
  smtpConfigured: boolean;
  serverTime: string;
  serverUtcOffsetSecs?: number;
  iceServers: RTCIceServer[];
  signalingConnected: boolean;
}

export interface ReadingsResponse {
  from: string;
  to: string;
  thresholdDb: number;
  readings: Reading[];
  stats: Stats;
}

export interface SettingsResponse {
  settings: Settings;
  smtpConfigured: boolean;
  iceServers: RTCIceServer[];
}

export interface LoginResponse {
  token: string;
  expiresAt: string;
  user: User;
}

export interface DeviceControlResponse {
  device: Device;
  settings: Settings;
}

export interface ReportResult {
  deviceId: string;
  deviceName: string;
  recipients: string[];
  emailStatus: EmailStatus;
  alertId: string;
  sampleCount: number;
  periodFrom: string;
  periodTo: string;
}

export interface HealthResponse {
  ok: boolean;
  name: string;
  version: string;
  time: string;
  smtpConfigured: boolean;
}

export interface SnapshotPayload {
  devices: Device[];
  settings: Settings;
  serverTime: string;
  serverUtcOffsetSecs?: number;
  signaling: Record<string, boolean>;
}

export type LiveMessage =
  | { type: "snapshot"; data: SnapshotPayload }
  | { type: "device"; data: Device }
  | { type: "alert"; data: Alert }
  | { type: "settings"; data: Settings }
  | { type: "signaling"; data: { role?: string; deviceId?: string; connected: boolean } }
  | { type: "devices"; data: Device[] }
  | { type: "call-record"; data: CallRecord }
  | { type: "live"; data: LiveData }
  | { type: "signal"; callId?: string; from?: string; mode?: string; deviceId?: string; payload?: SignalPayload }
  | { type: "hangup"; callId?: string; from?: string; mode?: string; deviceId?: string }
  | { type: "signal-error"; callId?: string; deviceId?: string; message?: string }
  | { type: "command"; action?: string; enabled?: boolean }
  | { type: "ready"; deviceId?: string };

export interface LiveData {
  deviceId: string;
  currentDb: number | null;
  averageDb: number | null;
  rolling15sDb?: number | null;
  peakDb: number | null;
  capturing: boolean;
  suspended: boolean;
  systemPlaying: boolean;
  aecEnabled: boolean;
  recordedAt: string;
  device: Device;
}

export interface CallRecord {
  id: string;
  deviceId: string;
  mode: "intercom" | "broadcast";
  startedAt: string;
  endedAt: string;
  durationSecs: number;
  status: "completed" | "missed" | "rejected" | "failed";
  hasAudio: boolean;
  createdAt: string;
}

export interface CreatedDevice {
  device: Device;
  token: string;
}

export interface DeviceTokenResponse {
  deviceId: string;
  token: string;
}

export interface CreateCallPayload {
  deviceId: string;
  mode: "intercom" | "broadcast";
  startedAt: string;
  endedAt: string;
  status?: "completed" | "missed" | "rejected" | "failed";
  audioBase64?: string;
  audioMime?: string;
}

export interface SignalPayload {
  kind?: "offer" | "answer" | "ice" | "reject" | string;
  sdp?: string;
  candidate?: RTCIceCandidateInit;
  reason?: string;
}

export type IntercomMode = "intercom" | "broadcast";


export interface CreateUserPayload {
  username: string;
  email: string;
  password: string;
  role: UserRole;
  deviceIds: string[];
}

export interface UpdateUserPayload {
  username?: string;
  email?: string;
  password?: string;
  role?: UserRole;
  deviceIds?: string[];
  disabled?: boolean;
}

export interface UpdateCheck {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  releaseUrl: string;
  releaseNotes: string;
  publishedAt: string;
  downloadUrls: {
    linuxX86_64: string;
    windowsServer: string;
  };
}
