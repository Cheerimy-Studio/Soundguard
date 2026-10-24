#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BINARY="dist/soundguard-server-linux-x86_64"
if [ ! -f "$BINARY" ]; then
  echo "缺少 $BINARY。请先完成 Linux 交叉编译或从 CI 产物复制。" >&2
  exit 1
fi
if [ ! -d web/dist ]; then
  echo "缺少 web/dist，请先运行 npm run build:web。" >&2
  exit 1
fi

STAGING="data/linux-package"
rm -rf "$STAGING"
mkdir -p "$STAGING/deploy" "$STAGING/web"

cp "$BINARY" "$STAGING/soundguard-server"
chmod +x "$STAGING/soundguard-server"
cp -R web/dist "$STAGING/web/dist"
cp .env.example README-LINUX.md "$STAGING/"
cp deploy/linux/soundguard.service deploy/linux/install.sh deploy/linux/uninstall.sh deploy/linux/nginx.conf.example "$STAGING/deploy/"
chmod +x "$STAGING/deploy/install.sh" "$STAGING/deploy/uninstall.sh"

mkdir -p dist
rm -f dist/soundguard-linux-x86_64.tar.gz.new
tar -czf dist/soundguard-linux-x86_64.tar.gz.new -C "$STAGING" .
mv -f dist/soundguard-linux-x86_64.tar.gz.new dist/soundguard-linux-x86_64.tar.gz
echo "packaged dist/soundguard-linux-x86_64.tar.gz"
