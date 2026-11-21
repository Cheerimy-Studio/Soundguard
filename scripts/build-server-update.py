#!/usr/bin/env python3
"""Builds the ZIP uploaded through Settings -> 软件更新 -> 上传完整包."""
import json
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
version = json.loads((root / "package.json").read_text(encoding="utf-8"))["version"]
windows = root / "dist" / "soundguard-server.exe"
linux = root / "dist" / "soundguard-server-linux-x86_64"
web = root / "web" / "dist"
out = root / "dist" / f"soundguard-server-update-{version}.zip"

for path in (windows, linux):
    if not path.is_file():
        raise SystemExit(f"missing server binary: {path}")
if not (web / "index.html").is_file():
    raise SystemExit("missing web/dist; run npm run build:web first")

with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
    zf.write(windows, "windows/soundguard-server.exe")
    zf.write(linux, "linux/soundguard-server")
    for path in sorted(web.rglob("*")):
        if path.is_file():
            zf.write(path, "web/dist/" + path.relative_to(web).as_posix())
    zf.writestr("version.txt", version + "\n")

print(out)
