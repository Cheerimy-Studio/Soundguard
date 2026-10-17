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
cargo +1.77.2 build --release --manifest-path src-tauri\Cargo.toml
```

产物：

```text
client-win7\target\release\soundguard-client.exe
```

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
