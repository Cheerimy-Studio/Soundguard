#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "请使用 root 运行：sudo ./deploy/uninstall.sh" >&2
  exit 1
fi

PURGE="${1:-}"

systemctl disable --now soundguard 2>/dev/null || true
rm -f /etc/systemd/system/soundguard.service
systemctl daemon-reload
systemctl reset-failed soundguard 2>/dev/null || true

if [ "$PURGE" = "--purge" ]; then
  rm -rf /opt/soundguard
  echo "已卸载声垣服务并删除 /opt/soundguard（包括数据、证书和密钥）。"
else
  rm -f /opt/soundguard/soundguard-server
  rm -rf /opt/soundguard/web
  echo "已卸载声垣服务程序。"
  echo "数据与配置保留在 /opt/soundguard/data 和 /opt/soundguard/.env。"
  echo "如需彻底删除：sudo ./deploy/uninstall.sh --purge"
fi
