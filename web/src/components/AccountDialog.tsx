import { useState } from "react";
import { KeyRound, LoaderCircle, X } from "lucide-react";
import { api } from "../api";
import { roleLabels } from "../lib/permissions";
import type { User } from "../types";

export function AccountDialog({ user, onClose, onChanged }: {
  user: User; onClose: () => void; onChanged: () => void;
}) {
  const [current, setCurrent] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (busy) return;
    if (password !== confirm) { setError("两次输入的新密码不一致"); return; }
    if ([...password].length < 8 || [...password].length > 128) {
      setError("新密码需为 8–128 个字符"); return;
    }
    setBusy(true); setError("");
    try {
      await api.changePassword(current, password);
      onChanged();
    } catch (error) {
      setError(error instanceof Error ? error.message : "修改密码失败");
      setBusy(false);
    }
  }

  return <div className="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="account-title"
    onKeyDown={(event) => { if (event.key === "Escape" && !busy) onClose(); }}>
    <form className="modal-panel access-modal" onSubmit={submit}>
      <div className="modal-panel-head">
        <div><h2 id="account-title">账号与密码</h2><p className="modal-help">{user.username} · {roleLabels[user.role]}</p></div>
        <button type="button" className="icon-button ghost" aria-label="关闭" disabled={busy} onClick={onClose}><X size={17} /></button>
      </div>
      <p className="modal-help">修改后需重新登录，其他已登录的会话也将失效。</p>
      <label className="form-field"><span>当前密码</span><input type="password" autoComplete="current-password"
        value={current} onChange={event => setCurrent(event.target.value)} required autoFocus disabled={busy} /></label>
      <label className="form-field"><span>新密码</span><input type="password" autoComplete="new-password"
        value={password} onChange={event => setPassword(event.target.value)} required disabled={busy} /></label>
      <label className="form-field"><span>再次输入新密码</span><input type="password" autoComplete="new-password"
        value={confirm} onChange={event => setConfirm(event.target.value)} required disabled={busy} /></label>
      {error && <div className="form-error" role="alert">{error}</div>}
      <div className="modal-actions"><button type="button" className="ghost-button" disabled={busy} onClick={onClose}>取消</button>
        <button type="submit" className="primary-button" disabled={busy || !current || !password || !confirm}>
          {busy ? <LoaderCircle size={15} className="spin" /> : <KeyRound size={15} />}修改密码</button></div>
    </form>
  </div>;
}
