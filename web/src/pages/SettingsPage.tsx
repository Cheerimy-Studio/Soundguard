import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
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
import type { EmailStatus, Overview, Settings } from "../types";

interface SettingsPageProps {
  overview: Overview;
  deviceId: string;
  onSave: (settings: Settings) => Promise<Settings>;
  onNotify: (message: string, tone?: "success" | "error" | "info") => void;
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

export function SettingsPage({ overview, deviceId, onSave, onNotify }: SettingsPageProps) {
  const [baseline, setBaseline] = useState<Settings>(overview.settings);
  const [form, setForm] = useState<Settings>(overview.settings);
  const [alertEmails, setAlertEmails] = useState(overview.settings.alertRecipients.join(", "));
  const [reportEmails, setReportEmails] = useState(overview.settings.reportRecipients.join(", "));
  const [saving, setSaving] = useState(false);
  const [testingAlert, setTestingAlert] = useState(false);
  const [sendingReport, setSendingReport] = useState(false);
  const [formError, setFormError] = useState("");

  useEffect(() => {
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
  }, []);

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
    if (saving) return;
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

  return (
    <div className="settings-page">
      <div className="settings-topline">
        <div>
          <h2>告警与运行策略</h2>
          <p>设置对所有设备生效，并会在下一次客户端心跳时同步。</p>
        </div>
        <div className="settings-top-actions">
          {dirty && <span className="unsaved-chip">有未保存的更改</span>}
          <button type="button" className="ghost-button" onClick={reset} disabled={saving}>
            <RotateCcw size={15} />
            重置
          </button>
          <button type="button" className="primary-button" onClick={() => void save()} disabled={saving}>
            {saving ? <LoaderCircle size={15} className="spin" /> : <Save size={15} />}
            {saving ? "正在保存" : "保存设置"}
          </button>
        </div>
      </div>

      {formError && (
        <div className="settings-error">
          <AlertTriangle size={16} />
          {formError}
        </div>
      )}

      <div className="settings-grid">
        <div className="settings-main">
          <section className="panel settings-card">
            <div className="settings-card-head">
              <div className="settings-icon cyan"><SlidersHorizontal size={18} /></div>
              <div>
                <h3>分贝告警阈值</h3>
                <p>客户端每 30 秒计算一次平均分贝，达到阈值即生成告警并发送邮件。</p>
              </div>
            </div>
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
            <div className="threshold-hint">
              <BellRing size={14} />
              建议室内会议场景设置在 60–70 dB。超限事件会按冷却时间合并发送邮件，避免连续轰炸。
            </div>
          </section>

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
                  onChange={(event) => update("retentionDays", Number(event.target.value))}
                >
                  <option value={7}>7 天</option>
                  <option value={30}>30 天</option>
                  <option value={90}>90 天</option>
                  <option value={180}>180 天</option>
                  <option value={365}>365 天</option>
                </select>
                <small>过期 JSON 遥测文件自动清理</small>
              </label>
            </div>
            <div className="inline-note">
              <Clock3 size={13} />
              服务端按 10 秒周期检查设备在线状态，超时只发送一次离线告警，恢复后自动记录恢复事件。
            </div>
          </section>

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
              <div>
                <span>证据时长</span>
                <strong>30 秒 · 完整平均区间</strong>
              </div>
            </div>
            <div className="inline-note">
              <Volume2 size={13} />
              音频只保留在本地服务端与告警邮件中，不经过第三方服务。请确认现场告知义务与适用法规。
            </div>
          </section>
        </div>

      </div>
    </div>
  );
}
