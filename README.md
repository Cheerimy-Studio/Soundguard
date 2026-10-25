# 声垣 SoundGuard · 智能分贝监测与远程对讲系统

> 采集端已统一为原生 egui 客户端，Windows WebView/Tauri 与 Win7 客户端不再构建或发布。
> 本轮修复、验证与升级说明见最新 Fix 包内的 `READ_ME_FIRST.md`。

一套 Rust 全栈环境声学监测系统：

- **采集端（Windows 10/11）**：Rust + eframe/egui 原生窗口，WASAPI 采集与回放，无需 WebView2；30 秒能量均值、超限报警音、证据 WAV、设置/退出 2FA 验证、断线重连与 WebRTC 对讲。
- **服务端**：Rust + Axum，JSON 文件持久化、用户名/密码登录、Argon2 哈希、超级管理员/更新管理员/设备管理员角色、设备级策略、SMTP 告警、日报、离线检测与 WebRTC 信令。Linux 发布包可直接部署到服务器。
- **管理端**：React + TypeScript + ECharts，浅色响应式界面，多设备接入与切换、实时波形、趋势、告警中心、证据回放、远程采集启停、WebRTC 双向对讲与单向广播、分贝战争排行榜。
- **安卓端**：原生 WebView 外壳 + 前台监听服务，本地记忆登录、设备选择、实时波形、超标通知、录音回放、双向对讲、公告、采集计划与分贝战争排名。

> 原生采集端实测空闲约 58 MB 工作集，采集中约 90 MB（含 WebRTC、30 秒音频缓冲与网络连接）；空闲 CPU 约 2%。音频采集、30 秒聚合、证据缓冲、报警音、密码退出与网络心跳全部在 Rust 侧完成。

---

## 1. 已实现功能

### 采集端（Windows）

- 浅色卡片界面，只显示后台设置的唯一设备名称；状态、计划与阈值说明集中在主卡片。
- 打开客户端设置、关闭自动接听、关闭客户端都需要输入首次配置时设置的本机密码。
- 基于 Windows WASAPI 的原生麦克风采集，支持设备时钟与格式自动转换；Rust 实时计算 RMS / dBFS。
- 使用 Windows 通信设备与回声消除参考端点；同时检测系统播放电平，电脑自身播放的声音不会计入环境分贝。驱动不支持硬件 AEC 时，会检测到系统播放并从分贝统计中忽略该时段，但录音证据仍保留完整 30 秒。
- 通话期间自动暂停分贝检测与上报，结束后自动恢复。
- 普通标签文字不可选中，只有输入框允许复制，避免误选影响操作。
- 每 30 秒计算一次 **Leq 能量平均**、峰值、最低值与采样帧数；测量前应用 80 Hz 高通；不对环境测量应用会压低持续噪声的自适应噪声门。
- 内置 **RNNoise AI 降噪**（nnnoiseless 纯 Rust 移植）：常驻背景噪声（如持续 60 dB 嗡声）被神经网络持续抑制，突发的异常声音保持突出。提供关闭/轻度/标准/强化四档模式与人声增强（2–5 kHz 语音临场感提升），在管理端统一设置，采集端约 10 秒内自动生效。
- 可校准的 dB SPL 偏移（默认 90 dB），超限时自动上传完整 **30 秒**单声道 48 kHz WAV，覆盖整个平均窗口。
- 实时滚动声压折线图，包含平均线与阈值线；客户端每秒把当前读数推送到服务端，后台与安卓端同步刷新。
- 通话记录卡片：显示管理端发起的对讲/广播记录，并可在客户端回放。
- 每 10 秒心跳；每 30 秒上报；断网时报告进入本地待发队列，恢复后重发。
- 密码授权关闭与设置解锁：本机密码以 Argon2 哈希保存在客户端程序目录，连续输错 5 次锁定 5 分钟；关闭事件仍会上报服务端审计。
- 关闭弹窗不再提供强制退出按钮；退出必须通过密码验证。
- 窗口关闭由 Rust 拦截；强杀进程 / 断电 / 断网由服务端在 45 秒（可配置）后触发离线告警。
- 管理员远程暂停/恢复采集：客户端保持心跳在线，但停止麦克风采集与上报。
- 支持多设备：每台设备独立令牌、独立状态、独立告警与通话记录。
- WebRTC 双向对讲、单向广播；支持自动接听或现场手动接听，频道状态与计时可见。关闭自动接听需要密码验证。
- 对讲启用回声消除与半双工防啸叫保护：远端说话时自动压低本端麦克风，避免同场部署时形成声反馈。
- 管理端与安卓端通话录音使用 48 kHz 混音与 192 kbps Opus 编码，保存清晰的双方语音。
- 通知板块显示后台发布的标题与内容；通话记录仅展示最近 5 条，支持回放。
- 界面采用原生 egui 绘制，浅色卡片布局，支持高 DPI 与字体回退，不依赖 WebView2。

### 服务端

- 用户名/密码登录（JWT，管理端默认 24 小时，安卓端记住 30 天；Argon2 密码哈希；连续失败锁定 5 分钟）。首次启动自动创建 `admin` 账号并生成随机密码。
- 多用户：超级管理员、设备管理员；设备管理员只能查看和操作分配给自己的设备的监测数据与对讲/广播，设备接入、令牌轮换、设备策略和删除均由超级管理员完成，不能修改全局设置或管理用户。
- 告警、遥测、远程控制、WebRTC 信令均按设备隔离。
- 通话记录：保存对讲/广播的发起时间、模式、时长、状态与可选 WebM 录音。
- 设备令牌认证；停止密码校验与锁定；所有安全事件审计；每个设备可覆盖阈值、冷却、离线判定和证据开关。
- 分贝阈值、冷却时间、离线判定、证据时长、数据保留、日报时间均可在线配置，实时同步到客户端。
- 告警邮件：噪音超限（可附 WAV）、设备离线、关闭验证失败、授权关闭审计。
- 日报：按设备定时汇总最近 24 小时数据，附 CSV 与 SVG 趋势图；未配置 SMTP 时写入 `data/outbox/` 预览文件。
- 跨域白名单（开发期宽松）、请求体大小限制、证据文件名白名单。
- JSON 状态文件 + 按天 JSONL 遥测文件，自动清理过期读数。
- WebSocket：管理端实时推送；设备/管理端 WebRTC 信令中继。
- 静态托管管理端 `web/dist`。

### 管理端

- 用户名/密码登录、角色权限、设备连接管理、用户管理、顶部设备切换器、每秒实时曲线、24 小时统计。
- 添加设备并一次性展示设备令牌，支持编辑名称、设备独立策略、轮换令牌、移除设备；设备管理员只能操作分配给自己的设备。
- 多设备页面均按当前选中设备加载。
- 历史数据页：24h / 7d / 30d 切换、均值+峰值双曲线、阈值线、数据缩放、CSV 导出、原始记录表格。
- 告警中心：按类型筛选、已确认状态、证据音频在线回放与下载、邮件投递状态。
- 设备控制：远程暂停/恢复采集、节点状态、安全机制与语音通道。
- 系统设置（仅超级管理员）：全局默认阈值、收件人、客户端通知、日报、离线判定、告警冷却、证据录音、数据保留。
- WebRTC 对讲：管理员发起双向对讲或广播；支持静音、挂断、音频解锁提示。
- 管理端与安卓端在通话期间自动录制混音，并在结束时上传为通话记录；Windows 客户端与安卓端均可回放。

---

## 2. 架构

```text
┌────────────────────── Windows 采集端 (egui + Rust 原生) ───────────────────────┐
│  WASAPI 麦克风 ──► Rust 30s Leq / 峰值 / 环形音频缓冲 ──► HTTP 心跳 + 遥测       │
│       │                         │                          │                 │
│       └── 原生 egui 界面 ◄──────┴── 2FA 退出 / 设置 / 报警音  │                 │
│                                      WebRTC 麦克风与扬声器 ◄─┼── 信令 WS      │
└─────────────────────────────────────────────────────────────┼────────────────┘
                                                              ▼
┌───────────────────────── Rust Axum 服务端 ────────────────────────────────────┐
│  REST API / WebSocket 广播 / WebRTC 信令中继 / 多设备隔离                        │
│  JSON 状态 + 按天 JSONL 遥测 + WAV 证据                                        │
│  离线检测 / 告警冷却 / 日报调度 / SMTP (lettre)                                │
└───────────────────────────────────────────────────────────────┬───────────────┘
                                                                │
                 ┌──────────────────────────────────────────────┴──────────────┐
                 ▼                                                             ▼
       React + ECharts 管理端                                      SMTP / 管理员邮箱
       实时曲线 · 告警 · 控制 · 对讲
```

### WebRTC 说明

音视频媒体本身是管理端浏览器与原生采集端之间的 **P2P WebRTC**，服务端只转发 SDP 与 ICE 信令，不转发媒体流。默认使用 Google STUN；跨公网或对称 NAT 环境请在 `.env` 的 `ICE_SERVERS` 中加入 TURN 服务器：

```env
ICE_SERVERS=[{"urls":"stun:stun.l.google.com:19302"},{"urls":"turn:turn.example.com:3478","username":"user","credential":"secret"}]
```

管理端页面若通过局域网 IP 用 HTTP 打开，浏览器会禁止 `getUserMedia`。对讲功能要求在 `localhost` 或 HTTPS 下访问管理端。

---

## 3. 目录结构

```text
分贝仪/
├─ server/                  Rust Axum 服务端
│  ├─ src/api.rs            HTTP、WebSocket、信令
│  ├─ src/store.rs          JSON / JSONL / 证据持久化
│  ├─ src/mailer.rs         SMTP 与 dry-run outbox
│  ├─ src/monitor.rs        离线检测与数据清理
│  ├─ src/reports.rs        CSV / SVG 日报
│  └─ src/auth.rs           JWT 与常量时间比较
├─ web/                     React 管理端 (Vite + ECharts)
├─ client-native/           Windows 采集端（egui + Rust）
│  ├─ src/ui.rs             浅色卡片界面、波形、设置抽屉
│  ├─ src/audio.rs          WASAPI 采集、30s Leq、证据 WAV
│  ├─ src/windows_audio.rs  WASAPI 采集与 AEC 参考
│  ├─ src/rtc.rs            WebRTC 对讲与 PCMU 音频
│  ├─ src/playback.rs       扬声器报警音与 WebM/Opus 回放
│  └─ assets/               中文后备字体、应用图标
├─ client/common/           采集端与服务端共享的音频/配置/安全模块
├─ scripts/
│  ├─ seed-demo.mjs         生成 24h 演示数据并触发一次带证据的告警
│  ├─ collect-release.mjs   收集发布版 exe 到 dist/
│  ├─ start.ps1             从 dist/ 启动服务端与客户端
├─ mobile/android/           安卓 App（WebView + 前台监听服务）
├─ dist/                    预构建发布版 exe 与安卓 APK
├─ data/                    运行时数据（gitignore）
├─ .env.example
└─ README.md
```

---

## 4. 快速启动

### 环境要求

- Windows 10/11 x64（原生采集端；Windows 7 不再支持）
- Node.js 20+（本项目验证于 Node 24）
- Rust stable（本项目验证于 1.94）与 MSVC Build Tools
- 可选：Python 3 + Pillow（重新生成图标）；本仓库已包含生成好的图标

### 4.1 配置

```powershell
copy .env.example .env
```

打开 `.env`，至少修改：

```env
ADMIN_PASSWORD=
JWT_SECRET=
DEVICE_TOKEN=
# 当前客户端使用本地密码；STOP_PIN 仅用于兼容旧版 API
STOP_PIN=
ALERT_TO=你的邮箱@example.com
```

`ALERT_TO` 留空时不会真的发信，告警仍会进入后台；SMTP 未配置时邮件会写入 `data/outbox/` 方便检查。

### 4.2 安装依赖并构建管理端

```powershell
npm run setup
npm run build:web
```

### 4.3 启动服务端

在仓库根目录运行（`WEB_DIST=web/dist` 是相对路径，必须从根目录启动）：

```powershell
npm run dev:server
```

打开 <http://127.0.0.1:8080>，用户名 `admin`，初始密码在 `data/initial-admin-password.txt`。首次登录后请立即修改密码并删除该文件。

### 4.4 启动采集端

另开一个终端：

```powershell
npm run dev:client
```

首次启动 Windows 客户端会进入引导页，要求填写服务器地址、设备编号、设备令牌，并设置本机密码。设备编号和令牌在管理端“设备连接”中添加设备时生成。

要接入第二台设备或使用自定义名称（例如“教室 101”）：

1. 在管理端打开 **设备连接**，点击 **添加设备**。
2. 填写唯一设备名称（例如“教室 101”），创建后立即复制管理端生成的一次性设备令牌。
3. 首次启动会进入引导页；填写服务器地址、设备编号、设备令牌，并设置本机密码。之后打开设置时输入该本地密码。
4. 回到管理端顶部设备切换器，即可分别查看每台设备的实时数据、告警与通话记录。

采集端绑定信息保存在：

```text
客户端程序目录\client-config.json
```

程序优先把服务地址、设备编号、设备令牌和校准偏移保存在可执行文件旁边；本地密码只保存 Argon2 哈希，不保存明文。若旧版本已在 `%APPDATA%\com.soundguard.client\client-config.json` 存在配置，会自动迁移，并在新版本首次启动时要求补设本地密码。

首次配置时设置的本机密码用于打开设置、关闭自动接听和关闭客户端；密码以 Argon2 哈希只保存在本机 `client-config.json`。

### 4.5 发布版

```powershell
npm run build
```

构建产物会自动收集到：

```text
dist\soundguard-server.exe
dist\soundguard-client-native.exe
```

也可以直接运行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\start.ps1
```

启动脚本要求先存在 `.env`，并会从仓库根目录启动服务端，确保能读取 `web/dist`。原生采集端不需要 WebView2。

### 4.6 安卓 App

仓库已包含可直接安装的 APK：

```text
dist\soundguard-android-0.1.0.apk
```

安装后打开 **声垣**：

1. 服务器地址填写电脑在局域网中的地址，例如 `http://192.168.1.10:8080`。手机上的 `127.0.0.1` 指向手机本身，不能使用。
2. 先检测服务器连接，然后输入用户名和密码。登录令牌会保存在 App 私有存储中，30 天内再次打开会自动登录。
3. 顶部选择设备后，页面每秒同步该设备的实时分贝与波形。
4. 首次启动请允许 **麦克风** 与 **通知** 权限；在“后台提醒”中点击“允许后台运行”，把 App 加入电池优化白名单，关闭 App 界面后仍能收到超标通知。

安卓端提供：初始化服务器检测、用户名/密码登录与自动登录、设备选择、每秒实时波形、最近 1/5/15/30/60/120/180 分钟历史波形与具体超限时间、采集启停、公告查看与编辑（超级管理员）、告警/通话录音回放、双向对讲与远程广播、后台超标通知。没有阈值、邮件、日报等高级设置。

重新构建 APK（需要 JDK 21 与 Android SDK 35）：

```powershell
cd mobile\android
.\gradlew.bat assembleDebug
```

调试 APK 输出在 `mobile/android/app/build/outputs/apk/debug/app-debug.apk`。发布 APK 使用项目发布密钥签名（`mobile/android/keystore/soundguard-release.jks`，别名 `soundguard`，密码保存在 `mobile/android/keystore.properties`）。请务必备份密钥和密码文件，否则无法覆盖升级已安装的 App。

### 4.7 Linux 服务端

仓库提供：

```text
dist\soundguard-linux-x86_64.tar.gz
dist\soundguard-server-linux-x86_64
Dockerfile
docker-compose.yml
deploy/linux/install.sh
deploy/linux/soundguard.service
```

Linux 版本是静态链接的 x86_64 musl 二进制，不需要在服务器安装 Rust、Node.js 或 OpenSSL。推荐使用 systemd 发布包：

```bash
tar -xzf soundguard-linux-x86_64.tar.gz
cd soundguard
sudo ./deploy/install.sh
```

服务安装在 `/opt/soundguard`，初始管理员密码位于：

```text
/opt/soundguard/data/initial-admin-password.txt
```

也可以使用 Docker：

```bash
cp .env.example .env
docker compose up -d --build
```

完整的 Linux 部署、HTTPS、备份、防火墙与排障说明见 [README-LINUX.md](README-LINUX.md)。

---

## 5. 配置项

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `PORT` / `BIND_ADDR` | `8080` / `0.0.0.0` | 服务端监听 |
| `DATA_DIR` | `data` | 状态、遥测、证据、outbox |
| `WEB_DIST` | `web/dist` | 管理端构建目录 |
| `PUBLIC_BASE_URL` | `http://127.0.0.1:8080` | 邮件中的管理端链接 |
| `ADMIN_PASSWORD` | 空 | 首次启动的超级管理员密码；空时自动生成随机密码 |
| `ADMIN_EMAIL` | 空 | 超级管理员接收邮箱；留空时回退到第一个 `ALERT_TO` |
| `JWT_SECRET` | 空 | JWT 签名密钥；空/默认值时自动生成并保存到 `data/jwt-secret` |
| `JWT_TTL_HOURS` | `24` | 登录有效期 |
| `DEVICE_ID` | 空 | 初始设备编号；空时自动生成随机编号 |
| `DEVICE_NAME` | 未命名设备 | 初始设备名称（后台可修改，仅名称） |
| `DEVICE_TOKEN` | 空 | 首次创建初始设备时使用；空时自动生成到 `data/initial-device-token` |
| `STOP_PIN` | 空 | 旧版采集端服务端关闭密码；当前客户端使用本地密码，保留用于兼容 API |
| `THRESHOLD_DB` | `65` | 平均分贝阈值 |
| `ALERT_TO` | 空 | 告警收件人，支持逗号/分号分隔 |
| `REPORT_TO` | 回退到 `ALERT_TO` | 日报收件人 |
| `DAILY_REPORT_ENABLED` | `true` | 是否启用定时日报 |
| `DAILY_REPORT_TIME` | `08:00` | 本地时间日报发送时刻 |
| `OFFLINE_TIMEOUT_SECS` | `45` | 离线判定秒数（客户端 10 秒心跳） |
| `ALERT_COOLDOWN_SECS` | `300` | 持续超限时邮件冷却 |
| `EVIDENCE_SECONDS` | `30` | 固定值；证据录音始终覆盖完整 30 秒平均窗口，该项不会被覆盖 |
| `DENOISE_MODE` | `off` | RNNoise AI 降噪：`off` / `light` / `standard` / `strong`；也可在管理端设置页修改 |
| `VOICE_ENHANCE` | `false` | 降噪后的语音临场感提升（2–5 kHz），影响分贝测量与证据录音 |
| `AUTO_RECORD_EVIDENCE` | `true` | 超限自动录音 |
| `RETENTION_DAYS` | `30` | 遥测、告警、通话记录和邮件预览保留天数；无引用音频自动清理 |
| `SMTP_HOST/PORT/USER/PASS/FROM` | 空 | SMTP 配置 |
| `ICE_SERVERS` | Google STUN | WebRTC ICE 服务器 JSON |
| `CORS_ORIGINS` | 空 | 允许的管理端来源，逗号分隔；空时支持同源访问及内置安卓 App。安卓 App 来源为 `https://appassets.androidplatform.net` |
| `TLS_CERT` / `TLS_KEY` | 空 | 同时配置时服务端直接提供 HTTPS/WSS；否则建议在 Nginx/Caddy 终止 TLS |
| `MEDIA_KEY_FILE` | `data/media-key` | 音频加密密钥路径；生产建议放在 data 目录和备份卷之外 |
| `NVIDIA_API_KEY` | 空 | build.nvidia.com 的 API Key（推荐在管理端「系统设置」填写）；配置后告警中心可对超限证据一键 NVIDIA BNR 云降噪，失败自动重试 3 次。注意：音频会上传至 NVIDIA 处理 |
| `NVIDIA_BNR_FUNCTION_ID` | `b415e29a-…` | NVIDIA 托管 BNR 端点的 function-id，一般无需修改 |
| `SMTP_TLS` | `starttls` | `starttls`（587）、`wrapper`（465）或 `none`（仅本地中继） |

阈值、收件人、日报时间、离线判定、冷却、证据与保留天数也可在管理端设置页修改，修改后立即同步客户端。

`.env` 中显式填写的 `THRESHOLD_DB`、`ALERT_TO`、`REPORT_TO`、日报、离线判定、冷却、自动录音和保留天数，会在每次启动时覆盖 `data/state.json` 中保存的全局设置，并写入日志；删除对应环境变量后，管理端保存的值重新生效。设备级单独设置仍优先于全局设置。

---

## 6. 邮件

### SMTP

QQ 邮箱示例：

```env
SMTP_HOST=smtp.qq.com
SMTP_PORT=587
SMTP_USER=your_account@qq.com
SMTP_PASS=你的SMTP授权码
SMTP_FROM=your_account@qq.com
SMTP_FROM_NAME=声垣管理员
SMTP_TLS=starttls
```

`SMTP_PASS` 不是 QQ 登录密码，而是邮箱设置中开启 SMTP 后生成的授权码。

### 投递模式

- `SMTP_TLS=starttls`：587 端口 STARTTLS（QQ/163/企业邮箱常用）。已用 QQ SMTP 实测发送成功。
- `SMTP_TLS=wrapper`：465 端口隐式 TLS。
- `SMTP_TLS=none`：仅用于本地可信中继，不要在公网使用。

### dry-run

如果 `SMTP_HOST` 为空：

- 所有邮件不会真实发出；
- 系统在 `data/outbox/` 写入标题、收件人、正文和附件文件名，方便联调；
- 告警中心显示“本地预览”，表示邮件未实际发出。

建议部署前用设置页的“发送测试告警”和“立即生成日报”验证收件链路。

---

## 7. 数据与隐私

运行时数据：

```text
data/
├─ state.json                 多设备、设置、告警、通话记录、日报状态
├─ readings/YYYY-MM-DD.jsonl  每 30 秒一条遥测（含设备编号）
├─ evidence/*.wav             超限证据（48 kHz，单声道）
├─ calls/*.webm               管理端或安卓端发起的通话录音（AES-256-GCM 加密）
├─ media-key                  音频加密密钥（务必备份）
└─ outbox/*.txt               SMTP dry-run 预览
```

- 音频缓冲区只在内存中保留最近约 32 秒；**只有平均分贝超限且启用证据功能时**，才把最近 N 秒 WAV 上传并随邮件发送。
- 采集端界面明确显示采集状态、远程对讲状态与证据设置，不做隐藏采集。
- 声纹/语音识别未启用。若后续接入 Whisper / SpeechBrain 等声纹识别，请先完成合法性评估、隐私告知和访问控制。
- 录音与员工/场所监测可能受当地法律约束，部署者需自行承担合规责任。

---

## 8. 安全设计

- 管理端 JWT；同一账号登录连续失败 5 次锁定 5 分钟。
- 设备侧使用独立设备令牌，不暴露管理 JWT。
- 本机密码只以 Argon2 哈希保存在客户端程序目录，不保存明文；旧版服务端 STOP_PIN 仅保留用于兼容 API。
- 打开客户端设置、关闭自动接听、关闭客户端都需要本机密码；连续错误 5 次锁定 5 分钟，并发送安全审计事件。
- 设备绑定信息（服务地址、设备编号、令牌、校准偏移）保存在客户端程序目录；本机密码哈希也只保存在该目录。
- 设备令牌只以 SHA-256 哈希保存在服务端；管理端添加设备时一次性显示明文令牌。
- 证据 WAV 与通话录音使用 **AES-256-GCM** 静态加密；密钥保存在 `data/media-key`（仅服务账号可读）。API 鉴权后按需解密，磁盘上不保留明文音频。
- 可直接启用原生 HTTPS/WSS（`TLS_CERT` + `TLS_KEY`），或在 Nginx/Caddy 终止 TLS；WebRTC 媒体本身使用 DTLS-SRTP 加密。
- `data/media-key` 必须与业务数据一起备份，丢失后已加密的历史音频无法恢复。
- 证据与通话录音接口分别需要管理员或对应设备令牌鉴权；文件名白名单校验。
- 正常关闭验证通过后标记 `expectedOffline`，不会误报离线。
- 强杀、断电、断网、系统休眠导致心跳中断，超过阈值后发送一次离线邮件。
- 首次启动自动生成随机超级管理员密码、JWT 密钥和初始设备令牌；管理员密码文件权限为仅服务账号可读，使用后应删除。设备令牌后续由管理端生成。
- 生产环境请：
  - 使用 HTTPS（推荐 Caddy / Nginx 反向代理，并转发 WebSocket）；
  - 配置 TURN，避免公网对称 NAT 下 WebRTC 失败；
  - 修改所有默认凭据，并使 JWT 密钥足够长；
  - 将管理端访问限制在可信网络或 VPN；
  - 不要把 `data/` 和 `.env` 提交到版本库。

> 本系统是相对变化监测与告警工具，并未按计量级声级计进行型式检定，不应作为法定噪声监测或执法依据。

---

## 9. 校准

1. 将设备放在现场典型位置，避免直接对向空调出风口或扬声器。
2. 用专业声级计与采集端同时读数。
3. 打开采集端“客户端设置 → 麦克风校准”，调整“dB 偏移”：
   - 若客户端显示 38 dB、声级计显示 55 dB，则将偏移从 90 调到约 107。
   - 保存后立即应用，下一窗口生效。
4. 建议在安静环境与正常交谈两种场景各校准一次。

不同麦克风的频响与自动增益差异较大，默认 90 仅适用于多数笔记本内置麦克风。若要长期使用，建议固定音频设备并关闭系统“麦克风增强”。

---

## 10. 开发与测试

```powershell
# 服务端单元测试
npm run test:server

# 管理端类型检查 + 构建
npm run build:web

# 客户端编译检查
cargo check --manifest-path client-native/Cargo.toml
```

已验证的核心链路：

- 管理员登录、JWT、设置同步；
- 真实麦克风 → Rust 30 秒 Leq → 遥测入库 → 管理端实时更新；
- 多设备添加、令牌接入、顶部设备切换、按设备告警与统计；
- 客户端设置密码验证、关闭自动接听密码验证、关闭密码验证；
- 阈值触发告警 → SMTP dry-run 邮件（含 WAV 证据）；
- 强制断连 → 45 秒后离线告警；
- 远程暂停/恢复采集 → 客户端状态与音频线程响应；
- 关闭密码错误 → 安全审计 + 锁定计数；
- 正确关闭密码 → 授权关闭事件 + 不触发离线告警；
- WebRTC 双向对讲、单向广播、自动接听与手动接听、管理端远程音轨确认；
- 管理端通话混音录制、服务端保存、客户端通话记录回放；
- 告警证据 WAV 读取与后台在线回放（含修复后的告警 ID → 证据文件映射）；
- 24 小时历史曲线、CSV 导出、告警筛选、证据回放；
- 浅色客户端界面、实时滚动声压图、平均值/阈值线；
- 系统播放检测与忽略、通话期间分贝暂停、通知板块、最近 5 条通话记录。
- 发布版构建。

### 可选：演示数据

在服务端运行且 `.env` 使用演示设备令牌时：

```powershell
npm run seed
```

会生成最近 24 小时波动数据，并发送一条带 3 秒 WAV 的证据告警。该脚本会写入真实 `data/readings/`，请勿在生产环境随意运行。

---

## 11. 已知边界与后续阶段

本版本聚焦“可工作的第一阶段 + 第二阶段核心安全能力”。以下未包含或有意留白：

- **声纹识别 / 人员身份识别**：未实现。建议后续采用独立 Python 服务（SpeechBrain / Whisper + 声纹 embedding），通过异步任务接入，且必须单独做合规评估。
- **MQTT**：当前采用 REST + WebSocket 直连，减少部署依赖，已验证更稳定的局域网与公网直连。若已有 MQTT 平台，可在服务端增加一个 `rumqttc` 桥接层，将 `decibel/{deviceId}/telemetry` 发布到现有消息总线。
- **真正远程关机**：出于安全考虑，远程只控制“采集启停”，不直接杀掉客户端进程；否则设备离网后无法远程恢复。现场退出和设置解锁都必须输入关闭密码。7×24 部署建议配置 Windows 自动登录 + 启动项。
- **多设备**：每台设备支持独立阈值、冷却时间、离线判定与证据开关；未设置的字段继承全局默认值。
- **TURN**：默认只带 STUN，公网复杂网络请自行配置 TURN。
- **电脑播放声处理**：优先使用 Windows 硬件 AEC；若声卡驱动不支持 AEC，系统通过系统播放电平检测到电脑正在发声时，会暂停该时段的分贝计算以避免误报。
- **客户端通知**：当前为后台统一通知，适用于所有设备；尚未做按设备定向通知。
- **通话录音**：管理端与安卓端在通话期间录制；如果通话方直接关闭页面/App 且未正常挂断，可能没有录音，但设备端仍会结束通话。
- **设置作用域**：邮件、日报、客户端通知为全局设置；阈值、冷却、离线判定、证据开关支持设备覆盖。
- **安卓后台**：使用前台服务与 5 秒状态轮询；部分厂商系统仍可能清理后台，请在系统电池管理中允许“声垣”后台运行。安卓登录令牌默认记住 30 天，过期后需重新输入用户名和密码。
- **局域网 HTTP**：安卓 App 为便于局域网部署允许明文 HTTP；若部署到公网，请在反向代理上启用 HTTPS，并同步修改 App 中的服务器地址。
- **安装包签名**：Windows exe 未签名，可能提示 SmartScreen；安卓 APK 使用本机调试证书自签名，适合内网侧载，不用于应用商店。正式分发请使用企业代码签名证书与独立安卓发布证书。
- **声级计量级精度**：未做 A 计权滤波与麦克风频响补偿；如需计量级读数，应接入专用声级计硬件或实现 A 计权与校准证书。

---

## 12. 许可

MIT。音频采集与邮件告警涉及隐私与合规，请在合法授权的场所部署。
