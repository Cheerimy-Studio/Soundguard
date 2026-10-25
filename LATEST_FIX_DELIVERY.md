# SoundGuard 最新 FIX · 2026-10-25

本包为停止追加修改/验证时的最新成果，以本文件为准。基线完整 SHA 见 BASE_COMMIT.txt。
未推送远程仓库，未修改线上服务。

## 包含文件
- windows/native/soundguard-client-native.exe：Windows 10/11 x64 原生 egui 客户端。
- windows/win7/soundguard-client.exe 与 WebView2Loader.dll：保留旧 WebView 界面的 Win7 x64 客户端。需保留原 WebView2 109 运行环境；本包不含完整 WebView2 固定运行时。
- source/：完整最新修复源码，含 Win7 和安卓端修改。
- changes.patch：针对 BASE_COMMIT.txt 的可应用补丁，含新增字体等二进制资源。
- web-dist/ 与 source/web/dist/：已构建管理后台资源。
- verification/：本轮已有检查记录。按要求不再追加测试。

## 本轮修复
1. 恢复 egui 系统标题栏、窗口图标和 EXE 图标；继续按旧 WebView 调整双栏卡片、字号、矢量图标、设置抽屉及密码弹窗。
2. 统一正常关闭入口的密码保护，修复已配置设备的须知页/退出 IPC 绕过；预览和截图不会修改真实配置或绕过正式会话验证。
3. 旧 WebView/Win7 支持本地密码，以及联网验证固定密码和动态验证码；保留原 Win7 构建路线，修复不兼容依赖锁定。
4. 服务端下发有效计划状态，客户端共享计划时钟；跨午夜、全天、手动暂停优先及断网后的缓存计划切换统一处理。
5. 网页和 APP 显示计划状态及战争模式；增加权限内排名；安卓后台提醒遵守战争与采集时段。
6. 管理后台加入角色权限矩阵、设置分区、敏感操作限制；修复未保存设备策略被刷新覆盖；固定密码独立保存并保留首尾空格。
7. 恢复原生 Opus/WebM 回放，启用真实浏览器录音解码测试；损坏容器返回错误；修复并发配置保存临时文件冲突。
8. 网页、旧客户端和 APP 加入中文后备字体。默认客户端构建切换到 egui，旧 WebView/Win7 仍有独立入口。

## 已有验证
- 原生端 35 项测试通过，0 忽略，包含 Chromium 实际生成的 Opus/WebM 录音解码。
- 服务端 26 项测试通过；109 项 HTTP/WebSocket/存储检查通过。
- 10 组浏览器 GUI/移动网页流程通过，覆盖计划保存、角色权限、编辑保留和 APP 计划展示。
- 管理后台生产构建通过；服务端 Clippy 检查通过。管理后台仍有 JS 文件较大的构建提示。
- 原生 Windows EXE 与 Win7 EXE 已交叉编译链接生成；Win7 使用 Rust 1.77.2，已完成 combase 到 ole32 的受限导入修正，并附 WebView2Loader.dll。
- 已在测试显示环境检查原生主界面、设置、密码弹窗、首次配置和战争模式。截图不等于 Windows 系统标题栏实机验收。

## 使用与升级
先关闭旧客户端，备份 client-config.json。将对应平台 EXE 放入可写目录，并迁入原配置。Win7 请使用 windows/win7，勿用 native 替换；保留原 WebView2 109 环境。
升级服务端前备份完整数据目录、.env 与媒体密钥。服务端和后台应配套更新，否则旧客户端无法取得本轮服务端计划支持。本包不包含服务器发布二进制、安卓 APK 或签名密钥。
干净基线可执行 git apply --check changes.patch 后 git apply changes.patch；有本地修改时先合并。也可直接使用 source 完整源码。
回滚先停止新程序，再恢复旧程序及升级前一致的数据/配置/密钥备份。

## 尚未验收
Windows/Win7/安卓实机、真实麦克风/扬声器/AEC、系统 DPI 与休眠、公网 TURN、SMTP 及长期运行仍待验收；正常关闭需要验证密码，但操作系统强制终止进程不属于应用可拦截入口。
当前不能宣称与旧版逐像素完全一致，也不能宣称所有功能实机兼容验收完成。
source/FIX_RELEASE.md 和 source/FIX_ROUND2.md 包含历史或先行包说明，本包内容及验证状态以此文件和 verification 记录为准。
