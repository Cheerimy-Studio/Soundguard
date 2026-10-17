# 声垣 Windows 7 兼容采集端

该目录是一个独立的 Tauri 客户端，用于在 **Windows 7 SP1 64 位** 上运行与主客户端相同的界面和采集功能。

## 为什么需要单独版本

主客户端使用 Rust 1.94 / Tauri 2.12 构建，其 Windows 运行库依赖 `bcryptprimitives.dll!ProcessPrng`，该接口只在 Windows 10 及以上存在。直接安装 WebView2 不能解决这个错误。

本目录将工具链固定为：

- Rust `1.77.2`（最后一个正式支持 Windows 7 的 Rust 版本）
- Tauri `2.2.5` / `tauri-build 2.2.0`
- `tauri-runtime` `2.3.0`
- `tauri-runtime-wry` `2.3.0`
- `tauri-utils` `2.4.0`
- `wasapi` `0.19.0`
- MSVC 静态 CRT（`+crt-static`），不要求安装 VC++ 运行库

## 构建

```powershell
rustup toolchain install 1.77.2-x86_64-pc-windows-msvc
cd client-win7
.\build.ps1
```

`build.ps1` 会在编译后调用 `scripts/patch-imports.py`，把旧版 `windows` crate 链接的 `combase.dll!CoTaskMemAlloc/CoTaskMemFree` 导入改到 Windows 7 自带的 `ole32.dll`。缺少这一步时，Windows 7 会报“丢失 combase.dll”。脚本需要 Python 3 和 `pefile`：

```powershell
python -m pip install pefile
```

产物：

```text
client-win7\target\release\soundguard-client.exe
```

## 生成含 WebView2 109 的安装包

安装版会把 WebView2 109 固定运行时一起打包，目标机器无需联网，也无需预装 WebView2。先下载微软 109.0.1518.78 x64 固定运行时 cab，解压到：

```text
src-tauri\webview2-runtime\Microsoft.WebView2.FixedVersionRuntime.109.0.1518.78.x64\
```

该目录必须直接包含 `msedgewebview2.exe`。然后运行：

```powershell
# 在 client-win7 目录
..\client\node_modules\.bin\tauri.cmd bundle --bundles nsis
```

安装包输出到：

```text
target\release\bundle\nsis\声垣_0.1.0_x64-setup.exe
```

安装模式为当前用户，程序安装到 `%LOCALAPPDATA%\声垣`，该目录可写，`client-config.json` 会保存在程序旁边。

## 运行要求

1. Windows 7 SP1 64 位，安装 SHA-2 更新 KB4474419、KB4490628。
2. 安装 WebView2 Runtime **109.0.1518.78**（最后支持 Windows 7 的版本）。
3. 把 exe 和 `client-config.json` 放在同一可写目录运行，不要放在 `Program Files`。

## 与主客户端的差异

界面 HTML/CSS/JS 和业务逻辑与 `client/` 保持一致。为兼容旧版 API，仅有三处实现差异：

- 麦克风权限通过 WebView2 `PermissionRequested` 事件授权，替代 Tauri 2.12 的 `on_permission_request`。
- WASAPI 使用 0.19 的设备枚举接口。
- 系统播放检测直接调用 `IAudioMeterInformation`。

## 限制

Windows 7 已停止安全更新。该版本仅建议用于隔离局域网；公网部署应使用 Windows 10/11 主客户端。
