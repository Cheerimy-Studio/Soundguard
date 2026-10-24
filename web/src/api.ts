import type {
  NvidiaKeyState,
  Alert,
  CreateUserPayload,
  CallRecord,
  CreateCallPayload,
  CreatedDevice,
  Device,
  DeviceControlResponse,
  DeviceTokenResponse,
  HealthResponse,
  LoginResponse,
  Overview,
  ReadingsResponse,
  ReportResult,
  Settings,
  SettingsResponse,
  UpdateUserPayload,
  User,
} from "./types";

const TOKEN_KEY = "soundguard.token";

export class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

interface RequestOptions extends RequestInit {
  auth?: boolean;
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const { auth = true, headers, ...init } = options;
  const mergedHeaders = new Headers(headers);
  if (init.body && !(init.body instanceof FormData) && !mergedHeaders.has("Content-Type")) {
    mergedHeaders.set("Content-Type", "application/json");
  }
  if (auth) {
    const token = getToken();
    if (token) {
      mergedHeaders.set("Authorization", `Bearer ${token}`);
    }
  }
  let response: Response;
  try {
    response = await fetch(`/api${path}`, { ...init, headers: mergedHeaders, signal: init.signal ?? AbortSignal.timeout(20_000) });
  } catch {
    throw new ApiError(0, "无法连接到服务器，请检查网络或服务状态");
  }

  if (!response.ok) {
    let message = `请求失败（${response.status}）`;
    try {
      const body = await response.json();
      if (body?.error) message = body.error;
    } catch {
      // Keep the status-based message when the response is not JSON.
    }
    if (response.status === 401 && auth && mergedHeaders.get("Authorization") === `Bearer ${getToken()}`) {
      clearToken();
      window.dispatchEvent(new Event("soundguard:logout"));
    }
    throw new ApiError(response.status, message);
  }

  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

async function requestBlob(path: string): Promise<Blob> {
  const token = getToken();
  let response: Response;
  try {
    response = await fetch(`/api${path}`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
      signal: AbortSignal.timeout(30_000),
    });
  } catch {
    throw new ApiError(0, "无法连接到服务器");
  }
  if (!response.ok) {
    if (response.status === 401 && token === getToken()) {
      clearToken();
      window.dispatchEvent(new Event("soundguard:logout"));
    }
    let message = `下载失败（${response.status}）`;
    try {
      const body = await response.json();
      if (body?.error) message = body.error;
    } catch {
      // Ignore.
    }
    throw new ApiError(response.status, message);
  }
  return response.blob();
}

function deviceQuery(deviceId?: string | null): string {
  return deviceId ? `deviceId=${encodeURIComponent(deviceId)}` : "";
}

export const api = {
  login(username: string, password: string): Promise<LoginResponse> {
    return request<LoginResponse>("/auth/login", {
      method: "POST",
      auth: false,
      body: JSON.stringify({ username, password, remember: false }),
    });
  },

  me(): Promise<User> {
    return request<User>("/auth/me");
  },

  users(): Promise<User[]> {
    return request<User[]>("/users");
  },

  createUser(payload: CreateUserPayload): Promise<User> {
    return request<User>("/users", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },

  updateUser(id: string, payload: UpdateUserPayload): Promise<User> {
    return request<User>(`/users/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  },

  deleteUser(id: string): Promise<{ ok: boolean }> {
    return request<{ ok: boolean }>(`/users/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  },

  updateNotifications(payload: {
    enabled: boolean;
    title: string;
    content: string;
  }): Promise<SettingsResponse> {
    return request<SettingsResponse>("/notifications", {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  },

  updateDeviceSettings(
    id: string,
    payload: {
      thresholdDb: number | null;
      alertCooldownSecs: number | null;
      offlineTimeoutSecs: number | null;
      autoRecordEvidence: boolean | null;
    },
  ): Promise<Device> {
    return request<Device>(`/devices/${encodeURIComponent(id)}/settings`, {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  },

  health(): Promise<HealthResponse> {
    return request<HealthResponse>("/health", { auth: false });
  },

  overview(deviceId?: string | null): Promise<Overview> {
    const query = deviceQuery(deviceId);
    return request<Overview>(`/overview${query ? `?${query}` : ""}`);
  },

  devices(): Promise<{ devices: Device[] }> {
    return request<{ devices: Device[] }>("/devices");
  },

  createDevice(payload: {
    name: string;
    deviceId?: string;
  }): Promise<CreatedDevice> {
    return request<CreatedDevice>("/devices", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },

  updateDevice(
    id: string,
    payload: { name: string },
  ): Promise<Device> {
    return request<Device>(`/devices/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  },

  deleteDevice(id: string): Promise<{ ok: boolean }> {
    return request<{ ok: boolean }>(`/devices/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  },

  rotateDeviceToken(id: string): Promise<DeviceTokenResponse> {
    return request<DeviceTokenResponse>(
      `/devices/${encodeURIComponent(id)}/token`,
      { method: "POST" },
    );
  },

  readings(
    from: Date,
    to: Date,
    deviceId?: string | null,
    maxPoints = 1800,
  ): Promise<ReadingsResponse> {
    const params = new URLSearchParams({
      from: from.toISOString(),
      to: to.toISOString(),
      maxPoints: String(maxPoints),
    });
    if (deviceId) params.set("deviceId", deviceId);
    return request<ReadingsResponse>(`/readings?${params.toString()}`);
  },

  alerts(deviceId?: string | null, limit = 200): Promise<Alert[]> {
    const params = new URLSearchParams({ limit: String(limit) });
    if (deviceId) params.set("deviceId", deviceId);
    return request<Alert[]>(`/alerts?${params.toString()}`);
  },

  acknowledgeAlert(id: string): Promise<Alert> {
    return request<Alert>(`/alerts/${encodeURIComponent(id)}/ack`, {
      method: "POST",
    });
  },

  evidence(id: string): Promise<Blob> {
    return requestBlob(`/alerts/${encodeURIComponent(id)}/evidence`);
  },

  async denoiseEvidence(id: string): Promise<{ ok: boolean; cached: boolean }> {
    return request(`/alerts/${encodeURIComponent(id)}/evidence/denoise`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  },

  async evidenceDenoised(id: string): Promise<Blob> {
    return requestBlob(
      `/alerts/${encodeURIComponent(id)}/evidence/denoised`,
    );
  },

  nvidiaKey(): Promise<NvidiaKeyState> {
    return request<NvidiaKeyState>("/settings/nvidia-key");
  },

  setNvidiaKey(apiKey: string): Promise<NvidiaKeyState> {
    return request<NvidiaKeyState>("/settings/nvidia-key", {
      method: "PUT",
      body: JSON.stringify({ apiKey }),
    });
  },

  exportReadings(from: Date, to: Date, deviceId?: string | null): Promise<Blob> {
    const params = new URLSearchParams({
      from: from.toISOString(),
      to: to.toISOString(),
    });
    if (deviceId) params.set("deviceId", deviceId);
    return requestBlob(`/readings/export?${params.toString()}`);
  },

  settings(): Promise<SettingsResponse> {
    return request<SettingsResponse>("/settings");
  },

  updateSettings(settings: Settings): Promise<SettingsResponse> {
    return request<SettingsResponse>("/settings", {
      method: "PUT",
      body: JSON.stringify(settings),
    });
  },

  controlDevice(monitoring: boolean, deviceId: string): Promise<DeviceControlResponse> {
    return request<DeviceControlResponse>("/device/control", {
      method: "POST",
      body: JSON.stringify({ deviceId, monitoring }),
    });
  },

  sendReport(deviceId: string): Promise<ReportResult> {
    return request<ReportResult>("/reports/send", {
      method: "POST",
      body: JSON.stringify({ deviceId }),
    });
  },

  testAlert(deviceId: string): Promise<Alert> {
    return request<Alert>("/test/alert", {
      method: "POST",
      body: JSON.stringify({ deviceId }),
    });
  },

  calls(deviceId: string, limit = 20): Promise<CallRecord[]> {
    const params = new URLSearchParams({
      deviceId,
      limit: String(limit),
    });
    return request<CallRecord[]>(`/calls?${params.toString()}`);
  },

  saveCallRecord(payload: CreateCallPayload, sessionToken = getToken()): Promise<CallRecord> {
    return request<CallRecord>("/calls", {
      auth: false,
      headers: sessionToken ? { Authorization: `Bearer ${sessionToken}` } : {},
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
};

export function adminWebSocketUrl(): string {
  const token = getToken() ?? "";
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/ws?token=${encodeURIComponent(token)}`;
}
