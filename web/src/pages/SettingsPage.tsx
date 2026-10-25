import { useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  AudioWaveform,
  CloudCog,
  Download,
  BellRing,
  CheckCircle2,
  Clock3,
  Database,
  KeyRound,
  LoaderCircle,
  Mail,
  Megaphone,
  Mic2,
  RotateCcw,
  RefreshCw,
  Save,
  Send,
  ShieldAlert,
  ShieldCheck,
  SlidersHorizontal,
  Volume2,
} from "lucide-react";
import { api } from "../api";
import {
  emailStatusMeta,
  formatDb,
  formatFullDateTime,
} from "../lib/format";
import type { DeviceSettings, EmailStatus, Overview, Settings, UpdateCheck } from "../types";

interface SettingsPageProps {
  overview: Overview;
  deviceId: string;
  isSuper: boolean;
  canConfigure: boolean;
  onSave: (settings: Settings) => Promise<Settings>;
  onNotify: (message: string, tone?: "success" | "error" | "info") => void;
}

type PolicyFollow = {
  thresholdDb: boolean;
  alertCooldownSecs: boolean;
  offlineTimeoutSecs: boolean;
  autoRecordEvidence: boolean;
  capture: boolean;
};

type PolicyValues = {
  thresholdDb: number;
  alertCooldownSecs: number;
  offlineTimeoutSecs: number;
  autoRecordEvidence: boolean;
  captureStart: string;
  captureEnd: string;
};

function policyFromDevice(
  base: Settings,
  deviceSettings: DeviceSettings | undefined,
): { follow: PolicyFollow; values: PolicyValues } {
  const scheduleOverride =
    deviceSettings?.captureStart != null || deviceSettings?.captureEnd != null;
  return {
    follow: {
      thresholdDb: deviceSettings?.thresholdDb == null,
      alertCooldownSecs: deviceSettings?.alertCooldownSecs == null,
      offlineTimeoutSecs: deviceSettings?.offlineTimeoutSecs == null,
      autoRecordEvidence: deviceSettings?.autoRecordEvidence == null,
      capture: !scheduleOverride,
    },
    values: {
      thresholdDb: deviceSettings?.thresholdDb ?? base.thresholdDb,
      alertCooldownSecs: deviceSettings?.alertCooldownSecs ?? base.alertCooldownSecs,
      offlineTimeoutSecs: deviceSettings?.offlineTimeoutSecs ?? base.offlineTimeoutSecs,
      autoRecordEvidence: deviceSettings?.autoRecordEvidence ?? base.autoRecordEvidence,
      captureStart: deviceSettings?.captureStart ?? base.captureStart,
      captureEnd: deviceSettings?.captureEnd ?? base.captureEnd,
    },
  };
}

function splitEmails(raw: string): { valid: string[]; invalid: string[] } {
  const parts = raw
    .split(/[\s,;，；]+/)
    .map((part) => part.trim())
    .filter(Boolean);
  const valid: string[] = [];
  const invalid: string[] = [];
  for (const part of parts) {
    const ok = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(part);
    if (ok) valid.push(part);
    else invalid.push(part);
  }
  return { valid: [...new Set(valid)], invalid };
}

export function SettingsPage({ overview, deviceId, isSuper, canConfigure, onSave, onNotify }: SettingsPageProps) {
  const [baseline, setBaseline] = useState<Settings>(overview.settings);
  const [form, setForm] = useState<Settings>(overview.settings);
  const [policyDeviceId, setPolicyDeviceId] = useState<string>(deviceId || overview.device.id);
  const [policy, setPolicy] = useState(() =>
    policyFromDevice(overview.settings, overview.devices.find((d) => d.id === (deviceId || overview.device.id))?.settings),
  );
  const [policySaving, setPolicySaving] = useState(false);
  const policyEditing=useRef(false);
  const policyBound=useRef(policyDeviceId);
  const [nvidiaConfigured, setNvidiaConfigured] = useState<boolean | null>(null);
  const [nvidiaMasked, setNvidiaMasked] = useState("");
  const [nvidiaInput, setNvidiaInput] = useState("");
  const [nvidiaSaving, setNvidiaSaving] = useState(false);
  const [updateCheck, setUpdateCheck] = useState<UpdateCheck | null>(null);
  const [updateLoading, setUpdateLoading] = useState(false);
  const [updateApplying, setUpdateApplying] = useState(false);
  const [updateMessage, setUpdateMessage] = useState("");
  const [fixedPasswordInput, setFixedPasswordInput] = useState("");
  const [fixedPasswordSaving, setFixedPasswordSaving] = useState(false);
  const [alertEmails, setAlertEmails] = useState(overview.settings.alertRecipients.join(", "));
  const [reportEmails, setReportEmails] = useState(overview.settings.reportRecipients.join(", "));
  const [saving, setSaving] = useState(false);
  const [testingAlert, setTestingAlert] = useState(false);
  const [sendingReport, setSendingReport] = useState(false);
  const [formError, setFormError] = useState("");

  useEffect(() => {
    setPolicyDeviceId((current) =>
      overview.devices.some((d) => d.id === current) ? current : overview.devices[0]?.id ?? "",
    );
  }, [overview.devices]);

  const policySource = JSON.stringify(overview.devices.find(d => d.id === policyDeviceId)?.settings ?? {});
  useEffect(() => {
    if (!policyDeviceId) return;
    const device = overview.devices.find((d) => d.id === policyDeviceId);
    if (!device) return;
    if (policyBound.current !== policyDeviceId) {
      policyBound.current=policyDeviceId;
      policyEditing.current=false;
    }
    if (!policyEditing.current) setPolicy(policyFromDevice(baseline, device.settings));
  }, [policyDeviceId, policySource, baseline]);

  useEffect(() => {
    if (!isSuper) return;
    api
      .nvidiaKey()
      .then((state) => {
        setNvidiaConfigured(state.configured);
        setNvidiaMasked(state.masked);
      })
      .catch(() => undefined);
  }, [isSuper]);

  useEffect(() => {
    if (!canConfigure) return;
    let cancelled = false;
    api
      .settings()
      .then((result) => {
        if (cancelled) return;
        setBaseline(result.settings);
        setForm(result.settings);
        setAlertEmails(result.settings.alertRecipients.join(", "));
        setReportEmails(result.settings.reportRecipients.join(", "));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [canConfigure]);

  const alertParsed = useMemo(() => splitEmails(alertEmails), [alertEmails]);
  const reportParsed = useMemo(() => splitEmails(reportEmails), [reportEmails]);
  const dirty = useMemo(() => {
    const next = {
      ...form,
      alertRecipients: alertParsed.valid,
      reportRecipients: reportParsed.valid,
    };
    return JSON.stringify(next) !== JSON.stringify(baseline);
  }, [form, alertParsed.valid, reportParsed.valid, baseline]);

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setForm((previous) => ({ ...previous, [key]: value }));
  }

  function reset() {
    setForm(baseline);
    setAlertEmails(baseline.alertRecipients.join(", "));
    setReportEmails(baseline.reportRecipients.join(", "));
    setFormError("");
  }

  async function save() {
    if (saving || !canConfigure) return;
    if (alertParsed.invalid.length > 0 || reportParsed.invalid.length > 0) {
      setFormError(
        `邮箱格式不正确：${[...alertParsed.invalid, ...reportParsed.invalid].join("、")}`,
      );
      return;
    }
    setSaving(true);
    setFormError("");
    try {
      const saved = await onSave({
        ...form,
        alertRecipients: alertParsed.valid,
        reportRecipients: reportParsed.valid,
      });
      setBaseline(saved);
      setForm(saved);
      setAlertEmails(saved.alertRecipients.join(", "));
      setReportEmails(saved.reportRecipients.join(", "));
    } catch (error) {
      const message = error instanceof Error ? error.message : "保存失败";
      setFormError(message);
      onNotify(message, "error");
    } finally {
      setSaving(false);
    }
  }

  function updatePolicy<K extends keyof PolicyValues>(key: K, value: PolicyValues[K]) {
    policyEditing.current=true;
    setPolicy((previous) => ({ ...previous, values: { ...previous.values, [key]: value } }));
  }

  function toggleFollow(key: keyof PolicyFollow, follow: boolean) {
    policyEditing.current=true;
    setPolicy((previous) => ({ ...previous, follow: { ...previous.follow, [key]: follow } }));
  }

  function resetPolicy() {
    policyEditing.current=false;
    const device = overview.devices.find((d) => d.id === policyDeviceId);
    if (device) setPolicy(policyFromDevice(baseline, device.settings));
  }

  async function savePolicy() {
    if (policySaving || !policyDeviceId) return;
    setPolicySaving(true);
    try {
      const updated = await api.updateDeviceSettings(policyDeviceId, {
        thresholdDb: policy.follow.thresholdDb ? null : policy.values.thresholdDb,
        alertCooldownSecs: policy.follow.alertCooldownSecs ? null : policy.values.alertCooldownSecs,
        offlineTimeoutSecs: policy.follow.offlineTimeoutSecs ? null : policy.values.offlineTimeoutSecs,
        autoRecordEvidence: policy.follow.autoRecordEvidence ? null : policy.values.autoRecordEvidence,
        captureStart: policy.follow.capture ? null : policy.values.captureStart,
        captureEnd: policy.follow.capture ? null : policy.values.captureEnd,
      });
      if (policyBound.current === updated.id) {
        policyEditing.current=false;
        setPolicy(policyFromDevice(baseline,updated.settings));
      }
      onNotify(`设备策略已保存：${updated.name}`, "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "设备策略保存失败", "error");
    } finally {
      setPolicySaving(false);
    }
  }

  async function checkForUpdate() {
    if (updateLoading) return;
    setUpdateLoading(true);
    setUpdateMessage("");
    try {
      const result = await api.updateCheck();
      setUpdateCheck(result);
      if (!result.updateAvailable) {
        setUpdateMessage(`当前已是最新版本 v${result.currentVersion}`);
      }
    } catch (error) {
      setUpdateMessage(error instanceof Error ? error.message : "检查更新失败");
    } finally {
      setUpdateLoading(false);
    }
  }

  async function applyServerUpdate() {
    if (updateApplying || !updateCheck) return;
    setUpdateApplying(true);
    setUpdateMessage("正在下载并应用更新…");
    try {
      const result = await api.applyUpdate();
      if (result.ok) {
        setUpdateMessage(`已更新到 v${result.newVersion}，服务端正在重启，请稍后刷新页面`);
      } else {
        setUpdateMessage(result.message || "当前已是最新版本");
      }
    } catch (error) {
      setUpdateMessage(error instanceof Error ? error.message : "更新失败");
    } finally {
      setUpdateApplying(false);
    }
  }

  async function saveFixedPassword() {
    if (fixedPasswordSaving || !isSuper) return;
    setFixedPasswordSaving(true);
    try {
      const saved = (await api.setFixedPassword(fixedPasswordInput)).settings;
      setBaseline(saved);
      setForm(previous => ({ ...previous, fixedPasswordHash: saved.fixedPasswordHash }));
      setFixedPasswordInput("");
      onNotify("固定密码已保存", "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "保存失败", "error");
    } finally {
      setFixedPasswordSaving(false);
    }
  }

  async function saveNvidiaKey() {
    if (nvidiaSaving) return;
    setNvidiaSaving(true);
    try {
      const state = await api.setNvidiaKey(nvidiaInput.trim());
      setNvidiaConfigured(state.configured);
      setNvidiaMasked(state.masked);
      setNvidiaInput("");
      onNotify(state.configured ? "NVIDIA API Key 已保存" : "NVIDIA API Key 已清除", "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "保存失败", "error");
    } finally {
      setNvidiaSaving(false);
    }
  }

  async function testAlert() {
    if (testingAlert) return;
    setTestingAlert(true);
    try {
      const alert = await api.testAlert(deviceId);
      const mail = emailStatusMeta(alert.emailStatus);
      onNotify(`测试告警已生成 · ${mail.label}`, "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "测试告警失败", "error");
    } finally {
      setTestingAlert(false);
    }
  }

  async function sendReport() {
    if (sendingReport) return;
    setSendingReport(true);
    try {
      const result = await api.sendReport(deviceId);
      const mail = emailStatusMeta(result.emailStatus as EmailStatus);
      onNotify(`日报已生成（${result.sampleCount} 个样本）· ${mail.label}`, "success");
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "日报发送失败", "error");
    } finally {
      setSendingReport(false);
    }
  }

  const policyDevice = overview.devices.find((d) => d.id === policyDeviceId);

  return (
    <div className="settings-page">
      <div className="settings-topline">
        <div>
          <h2>告警与运行策略</h2>
          <p>
            {canConfigure
              ? "先调整全局默认，再选择具体节点覆盖独立策略；修改会在下一次客户端心跳时同步。"
              : "选择你负责的节点，调整该设备的独立策略；全局默认由超级管理员维护。"}
          </p>
        </div>
        <div className="settings-top-actions">
          {canConfigure && dirty && <span className="unsaved-chip">有未保存的更改</span>}
          {canConfigure && (
            <button type="button" className="ghost-button" onClick={reset} disabled={saving}>
              <RotateCcw size={15} />
              重置
            </button>
          )}
          {canConfigure && (
            <button type="button" className="primary-button" onClick={() => void save()} disabled={saving}>
              {saving ? <LoaderCircle size={15} className="spin" /> : <Save size={15} />}
              {saving ? "正在保存" : "保存全局默认"}
            </button>
          )}
        </div>
      </div>

      <nav className="settings-section-nav" aria-label="设置分区">
        <a href="#device-policy">设备策略</a>
        {canConfigure && <a href="#war-policy">分贝战争</a>}
        {isSuper && <a href="#schedule-policy">计划任务</a>}
        {isSuper && <a href="#password-policy">操作保护</a>}
        <span>{canConfigure ? "全局策略对所有客户端生效" : "仅可修改已分配设备的独立策略"}</span>
      </nav>
      {formError && (
        <div className="settings-error">
          <AlertTriangle size={16} />
          {formError}
        </div>
      )}

      <div className="settings-grid">
        <div className="settings-main">
          <section id="device-policy" className="panel settings-card">
<fieldset className="policy-fieldset" disabled={policySaving}>
            <div className="settings-card-head">
              <div className="settings-icon cyan"><SlidersHorizontal size={18} /></div>
              <div>
                <h3>设备独立策略</h3>
                <p>选择具体节点后调整；勾选“跟随全局”的项目将使用下方全局默认值。</p>
              </div>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>选择节点</span>
                <select
                  value={policyDeviceId}
                  onChange={(event) => setPolicyDeviceId(event.target.value)}
                >
                  {overview.devices.map((device) => (
                    <option key={device.id} value={device.id}>
                      {device.name}{device.online ? "" : "（离线）"}
                    </option>
                  ))}
                </select>
                <small>仅显示你有权限操作的设备</small>
              </label>
            </div>
            {policyDevice && (
              <>
                <div className="threshold-editor">
                  <div className="threshold-value">
                    <strong>{policy.values.thresholdDb.toFixed(1)}</strong>
                    <span>dB(A)</span>
                  </div>
                  <div className="threshold-slider">
                    <input
                      type="range"
                      min={30}
                      max={110}
                      step={0.5}
                      value={policy.values.thresholdDb}
                      disabled={policy.follow.thresholdDb}
                      onChange={(event) => updatePolicy("thresholdDb", Number(event.target.value))}
                      aria-label="该设备分贝阈值"
                    />
                    <div className="threshold-scale">
                      <span>30 · 安静</span>
                      <span>65 · 交谈</span>
                      <span>110 · 嘈杂</span>
                    </div>
                  </div>
                </div>
                <label className="check-row">
                  <input
                    type="checkbox"
                    checked={policy.follow.thresholdDb}
                    onChange={(event) => toggleFollow("thresholdDb", event.target.checked)}
                  />
                  <span>阈值跟随全局默认</span>
                </label>
                {canConfigure && (
                  <div className="device-schedule-block">
                    <div className="row-between">
                      <h4>计划任务</h4>
                      <label className="check-row">
                        <input
                          type="checkbox"
                          checked={policy.follow.capture}
                          onChange={(event) => toggleFollow("capture", event.target.checked)}
                        />
                        <span>跟随全局默认</span>
                      </label>
                    </div>
                    <div className="field-grid">
                      <label className="form-field">
                        <span>自动开启</span>
                        <input
                          type="time"
                          value={policy.values.captureStart}
                          disabled={policy.follow.capture}
                          onChange={(event) => updatePolicy("captureStart", event.target.value)}
                        />
                      </label>
                      <label className="form-field">
                        <span>自动关闭</span>
                        <input
                          type="time"
                          value={policy.values.captureEnd}
                          disabled={policy.follow.capture}
                          onChange={(event) => updatePolicy("captureEnd", event.target.value)}
                        />
                      </label>
                    </div>
                    <div className="inline-note">
                      可只填一个：仅自动开启=从该时间起保持采集；仅自动关闭=每日 00:00 起采集，到该时间停止。
                    </div>
                  </div>
                )}
                <div className="field-grid three">
                  <label className="form-field">
                    <span>告警冷却</span>
                    <select
                      value={policy.values.alertCooldownSecs}
                      disabled={policy.follow.alertCooldownSecs}
                      onChange={(event) => updatePolicy("alertCooldownSecs", Number(event.target.value))}
                    >
                      <option value={60}>1 分钟</option>
                      <option value={300}>5 分钟</option>
                      <option value={600}>10 分钟</option>
                      <option value={1800}>30 分钟</option>
                    </select>
                    <label className="check-row">
                      <input
                        type="checkbox"
                        checked={policy.follow.alertCooldownSecs}
                        onChange={(event) => toggleFollow("alertCooldownSecs", event.target.checked)}
                      />
                      <span>跟随全局默认</span>
                    </label>
                  </label>
                  <label className="form-field">
                    <span>离线判定</span>
                    <select
                      value={policy.values.offlineTimeoutSecs}
                      disabled={policy.follow.offlineTimeoutSecs}
                      onChange={(event) => updatePolicy("offlineTimeoutSecs", Number(event.target.value))}
                    >
                      <option value={30}>30 秒</option>
                      <option value={45}>45 秒</option>
                      <option value={60}>60 秒</option>
                      <option value={120}>2 分钟</option>
                      <option value={300}>5 分钟</option>
                    </select>
                    <label className="check-row">
                      <input
                        type="checkbox"
                        checked={policy.follow.offlineTimeoutSecs}
                        onChange={(event) => toggleFollow("offlineTimeoutSecs", event.target.checked)}
                      />
                      <span>跟随全局默认</span>
                    </label>
                  </label>
                  <label className="form-field">
                    <span>超限证据录音</span>
                    <div className="static-value">
                      {policy.values.autoRecordEvidence ? (
                        <><CheckCircle2 size={14} /> 开启</>
                      ) : (
                        <><ShieldAlert size={14} /> 关闭</>
                      )}
                    </div>
                    <label className="check-row">
                      <input
                        type="checkbox"
                        checked={policy.follow.autoRecordEvidence}
                        onChange={(event) => toggleFollow("autoRecordEvidence", event.target.checked)}
                      />
                      <span>跟随全局默认</span>
                    </label>
                  </label>
                </div>
                {!policy.follow.autoRecordEvidence && (
                  <label className="check-row policy-switch-row">
                    <input
                      type="checkbox"
                      checked={policy.values.autoRecordEvidence}
                      onChange={(event) => updatePolicy("autoRecordEvidence", event.target.checked)}
                    />
                    <span>为该设备{policy.values.autoRecordEvidence ? "开启" : "关闭"}超限录音</span>
                  </label>
                )}
                <div className="mail-actions">
                  <button
                    type="button"
                    className="ghost-button"
                    onClick={resetPolicy}
                    disabled={policySaving}
                  >
                    <RotateCcw size={14} />
                    还原
                  </button>
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => void savePolicy()}
                    disabled={policySaving}
                  >
                    {policySaving ? <LoaderCircle size={15} className="spin" /> : <Save size={15} />}
                    {policySaving ? "正在保存" : "保存该设备策略"}
                  </button>
                </div>
                <div className="inline-note">
                  <Clock3 size={13} />
                  独立策略仅对所选设备生效，并在下一次客户端心跳时同步；设备接入与令牌由超级管理员在“设备连接”中管理。
                </div>
              </>
            )}
          </fieldset></section>

          <section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon cyan"><SlidersHorizontal size={18} /></div>
              <div>
                <h3>{canConfigure ? "全局默认 · 分贝告警阈值" : "分贝告警阈值（全局默认，只读）"}</h3>
                <p>客户端每 30 秒计算一次平均分贝，达到阈值即生成告警并发送邮件。</p>
              </div>
            </div>
            {canConfigure ? (
              <div className="threshold-editor">
                <div className="threshold-value">
                  <strong>{form.thresholdDb.toFixed(1)}</strong>
                  <span>dB(A)</span>
                </div>
                <div className="threshold-slider">
                  <input
                    type="range"
                    min={30}
                    max={110}
                    step={0.5}
                    value={form.thresholdDb}
                    onChange={(event) => update("thresholdDb", Number(event.target.value))}
                    aria-label="分贝阈值"
                  />
                  <div className="threshold-scale">
                    <span>30 · 安静</span>
                    <span>65 · 交谈</span>
                    <span>110 · 嘈杂</span>
                  </div>
                </div>
              </div>
            ) : (
              <div className="global-summary">
                <div>
                  <span>阈值</span>
                  <strong>{baseline.thresholdDb.toFixed(1)} dB</strong>
                </div>
                <div>
                  <span>冷却</span>
                  <strong>{baseline.alertCooldownSecs >= 60 ? `${Math.round(baseline.alertCooldownSecs / 60)} 分钟` : `${baseline.alertCooldownSecs} 秒`}</strong>
                </div>
                <div>
                  <span>离线判定</span>
                  <strong>{baseline.offlineTimeoutSecs >= 60 ? `${Math.round(baseline.offlineTimeoutSecs / 60)} 分钟` : `${baseline.offlineTimeoutSecs} 秒`}</strong>
                </div>
                <div>
                  <span>超限录音</span>
                  <strong>{baseline.autoRecordEvidence ? "开启" : "关闭"}</strong>
                </div>
              </div>
            )}
            <div className="threshold-hint">
              <BellRing size={14} />
              建议室内会议场景设置在 60–70 dB。超限事件会按冷却时间合并发送邮件，避免连续轰炸。
            </div>
          </section>

          {canConfigure && (<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon cyan"><AudioWaveform size={18} /></div>
              <div>
                <h3>AI 降噪与人声增强</h3>
                <p>基于 RNNoise 神经网络：常驻背景噪声（如 60 dB 环境嗡声）被持续抑制，突发人声和异常事件保持突出。开启后分贝值反映降噪后的信号，请重新校准阈值。</p>
              </div>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>降噪模式</span>
                <select
                  value={form.denoiseMode}
                  onChange={(event) => update("denoiseMode", event.target.value)}
                >
                  <option value="off">关闭 · 原始声压测量</option>
                  <option value="light">轻度 · 保留更多环境细节</option>
                  <option value="standard">标准 · 推荐常驻噪声场所</option>
                  <option value="strong">强化 · 噪声门限随语音活动加深</option>
                </select>
                <small>所有采集端在下一次心跳时自动应用（约 10 秒）。</small>
              </label>
              <label className="form-field">
                <span>人声增强</span>
                <div className="static-value">
                  {form.voiceEnhance ? (
                    <><CheckCircle2 size={14} /> 语音临场感提升已开启</>
                  ) : (
                    <>关闭</>
                  )}
                </div>
                <small>降噪后对 2–5 kHz 语音频段做临场感提升，证据回放更清晰。</small>
              </label>
            </div>
            <label className="check-row policy-switch-row">
              <input
                type="checkbox"
                checked={form.voiceEnhance}
                onChange={(event) => update("voiceEnhance", event.target.checked)}
              />
              <span>为该系统的证据录音与分贝测量启用人声增强</span>
            </label>
            <div className="inline-note">
              <Volume2 size={13} />
              降噪在采集端本地完成，不改变证据的存储与加密方式；切换模式会重新开始 30 秒测量窗口。
            </div>
          </section>)}

          {canConfigure && (
<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon amber"><Mail size={18} /></div>
              <div>
                <h3>邮件通知</h3>
                <p>
                  {overview.smtpConfigured
                    ? "SMTP 已配置，告警与日报将投递到下列收件人。"
                    : "当前未配置 SMTP，邮件只会写入本地 data/outbox 预览文件，不会发送到邮箱。"}
                </p>
              </div>
              <span className={`smtp-pill ${overview.smtpConfigured ? "is-on" : ""}`}>
                {overview.smtpConfigured ? "SMTP 在线" : "SMTP 未配置"}
              </span>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>告警收件人 <em>噪音超限、离线与安全事件</em></span>
                <textarea
                  rows={3}
                  value={alertEmails}
                  onChange={(event) => setAlertEmails(event.target.value)}
                  placeholder="admin@example.com, security@example.com"
                />
                <small>
                  {alertParsed.valid.length} 个有效地址
                  {alertParsed.invalid.length > 0 && ` · 无效：${alertParsed.invalid.join("、")}`}
                </small>
              </label>
              <label className="form-field">
                <span>日报收件人 <em>按设备发送统计图与 CSV</em></span>
                <textarea
                  rows={3}
                  value={reportEmails}
                  onChange={(event) => setReportEmails(event.target.value)}
                  placeholder="manager@example.com"
                />
                <small>
                  {reportParsed.valid.length} 个有效地址
                  {reportParsed.invalid.length > 0 && ` · 无效：${reportParsed.invalid.join("、")}`}
                </small>
              </label>
            </div>
            <div className="mail-actions">
              <button
                type="button"
                className="secondary-button small"
                onClick={() => void testAlert()}
                disabled={testingAlert}
              >
                {testingAlert ? <LoaderCircle size={14} className="spin" /> : <BellRing size={14} />}
                发送测试告警
              </button>
              <button
                type="button"
                className="secondary-button small"
                onClick={() => void sendReport()}
                disabled={sendingReport}
              >
                {sendingReport ? <LoaderCircle size={14} className="spin" /> : <Send size={14} />}
                立即生成日报
              </button>
              <span className="smtp-note">
                发件服务器通过 <code>.env</code> 配置，不在网页中保存密码。
              </span>
            </div>
          </section>
)}

          {canConfigure && (
<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon amber"><Megaphone size={18} /></div>
              <div>
                <h3>客户端通知</h3>
                <p>通知会显示在所有采集端通话记录上方，可用于发布考场纪律、设备维护等提示。</p>
              </div>
              <button
                type="button"
                className={`switch ${form.notificationEnabled ? "is-on" : ""}`}
                role="switch"
                aria-checked={form.notificationEnabled}
                aria-label="启用客户端通知"
                onClick={() => update("notificationEnabled", !form.notificationEnabled)}
              >
                <span />
              </button>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>通知标题</span>
                <input
                  value={form.notificationTitle}
                  onChange={(event) => update("notificationTitle", event.target.value)}
                  placeholder="例如：晚自习安静提示"
                  maxLength={80}
                />
              </label>
              <label className="form-field">
                <span>通知内容</span>
                <textarea
                  value={form.notificationContent}
                  onChange={(event) => update("notificationContent", event.target.value)}
                  placeholder="填写需要展示给现场的通知内容…"
                  rows={3}
                  maxLength={2000}
                />
              </label>
            </div>
            <div className="inline-note">
              通知内容由服务端保存，所有设备在下一次心跳时同步。关闭开关后客户端显示“暂无通知”。
            </div>
          </section>
)}

          {canConfigure && (
<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon emerald"><Send size={18} /></div>
              <div>
                <h3>每日数据日报</h3>
                <p>定时汇总最近 24 小时的声学统计，附带 CSV 原始数据与 SVG 趋势图。</p>
              </div>
              <button
                type="button"
                className={`switch ${form.dailyReportEnabled ? "is-on" : ""}`}
                role="switch"
                aria-checked={form.dailyReportEnabled}
                aria-label="启用日报"
                onClick={() => update("dailyReportEnabled", !form.dailyReportEnabled)}
              >
                <span />
              </button>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>发送时间</span>
                <input
                  type="time"
                  value={form.dailyReportTime}
                  onChange={(event) => update("dailyReportTime", event.target.value)}
                  disabled={!form.dailyReportEnabled}
                />
              </label>
              <div className="form-field static-field">
                <span>报表内容</span>
                <div className="static-value">
                  <Database size={14} />
                  24h 数据 + 折线图 + CSV
                </div>
              </div>
            </div>
            {!form.dailyReportEnabled && (
              <div className="inline-note">日报已关闭，仍可通过上方“立即生成日报”手动发送。</div>
            )}
          </section>
)}

          {canConfigure && (
<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon rose"><ShieldCheck size={18} /></div>
              <div>
                <h3>离线检测与告警冷却</h3>
                <p>客户端异常关闭、断电或断网超过阈值时，服务端将发送离线告警。</p>
              </div>
            </div>
            <div className="field-grid three">
              <label className="form-field">
                <span>离线判定</span>
                <select
                  value={form.offlineTimeoutSecs}
                  onChange={(event) => update("offlineTimeoutSecs", Number(event.target.value))}
                >
                  <option value={30}>30 秒</option>
                  <option value={45}>45 秒</option>
                  <option value={60}>60 秒</option>
                  <option value={120}>2 分钟</option>
                  <option value={300}>5 分钟</option>
                </select>
                <small>客户端每 10 秒发送一次心跳</small>
              </label>
              <label className="form-field">
                <span>告警冷却</span>
                <select
                  value={form.alertCooldownSecs}
                  onChange={(event) => update("alertCooldownSecs", Number(event.target.value))}
                >
                  <option value={60}>1 分钟</option>
                  <option value={300}>5 分钟</option>
                  <option value={600}>10 分钟</option>
                  <option value={1800}>30 分钟</option>
                </select>
                <small>持续超限时避免邮件重复轰炸</small>
              </label>
              <label className="form-field">
                <span>数据保留</span>
                <select
                  value={form.retentionDays}
                  disabled={!isSuper}
                  onChange={(event) => update("retentionDays", Number(event.target.value))}
                >
                  <option value={7}>7 天</option>
                  <option value={30}>30 天</option>
                  <option value={90}>90 天</option>
                  <option value={180}>180 天</option>
                  <option value={365}>365 天</option>
                </select>
                <small>过期遥测、告警和音频自动清理；仅超级管理员可修改</small>
              </label>
            </div>
            <div className="inline-note">
              <Clock3 size={13} />
              服务端按 10 秒周期检查设备在线状态，超时只发送一次离线告警，恢复后自动记录恢复事件。
            </div>
          </section>
)}

                    {isSuper && (<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon rose"><CloudCog size={18} /></div>
              <div>
                <h3>NVIDIA 云降噪</h3>
                <p>配置 build.nvidia.com 的 API Key 后，告警中心回放超限证据时可一键调用 NVIDIA BNR 云端降噪。音频将上传至 NVIDIA 处理，请确认现场告知与合规要求。</p>
              </div>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>API Key {nvidiaConfigured === false ? "（未配置）" : nvidiaConfigured ? `（已配置 ${nvidiaMasked}）` : ""}</span>
                <input
                  type="password"
                  value={nvidiaInput}
                  onChange={(event) => setNvidiaInput(event.target.value)}
                  placeholder={nvidiaConfigured ? "输入新 Key 以更换，留空保持不变" : "nvapi-…"}
                  autoComplete="off"
                />
                <small>仅超级管理员可见，保存后以掩码显示；也可用环境变量 NVIDIA_API_KEY 配置。</small>
              </label>
            </div>
            <div className="mail-actions">
              <button
                type="button"
                className="primary-button"
                onClick={() => void saveNvidiaKey()}
                disabled={nvidiaSaving}
              >
                {nvidiaSaving ? <LoaderCircle size={15} className="spin" /> : <KeyRound size={15} />}
                保存 API Key
              </button>
            </div>
            <div className="inline-note">
              云端调用失败会自动重试 3 次；降噪结果缓存在本地服务端（加密存储），重复试听不再计费。
            </div>
          </section>)}

          {canConfigure && (<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon emerald"><RefreshCw size={18} /></div>
              <div>
                <h3>软件更新</h3>
                <p>从 GitHub Release 检查并安装服务端更新。更新完成后服务端会自动重启，管理端请刷新页面。</p>
              </div>
            </div>
            {updateCheck && (
              <div className="field-grid">
                <div className="form-field static-field">
                  <span>当前版本</span>
                  <div className="static-value"><code>v{updateCheck.currentVersion}</code></div>
                </div>
                <div className="form-field static-field">
                  <span>最新版本</span>
                  <div className="static-value">
                    <code>v{updateCheck.latestVersion}</code>
                    {updateCheck.updateAvailable && <span className="badge warn">有更新</span>}
                  </div>
                </div>
              </div>
            )}
            {updateMessage && <div className="inline-note">{updateMessage}</div>}
            <div className="mail-actions">
              <button type="button" className="ghost-button" onClick={() => void checkForUpdate()} disabled={updateLoading}>
                {updateLoading ? <LoaderCircle size={14} className="spin" /> : <RefreshCw size={14} />}
                检查更新
              </button>
              {isSuper && updateCheck?.updateAvailable && (
                <button type="button" className="primary-button" onClick={() => void applyServerUpdate()} disabled={updateApplying}>
                  {updateApplying ? <LoaderCircle size={14} className="spin" /> : <Download size={14} />}
                  {updateApplying ? "正在更新…" : "一键更新"}
                </button>
              )}
              {updateCheck?.updateAvailable && updateCheck.releaseUrl && (
                <a href={updateCheck.releaseUrl} target="_blank" rel="noopener noreferrer" className="ghost-button small" style={{ textDecoration: "none" }}>
                  查看发布说明
                </a>
              )}
            </div>
          </section>)}

          {canConfigure && (<section id="war-policy" className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon amber"><Megaphone size={18} /></div>
              <div>
                <h3>分贝战争</h3>
                <p>早读等需要大声朗读的场景。开启后客户端切换金色主题，设备按平均分贝实时排名。</p>
              </div>
              <button
                type="button"
                className={`switch ${form.warModeEnabled ? "is-on" : ""}`}
                role="switch"
                aria-checked={form.warModeEnabled}
                aria-label="启用分贝战争"
                onClick={() => update("warModeEnabled", !form.warModeEnabled)}
              >
                <span />
              </button>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>开始时间</span>
                <input type="time" value={form.warModeStart} onChange={(e) => update("warModeStart", e.target.value)} disabled={!form.warModeEnabled} />
              </label>
              <label className="form-field">
                <span>结束时间</span>
                <input type="time" value={form.warModeEnd} onChange={(e) => update("warModeEnd", e.target.value)} disabled={!form.warModeEnabled} />
              </label>
            </div>
            <div className="inline-note">非指定时段自动恢复普通模式。由服务端统一下发，原生端和 APP 使用同一时段；不改变原始测量值。</div>
          </section>)}

          {isSuper && (<section id="schedule-policy" className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon cyan"><Clock3 size={18} /></div>
              <div>
                <h3>计划任务 · 全局默认</h3>
                <p>由超级管理员维护全局默认自动开启/关闭时间；管理员可在设备独立策略中为单个节点覆盖。</p>
              </div>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>自动开启（可留空）</span>
                <input type="time" value={form.captureStart} onChange={(e) => update("captureStart", e.target.value)} />
              </label>
              <label className="form-field">
                <span>自动关闭（可留空）</span>
                <input type="time" value={form.captureEnd} onChange={(e) => update("captureEnd", e.target.value)} />
              </label>
            </div>
            <div className="inline-note">每日重复，可跨午夜。只填自动开启：从该时间起保持采集；只填自动关闭：每日 00:00 起采集，到该时间停止；两个相同表示全天。手动暂停优先，客户端约 10 秒内同步。</div>
          </section>)}

          {isSuper && (<section id="password-policy" className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon rose"><KeyRound size={18} /></div>
              <div>
                <h3>固定操作密码</h3>
                <p>固定密码由服务端联网验证，本地密码可离线使用。此处留空保持已有密码；密码首尾空格会保留。</p>
              </div>
            </div>
            <div className="field-grid">
              <label className="form-field">
                <span>新密码</span>
                <input
                  type="password"
                  value={fixedPasswordInput}
                  onChange={(e) => setFixedPasswordInput(e.target.value)}
                  placeholder="至少 8 个字符，留空保持不变"
                  autoComplete="new-password"
                />
              </label>
            </div>
            <div className="mail-actions">
              <button type="button" className="primary-button" onClick={() => void saveFixedPassword()} disabled={fixedPasswordSaving || !fixedPasswordInput}>
                {fixedPasswordSaving ? <LoaderCircle size={14} className="spin" /> : <Save size={14} />}
                保存固定密码
              </button>
            </div>
          </section>)}

{canConfigure && (
<section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon violet"><Mic2 size={18} /></div>
              <div>
                <h3>超限证据录音</h3>
                <p>客户端持续保留最近 30 秒的音频缓冲，超限时上传完整平均区间并随邮件发送。</p>
              </div>
              <button
                type="button"
                className={`switch ${form.autoRecordEvidence ? "is-on" : ""}`}
                role="switch"
                aria-checked={form.autoRecordEvidence}
                aria-label="启用超限录音"
                onClick={() => update("autoRecordEvidence", !form.autoRecordEvidence)}
              >
                <span />
              </button>
            </div>
            <div className="evidence-setting">
              <div className="evidence-duration">
                <span>证据时长</span>
                <strong>30 秒</strong>
              </div>
              <div className="evidence-meta">
                <span className="evidence-tag">完整平均区间</span>
                <span>单声道 · 48 kHz WAV</span>
              </div>
            </div>
            <div className="inline-note">
              <Volume2 size={13} />
              音频只保留在本地服务端与告警邮件中，不经过第三方服务。请确认现场告知义务与适用法规。
            </div>
          </section>
)}
        </div>

      </div>
    </div>
  );
}
