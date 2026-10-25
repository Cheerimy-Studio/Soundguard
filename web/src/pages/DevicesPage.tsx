import { useMemo, useState } from "react";
import {
  ArrowRightLeft,
  CheckCircle2,
  Copy,
  Edit3,
  KeyRound,
  LoaderCircle,
  Plus,
  Server,
  Trash2,
  Wifi,
  WifiOff,
  X,
} from "lucide-react";
import { api } from "../api";
import { deviceStatusMeta, formatDb, formatRelative } from "../lib/format";
import type { Device } from "../types";

interface DevicesPageProps {
  devices: Device[];
  selectedDeviceId: string;
  canManage: boolean;
  onSelectDevice: (id: string) => void;
  onDevicesUpdated: () => Promise<void>;
  onNotify: (message: string, tone?: "success" | "error" | "info") => void;
}

interface AccessInfo {
  device: Device;
  token: string;
  rotated?: boolean;
}

interface EditState {
  device: Device;
  name: string;
  thresholdDb: string;
  alertCooldownSecs: string;
  offlineTimeoutSecs: string;
  autoRecordEvidence: "" | "true" | "false";
}

function optionalNumber(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : null;
}

export function DevicesPage({
  devices,
  selectedDeviceId,
  canManage,
  onSelectDevice,
  onDevicesUpdated,
  onNotify,
}: DevicesPageProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [name, setName] = useState("");
  const [deviceKey, setDeviceKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");
  const [access, setAccess] = useState<AccessInfo | null>(null);
  const [edit, setEdit] = useState<EditState | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    kind: "rotate" | "delete";
    device: Device;
  } | null>(null);
  const [copied, setCopied] = useState(false);

  const ordered = useMemo(
    () =>
      [...devices].sort((a, b) => {
        if (a.id === selectedDeviceId) return -1;
        if (b.id === selectedDeviceId) return 1;
        return Number(b.online) - Number(a.online);
      }),
    [devices, selectedDeviceId],
  );

  function openAdd() {
    setName("");
    setDeviceKey("");
    setFormError("");
    setAddOpen(true);
  }

  async function createDevice(event: React.FormEvent) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setFormError("");
    try {
      const created = await api.createDevice({
        name: name.trim(),
        deviceId: deviceKey.trim() || undefined,
      });
      setAddOpen(false);
      setAccess({ device: created.device, token: created.token });
      await onDevicesUpdated();
    } catch (error) {
      setFormError(error instanceof Error ? error.message : "添加设备失败");
    } finally {
      setBusy(false);
    }
  }

  async function saveEdit(event: React.FormEvent) {
    event.preventDefault();
    if (!edit || busy) return;
    setBusy(true);
    setFormError("");
    try {
      await api.updateDevice(edit.device.id, {
        name: edit.name.trim(),
      });
      await api.updateDeviceSettings(edit.device.id, {
        thresholdDb: optionalNumber(edit.thresholdDb),
        alertCooldownSecs: optionalNumber(edit.alertCooldownSecs),
        offlineTimeoutSecs: optionalNumber(edit.offlineTimeoutSecs),
        autoRecordEvidence:
          edit.autoRecordEvidence === ""
            ? null
            : edit.autoRecordEvidence === "true",
        captureStart: edit.device.settings.captureStart,
        captureEnd: edit.device.settings.captureEnd,
      });
      setEdit(null);
      await onDevicesUpdated();
      onNotify("设备信息与独立策略已更新", "success");
    } catch (error) {
      setFormError(error instanceof Error ? error.message : "更新设备失败");
    } finally {
      setBusy(false);
    }
  }

  async function runConfirm() {
    if (!confirmAction || busy) return;
    setBusy(true);
    try {
      if (confirmAction.kind === "rotate") {
        const result = await api.rotateDeviceToken(confirmAction.device.id);
        setAccess({
          device: confirmAction.device,
          token: result.token,
          rotated: true,
        });
        onNotify("设备令牌已更换，旧令牌立即失效", "success");
      } else {
        const deleting = confirmAction.device;
        if (devices.length <= 1) {
          throw new Error("至少需要保留一个设备");
        }
        await api.deleteDevice(deleting.id);
        const fallback = devices.find((device) => device.id !== deleting.id);
        if (deleting.id === selectedDeviceId && fallback) {
          onSelectDevice(fallback.id);
        } else {
          await onDevicesUpdated();
        }
        onNotify("设备已移除，历史数据仍保留在服务端", "success");
      }
      setConfirmAction(null);
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "操作失败", "error");
    } finally {
      setBusy(false);
    }
  }

  async function copyAccess() {
    if (!access) return;
    const server = window.location.origin;
    const text = [
      `服务器地址：${server}`,
      `设备编号：${access.device.id}`,
      `设备令牌：${access.token}`,
      "在采集端右上角设置中填写以上信息（需要关闭密码验证）。",
    ].join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      onNotify("复制失败，请手动选择文本", "error");
    }
  }

  async function copyTokenOnly() {
    if (!access) return;
    try {
      await navigator.clipboard.writeText(access.token);
      onNotify("设备令牌已复制", "success");
    } catch {
      onNotify("复制失败，请手动选择文本", "error");
    }
  }

  return (
    <div className="devices-page">
      <section className="panel devices-hero">
        <div>
          <div className="eyebrow">设备接入</div>
          <h2>设备接入管理</h2>
          <p>
            每台设备拥有唯一名称和独立令牌。添加后，在采集端设置中填写服务器地址、设备编号和令牌即可接入。
          </p>
        </div>
        {canManage ? (
          <button type="button" className="primary-button" onClick={openAdd}>
            <Plus size={16} />
            添加设备
          </button>
        ) : (
          <span className="badge idle">设备管理员</span>
        )}
      </section>

      <div className="devices-grid">
        {ordered.map((device) => {
          const status = deviceStatusMeta(device.status);
          const selected = device.id === selectedDeviceId;
          return (
            <section
              className={`panel device-access-card ${selected ? "is-selected" : ""}`}
              key={device.id}
            >
              <div className="device-access-top">
                <div className={`device-node-icon ${device.online ? "is-online" : ""}`}>
                  <Server size={21} />
                </div>
                <div className="device-access-title">
                  <div>
                    <h3>{device.name}</h3>
                    {selected && <span className="selected-tag">当前查看</span>}
                  </div>
                </div>
                <span className={`device-state-pill tone-${status.tone}`}>
                  <span className={`status-dot tone-${status.tone}`} />
                  {status.label}
                </span>
              </div>

              <div className="device-readout-row">
                <div>
                  <span>当前声压</span>
                  <strong>{formatDb(device.currentDb)} <em>dB</em></strong>
                </div>
                <div>
                  <span>运行状态</span>
                  <strong>{device.monitoring ? "正在采集" : device.desiredMonitoring ? "等待启动" : "已暂停"}</strong>
                </div>
                <div>
                  <span>连接</span>
                  <strong className={device.online ? "text-online" : "text-muted"}>
                    {device.online ? <Wifi size={13} /> : <WifiOff size={13} />}
                    {device.online ? "在线" : "离线"}
                  </strong>
                </div>
              </div>

              <dl className="device-access-facts">
                <div>
                  <dt>设备编号</dt>
                  <dd><code>{device.id}</code></dd>
                </div>
                <div>
                  <dt>客户端版本</dt>
                  <dd>{device.appVersion ?? "等待上报"}</dd>
                </div>
                <div>
                  <dt>独立阈值</dt>
                  <dd>{device.settings.thresholdDb ? `${formatDb(device.settings.thresholdDb)} dB` : "跟随系统默认"}</dd>
                </div>
                <div>
                  <dt>最后心跳</dt>
                  <dd>{device.lastSeen ? formatRelative(device.lastSeen) : "尚未连接"}</dd>
                </div>
                <div>
                  <dt>添加时间</dt>
                  <dd>{device.createdAt ? formatRelative(device.createdAt) : "--"}</dd>
                </div>
              </dl>

              <div className="device-access-actions">
                <button
                  type="button"
                  className={selected ? "secondary-button small" : "primary-button small"}
                  onClick={() => onSelectDevice(device.id)}
                  disabled={selected}
                >
                  <ArrowRightLeft size={14} />
                  {selected ? "正在查看" : "切换到此设备"}
                </button>
                <button
                  type="button"
                  className="ghost-button small"
                  onClick={() =>
                    setEdit({
                      device,
                      name: device.name,
                      thresholdDb: device.settings.thresholdDb?.toString() ?? "",
                      alertCooldownSecs: device.settings.alertCooldownSecs?.toString() ?? "",
                      offlineTimeoutSecs: device.settings.offlineTimeoutSecs?.toString() ?? "",
                      autoRecordEvidence:
                        device.settings.autoRecordEvidence === null
                          ? ""
                          : device.settings.autoRecordEvidence
                            ? "true"
                            : "false",
                    })
                  }
                >
                  <Edit3 size={14} />
                  编辑
                </button>
                {canManage && (
                  <>
                    <button
                      type="button"
                      className="ghost-button small"
                      onClick={() => setConfirmAction({ kind: "rotate", device })}
                    >
                      <KeyRound size={14} />
                      更换令牌
                    </button>
                    <button
                      type="button"
                      className="ghost-button small danger-text"
                      onClick={() => setConfirmAction({ kind: "delete", device })}
                      disabled={devices.length <= 1}
                      title={devices.length <= 1 ? "至少保留一个设备" : "移除设备"}
                    >
                      <Trash2 size={14} />
                      移除
                    </button>
                  </>
                )}
              </div>
            </section>
          );
        })}
      </div>

      {addOpen && (
        <div className="modal-overlay" role="dialog" aria-modal="true">
          <form className="modal-panel access-modal" onSubmit={createDevice}>
            <div className="modal-panel-head">
              <div>
                <div className="eyebrow">新设备</div>
                <h2>添加采集设备</h2>
              </div>
              <button type="button" className="icon-button ghost" onClick={() => setAddOpen(false)} aria-label="关闭">
                <X size={17} />
              </button>
            </div>
            <p className="modal-help">
              设备名称必须唯一，例如“教室 101”。保存后会生成一次性设备令牌，需要填入采集端设置。
            </p>
            <label className="form-field">
              <span>设备名称</span>
              <input
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="例如：教室 101"
                autoFocus
                maxLength={60}
                required
              />
            </label>
            <label className="form-field">
              <span>自定义设备编号 <em>可选</em></span>
              <input
                value={deviceKey}
                onChange={(event) => setDeviceKey(event.target.value)}
                placeholder="留空自动生成，例如 room-101"
                maxLength={64}
              />
              <small>只能使用字母、数字、短横线和下划线。</small>
            </label>
            {formError && <div className="form-error">{formError}</div>}
            <div className="modal-actions">
              <button type="button" className="ghost-button" onClick={() => setAddOpen(false)}>取消</button>
              <button type="submit" className="primary-button" disabled={busy || !name.trim()}>
                {busy ? <LoaderCircle size={15} className="spin" /> : <Plus size={15} />}
                {busy ? "正在创建" : "创建设备"}
              </button>
            </div>
          </form>
        </div>
      )}

      {access && (
        <div className="modal-overlay" role="dialog" aria-modal="true">
          <div className="modal-panel access-modal token-modal">
            <div className="modal-panel-head">
              <div>
                <div className="eyebrow">{access.rotated ? "令牌已更新" : "设备已创建"}</div>
                <h2>{access.rotated ? "设备令牌已更换" : "设备创建成功"}</h2>
              </div>
              <button
                type="button"
                className="icon-button ghost"
                onClick={() => {
                  setAccess(null);
                  if (!access.rotated) onSelectDevice(access.device.id);
                }}
                aria-label="关闭"
              >
                <X size={17} />
              </button>
            </div>
            <div className="token-success">
              <CheckCircle2 size={18} />
              <span>
                {access.rotated
                  ? "旧令牌已立即失效，使用旧令牌的客户端会断开连接。"
                  : "请立即复制并妥善保存令牌，关闭本窗口后将不再显示完整令牌。"}
              </span>
            </div>
            <div className="access-details">
              <div><span>设备名称</span><strong>{access.device.name}</strong></div>
              <div><span>设备编号</span><code>{access.device.id}</code></div>
              <div><span>服务器地址</span><code>{window.location.origin}</code></div>
            </div>
            <div className="token-field">
              <span>设备令牌</span>
              <div>
                <code>{access.token}</code>
                <button type="button" className="ghost-button small" onClick={() => void copyTokenOnly()}>
                  <Copy size={13} />
                  复制
                </button>
              </div>
            </div>
            <ol className="access-steps">
              <li>点击采集端右上角设置，输入管理员关闭密码完成验证。</li>
              <li>填写服务器地址、设备编号和设备令牌。</li>
              <li>保存后客户端会自动重连，本页状态将变为在线。</li>
            </ol>
            <div className="modal-actions">
              <button type="button" className="ghost-button" onClick={() => void copyAccess()}>
                {copied ? <CheckCircle2 size={15} /> : <Copy size={15} />}
                {copied ? "已复制全部信息" : "复制全部连接信息"}
              </button>
              <button
                type="button"
                className="primary-button"
                onClick={() => {
                  setAccess(null);
                  if (!access.rotated) onSelectDevice(access.device.id);
                }}
              >
                完成
              </button>
            </div>
          </div>
        </div>
      )}

      {edit && (
        <div className="modal-overlay" role="dialog" aria-modal="true">
          <form className="modal-panel access-modal" onSubmit={saveEdit}>
            <div className="modal-panel-head">
              <div>
                <div className="eyebrow">编辑设备</div>
                <h2>编辑设备信息</h2>
              </div>
              <button type="button" className="icon-button ghost" onClick={() => setEdit(null)} aria-label="关闭">
                <X size={17} />
              </button>
            </div>
            <label className="form-field">
              <span>设备名称</span>
              <input
                value={edit.name}
                onChange={(event) => setEdit({ ...edit, name: event.target.value })}
                maxLength={60}
                required
              />
            </label>
            <div className="device-policy-head">
              <div>
                <strong>设备独立策略</strong>
                <small>留空表示跟随系统设置；证据音频固定保留完整 30 秒窗口。</small>
              </div>
            </div>
            <div className="form-grid two">
              <label className="form-field">
                <span>分贝阈值</span>
                <input
                  type="number"
                  min={30}
                  max={130}
                  step={0.1}
                  value={edit.thresholdDb}
                  onChange={(event) => setEdit({ ...edit, thresholdDb: event.target.value })}
                  placeholder="跟随系统"
                />
              </label>
              <label className="form-field">
                <span>告警冷却（秒）</span>
                <input
                  type="number"
                  min={30}
                  max={86400}
                  value={edit.alertCooldownSecs}
                  onChange={(event) => setEdit({ ...edit, alertCooldownSecs: event.target.value })}
                  placeholder="跟随系统"
                />
              </label>
            </div>
            <div className="form-grid two">
              <label className="form-field">
                <span>离线判定（秒）</span>
                <input
                  type="number"
                  min={20}
                  max={3600}
                  value={edit.offlineTimeoutSecs}
                  onChange={(event) => setEdit({ ...edit, offlineTimeoutSecs: event.target.value })}
                  placeholder="跟随系统"
                />
              </label>
              <label className="form-field">
                <span>超限证据</span>
                <select
                  value={edit.autoRecordEvidence}
                  onChange={(event) =>
                    setEdit({
                      ...edit,
                      autoRecordEvidence: event.target.value as "" | "true" | "false",
                    })
                  }
                >
                  <option value="">跟随系统</option>
                  <option value="true">启用</option>
                  <option value="false">关闭</option>
                </select>
              </label>
            </div>
            {formError && <div className="form-error">{formError}</div>}
            <div className="modal-actions">
              <button type="button" className="ghost-button" onClick={() => setEdit(null)}>取消</button>
              <button type="submit" className="primary-button" disabled={busy || !edit.name.trim()}>
                {busy ? <LoaderCircle size={15} className="spin" /> : <Edit3 size={15} />}
                保存
              </button>
            </div>
          </form>
        </div>
      )}

      {confirmAction && (
        <div className="modal-overlay" role="dialog" aria-modal="true">
          <div className="modal-panel confirm-modal">
            <div className={`modal-head-icon ${confirmAction.kind === "delete" ? "danger" : "warning"}`}>
              {confirmAction.kind === "delete" ? <Trash2 size={20} /> : <KeyRound size={20} />}
            </div>
            <h2>
              {confirmAction.kind === "delete" ? "移除设备" : "更换设备令牌"}
            </h2>
            <p>
              {confirmAction.kind === "delete"
                ? `即将移除「${confirmAction.device.name}」。历史遥测与告警仍保留在服务端，但该设备将无法继续上报。`
                : `更换后，「${confirmAction.device.name}」的旧令牌立即失效。采集端必须更新令牌后才能恢复连接。`}
            </p>
            <div className="modal-actions">
              <button type="button" className="ghost-button" onClick={() => setConfirmAction(null)}>取消</button>
              <button
                type="button"
                className={confirmAction.kind === "delete" ? "danger-button" : "primary-button"}
                onClick={() => void runConfirm()}
                disabled={busy}
              >
                {busy ? <LoaderCircle size={15} className="spin" /> : null}
                {confirmAction.kind === "delete" ? "确认移除" : "确认更换"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
