import { useEffect, useMemo, useState } from "react";
import {
  Check,
  KeyRound,
  LoaderCircle,
  Pencil,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
  UserRound,
  X,
} from "lucide-react";
import { api } from "../api";
import { roleLabels, rolePermissions } from "../lib/permissions";
import type { Device, User, UserRole } from "../types";

interface UsersPageProps {
  devices: Device[];
  currentUser: User;
  onNotify: (message: string, tone?: "success" | "error" | "info") => void;
}

interface UserForm {
  mode: "create" | "edit";
  user: User | null;
  username: string;
  email: string;
  password: string;
  role: UserRole;
  deviceIds: string[];
  disabled: boolean;
}

export function UsersPage({ devices, currentUser, onNotify }: UsersPageProps) {
  const canEdit = currentUser.role === "super_admin";
  const [users, setUsers] = useState<User[]>([]);
  const [loading, setLoading] = useState(true);
  const [form, setForm] = useState<UserForm | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const deviceNames = useMemo(
    () => new Map(devices.map((device) => [device.id, device.name])),
    [devices],
  );

  async function load() {
    setLoading(true);
    try {
      setUsers(await api.users());
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "用户列表加载失败", "error");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
  }, []);

  function openCreate() {
    setError("");
    setForm({
      mode: "create",
      user: null,
      username: "",
      email: "",
      password: "",
      role: "device_admin",
      deviceIds: devices[0] ? [devices[0].id] : [],
      disabled: false,
    });
  }

  function openEdit(user: User) {
    setError("");
    setForm({
      mode: "edit",
      user,
      username: user.username,
      email: user.email,
      password: "",
      role: user.role,
      deviceIds: [...user.deviceIds],
      disabled: user.disabled,
    });
  }

  function toggleDevice(id: string) {
    if (!form) return;
    const next = form.deviceIds.includes(id)
      ? form.deviceIds.filter((item) => item !== id)
      : [...form.deviceIds, id];
    setForm({ ...form, deviceIds: next });
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!form || busy || !canEdit) return;
    setBusy(true);
    setError("");
    try {
      if (form.mode === "create") {
        await api.createUser({
          username: form.username.trim(),
          email: form.email.trim(),
          password: form.password,
          role: form.role,
          deviceIds: form.role === "device_admin" ? form.deviceIds : [],
        });
        onNotify("用户已创建", "success");
      } else if (form.user) {
        await api.updateUser(form.user.id, {
          username: form.username.trim(),
          email: form.email.trim(),
          role: form.role,
          deviceIds: form.role === "device_admin" ? form.deviceIds : [],
          disabled: form.disabled,
          password: form.password ? form.password : undefined,
        });
        onNotify("用户信息已更新", "success");
      }
      setForm(null);
      await load();
    } catch (error) {
      setError(error instanceof Error ? error.message : "保存失败");
    } finally {
      setBusy(false);
    }
  }

  async function remove(user: User) {
    if (!canEdit) return;
    if (!window.confirm(`确定删除用户「${user.username}」吗？`)) return;
    try {
      await api.deleteUser(user.id);
      onNotify("用户已删除", "success");
      await load();
    } catch (error) {
      onNotify(error instanceof Error ? error.message : "删除失败", "error");
    }
  }

  return (
    <div className="users-page">
      <section className="panel users-hero">
        <div>
          <div className="eyebrow">角色与权限</div>
          <h2>管理后台账号</h2>
          <p>
            按职责选择角色，再分配设备。运营管理员负责全部设备和日常运行策略；账号授权与敏感设置由超级管理员维护。
          </p>
        </div>
        <div className="users-hero-actions">
          <button type="button" className="ghost-button" onClick={() => void load()}>
            <RefreshCw size={15} />
            刷新
          </button>
          {canEdit && <button type="button" className="primary-button" onClick={openCreate}>
            <Plus size={15} />
            添加用户
          </button>}
        </div>
      </section>

      <section className="panel permission-panel">
        <h3>角色权限一览</h3>
        <div className="permission-table-wrap"><table className="permission-table">
          <thead><tr><th>操作范围</th><th>超级管理员</th><th>运营管理员</th><th>设备管理员</th></tr></thead>
          <tbody>{rolePermissions.map(row => <tr key={row.label}><th>{row.label}</th>{row.values.map((v,i)=><td key={i}>{v}</td>)}</tr>)}</tbody>
        </table></div>
        {!canEdit && <p className="inline-note">当前账号可以查看权限，创建、编辑和删除账号需由超级管理员操作。</p>}
      </section>
      <section className="panel users-list-panel">
        <div className="panel-header">
          <div>
            <div className="eyebrow">账号列表</div>
            <h3>{loading ? "正在加载" : `${users.length} 个账号`}</h3>
          </div>
        </div>
        {loading ? (
          <div className="users-loading">
            <LoaderCircle size={20} className="spin" />
            正在读取用户…
          </div>
        ) : (
          <div className="users-list">
            {users.map((user) => (
              <article className={`user-row ${user.disabled ? "is-disabled" : ""}`} key={user.id}>
                <div className="user-avatar">
                  {user.role === "super_admin" ? <ShieldCheck size={19} /> : <UserRound size={19} />}
                </div>
                <div className="user-main">
                  <div className="user-name-line">
                    <strong>{user.username}</strong>
                    {user.id === currentUser.id && <span className="self-chip">当前账号</span>}
                    <span className={`role-chip ${user.role}`}>
                      {roleLabels[user.role]}
                    </span>
                    {user.disabled && <span className="disabled-chip">已停用</span>}
                  </div>
                  <div className="user-meta">
                    {user.email || "未设置接收邮箱"}
                    {" · "}
                    {user.role !== "device_admin"
                      ? "全部设备"
                      : user.deviceIds.length > 0
                        ? user.deviceIds.map((id) => deviceNames.get(id) ?? id).join("、")
                        : "尚未分配设备"}
                    {user.lastLoginAt ? ` · 最近登录 ${new Date(user.lastLoginAt).toLocaleString("zh-CN", { hour12: false })}` : ""}
                  </div>
                </div>
                {canEdit && <div className="user-actions">
                  <button type="button" className="ghost-button small" onClick={() => openEdit(user)}>
                    <Pencil size={13} />
                    编辑
                  </button>
                  {user.id !== currentUser.id && (
                    <button
                      type="button"
                      className="ghost-button small danger-text"
                      onClick={() => void remove(user)}
                    >
                      <Trash2 size={13} />
                      删除
                    </button>
                  )}
                </div>}
              </article>
            ))}
          </div>
        )}
      </section>

      {form && canEdit && (
        <div className="modal-overlay" role="dialog" aria-modal="true">
          <form className="modal-panel access-modal user-form" onSubmit={submit}>
            <div className="modal-panel-head">
              <div>
                <div className="eyebrow">{form.mode === "create" ? "新账号" : "账号设置"}</div>
                <h2>{form.mode === "create" ? "添加后台用户" : `编辑 ${form.user?.username ?? ""}`}</h2>
              </div>
              <button type="button" className="icon-button ghost" onClick={() => setForm(null)} aria-label="关闭">
                <X size={17} />
              </button>
            </div>

            <label className="form-field">
              <span>用户名</span>
              <input
                value={form.username}
                onChange={(event) => setForm({ ...form, username: event.target.value })}
                placeholder="例如：teacher-li"
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                required
              />
            </label>

            <label className="form-field">
              <span>接收邮箱</span>
              <input
                type="email"
                value={form.email}
                onChange={(event) => setForm({ ...form, email: event.target.value })}
                placeholder="用于接收该账号权限内的告警和日报"
                autoComplete="email"
                required
              />
            </label>

            <label className="form-field">
              <span>{form.mode === "create" ? "初始密码" : "重置密码"}</span>
              <input
                type="password"
                value={form.password}
                onChange={(event) => setForm({ ...form, password: event.target.value })}
                placeholder={form.mode === "create" ? "至少 8 个字符" : "留空表示不修改"}
                autoComplete="new-password"
                required={form.mode === "create"}
              />
            </label>

            <label className="form-field">
              <span>角色与操作范围</span>
              <select
                value={form.role}
                onChange={(event) => setForm({ ...form, role: event.target.value as UserRole })}
              >
                <option value="update_admin">运营管理员</option>
              <option value="device_admin">设备管理员</option>
                <option value="super_admin">超级管理员</option>
              </select>
            </label>

            {form.role === "device_admin" && (
              <div className="form-field">
                <span>可管理的设备</span>
                <div className="device-check-grid">
                  {devices.map((device) => {
                    const checked = form.deviceIds.includes(device.id);
                    return (
                      <button
                        key={device.id}
                        type="button"
                        className={`device-check ${checked ? "is-checked" : ""}`}
                        onClick={() => toggleDevice(device.id)}
                      >
                        <span className="device-check-box">{checked && <Check size={13} />}</span>
                        <span>{device.name}</span>
                      </button>
                    );
                  })}
                </div>
                {devices.length === 0 && <small>请先添加设备。</small>}
              </div>
            )}

            {form.mode === "edit" && form.user?.id !== currentUser.id && (
              <label className="toggle-field">
                <span>
                  <strong>停用账号</strong>
                  <small>停用后已登录会话会立即失效</small>
                </span>
                <input
                  type="checkbox"
                  checked={form.disabled}
                  onChange={(event) => setForm({ ...form, disabled: event.target.checked })}
                />
              </label>
            )}

            {error && <div className="form-error">{error}</div>}

            <div className="modal-actions">
              <button type="button" className="ghost-button" onClick={() => setForm(null)}>
                取消
              </button>
              <button type="submit" className="primary-button" disabled={busy}>
                {busy ? <LoaderCircle size={15} className="spin" /> : <KeyRound size={15} />}
                {busy ? "正在保存" : "保存用户"}
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}
