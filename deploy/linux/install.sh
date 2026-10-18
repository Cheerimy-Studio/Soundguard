#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "请使用 root 运行：sudo ./deploy/install.sh" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ ! -f "$ROOT_DIR/soundguard-server" ]; then
  ROOT_DIR="$(cd "$ROOT_DIR/.." && pwd)"
fi
DEST=/opt/soundguard

if [ ! -x "$ROOT_DIR/soundguard-server" ]; then
  echo "未找到 soundguard-server，请在解压后的发布包根目录运行。" >&2
  exit 1
fi

id soundguard >/dev/null 2>&1 || useradd --system --home-dir "$DEST" --shell /usr/sbin/nologin soundguard

install -d -m 0755 -o soundguard -g soundguard "$DEST"
install -d -m 0700 -o soundguard -g soundguard "$DEST/data"
if systemctl is-active --quiet soundguard; then
  systemctl stop soundguard
fi
install -m 0755 -o soundguard -g soundguard "$ROOT_DIR/soundguard-server" "$DEST/soundguard-server.new"
mv -f "$DEST/soundguard-server.new" "$DEST/soundguard-server"
install -d -m 0755 -o soundguard -g soundguard "$DEST/deploy"
install -m 0755 -o soundguard -g soundguard "$ROOT_DIR/deploy/uninstall.sh" "$DEST/deploy/uninstall.sh"
rm -rf "$DEST/web"
cp -R "$ROOT_DIR/web" "$DEST/web"
chown -R soundguard:soundguard "$DEST/web"

if [ ! -f "$DEST/.env" ]; then
  install -m 0600 -o soundguard -g soundguard "$ROOT_DIR/.env.example" "$DEST/.env"
  echo "已创建 $DEST/.env，请按需填写 SMTP、域名等配置。"
fi
install -m 0644 "$ROOT_DIR/deploy/soundguard.service" /etc/systemd/system/soundguard.service

systemctl daemon-reload
systemctl enable soundguard
systemctl restart soundguard
systemctl is-active --quiet soundguard || { journalctl -u soundguard -n 30 --no-pager; exit 1; }

echo "============================================================"
echo "声垣服务已启动：http://<服务器IP>:8080"
echo "初始管理员用户名：admin"
echo "初始密码文件：$DEST/data/initial-admin-password.txt"
echo "首次登录后请立即修改密码，并删除该密码文件。"
echo "查看日志：journalctl -u soundguard -f"
echo "============================================================"
