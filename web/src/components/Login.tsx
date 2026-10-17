import { useEffect, useState } from "react";
import { ArrowRight, Eye, EyeOff, LockKeyhole, Waves } from "lucide-react";
import { api, setToken } from "../api";
import type { HealthResponse, User } from "../types";

interface LoginProps {
  onAuthenticated: (token: string, user: User) => void;
}

export function Login({ onAuthenticated }: LoginProps) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [healthFailed, setHealthFailed] = useState(false);

  useEffect(() => {
    let active = true;
    api
      .health()
      .then((result) => {
        if (active) setHealth(result);
      })
      .catch(() => {
        if (active) setHealthFailed(true);
      });
    return () => {
      active = false;
    };
  }, []);

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!username.trim() || !password.trim() || loading) return;
    setLoading(true);
    setError("");
    try {
      const result = await api.login(username.trim(), password);
      setToken(result.token);
      onAuthenticated(result.token, result.user);
    } catch (err) {
      setError(err instanceof Error ? err.message : "登录失败");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="login-simple">
      <div className="login-brand">
        <div className="brand-mark">
          <Waves size={22} strokeWidth={2.4} />
        </div>
        <div>
          <div className="brand-name">声垣</div>
          <div className="brand-latin">SOUNDGUARD</div>
        </div>
      </div>

      <div className="login-card">
        <div className="login-card-top">
          <span className={`login-server ${healthFailed ? "is-off" : ""}`}>
            <span className="dot" />
            {healthFailed
              ? "服务不可达"
              : health
                ? `服务正常 · v${health.version}`
                : "正在连接服务"}
          </span>
        </div>
        <h1>声垣管理端</h1>
        <p>登录后查看环境声音、处理告警和管理设备。</p>

        <form onSubmit={handleSubmit}>
          <label htmlFor="admin-username">用户名</label>
          <div className="login-password">
            <LockKeyhole size={17} />
            <input
              id="admin-username"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              placeholder="admin"
              autoComplete="username"
              autoCapitalize="none"
              spellCheck={false}
            />
          </div>
          <label htmlFor="admin-password">密码</label>
          <div className="login-password">
            <LockKeyhole size={17} />
            <input
              id="admin-password"
              type={visible ? "text" : "password"}
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder="请输入密码"
              autoComplete="current-password"
            />
            <button
              type="button"
              className="plain-icon"
              onClick={() => setVisible((value) => !value)}
              aria-label={visible ? "隐藏密码" : "显示密码"}
            >
              {visible ? <EyeOff size={16} /> : <Eye size={16} />}
            </button>
          </div>
          {error && <div className="login-error">{error}</div>}
          <button className="login-submit" type="submit" disabled={loading}>
            {loading ? "正在登录…" : "进入管理端"}
            {!loading && <ArrowRight size={16} />}
          </button>
        </form>

        <div className="login-footer">
          <span>© 2026 Cheerimy Studio</span>
          <span>{health?.smtpConfigured ? "SMTP 已配置" : "SMTP 未配置"}</span>
        </div>
      </div>
    </div>
  );
}
