# 声垣 Linux 服务端部署

发布包包含一个静态链接的 `soundguard-server`（Linux x86_64, musl），无需安装 Rust、Node.js 或 OpenSSL。管理端前端已包含在 `web/dist`。

## 方式一：systemd 直接部署（推荐）

1. 把发布包解压到任意目录，例如：
   ```bash
   tar -xzf soundguard-linux-x86_64.tar.gz
   cd soundguard
   ```
2. 安装：
   ```bash
   sudo ./deploy/install.sh
   ```
3. 打开 `http://服务器IP:8080`。
4. 首次启动会自动创建超级管理员：
   ```text
   用户名：admin
   密码：/opt/soundguard/data/initial-admin-password.txt
   ```
   首次登录后请修改密码，并删除该文件。
5. 查看状态与日志：
   ```bash
   systemctl status soundguard
   journalctl -u soundguard -f
   ```

服务目录为 `/opt/soundguard`，数据目录为 `/opt/soundguard/data`。升级时替换 `soundguard-server` 和 `web/`，然后执行 `sudo systemctl restart soundguard`，`data/` 会保留。

## 方式二：Docker Compose

在项目根目录：

```bash
cp .env.example .env
docker compose up -d --build
docker compose logs -f soundguard
```

首次启动后，初始管理员密码位于容器数据卷：

```bash
docker compose exec soundguard cat /data/initial-admin-password.txt
```

管理端：`http://服务器IP:8080`。

## 首次配置

编辑 `/opt/soundguard/.env` 或 Docker 的 `.env`：

```env
PORT=8080
BIND_ADDR=0.0.0.0
PUBLIC_BASE_URL=https://sound.example.com
# STOP_PIN 仅用于兼容旧版采集端 API；当前 Windows 客户端使用首次配置时设置的本地密码
ALERT_TO=admin@example.com
SMTP_HOST=smtp.example.com
SMTP_PORT=587
SMTP_USER=...
SMTP_PASS=...
SMTP_FROM=...
ICE_SERVERS=[{"urls":"stun:stun.l.google.com:19302"}]
```

`ADMIN_PASSWORD` 留空时，服务端会在首次启动时自动生成随机超级管理员密码。`JWT_SECRET` 与初始设备令牌也会自动生成并写入 `data/`，重启不会变化。

修改配置后重启：

```bash
sudo systemctl restart soundguard
```

## 用户与权限

- **超级管理员**：管理所有设备、全局策略、日报、邮件、通知和后台用户。
- **设备管理员**：只能查看、控制、新增并管理分配给自己的设备（可轮换令牌、删除设备），不能修改全局设置或管理用户。

在管理端「用户管理」中创建账号并勾选可管理的设备。用户名和密码使用 Argon2 哈希存储。

## 原生 HTTPS

在 `.env` 中同时配置证书和私钥，服务端会直接提供 HTTPS/WSS：

```env
TLS_CERT=/etc/soundguard/fullchain.pem
TLS_KEY=/etc/soundguard/privkey.pem
```

证书目录需要让 `soundguard` 用户可读，并确保私钥权限为 `0600`。使用原生 HTTPS 后，管理端、安卓 App 和 Windows 采集端的服务器地址都要改成 `https://...`。

## 反向代理与 HTTPS

浏览器只在 `localhost` 或 HTTPS 下允许麦克风/WebRTC。公网部署也可以使用 Nginx/Caddy 配置 HTTPS，示例见 `deploy/nginx.conf.example`：

```
server_name sound.example.com;
proxy_pass http://127.0.0.1:8080;
proxy_set_header Upgrade $http_upgrade;
proxy_set_header Connection "upgrade";
```

WebSocket 必须保留 Upgrade 头，通话与实时数据依赖它。

## 数据与备份

```text
/opt/soundguard/data/state.json                 用户、设备、设置、告警、通话记录
/opt/soundguard/data/readings/*.jsonl            按天遥测
/opt/soundguard/data/evidence/*.wav              30 秒告警证据
/opt/soundguard/data/calls/*.webm                通话录音（AES-256-GCM 加密）
/opt/soundguard/data/media-key                   音频加密密钥（如未设置 MEDIA_KEY_FILE）
/etc/soundguard/media-key                        生产建议的密钥位置（单独备份，不能丢）
/opt/soundguard/data/initial-admin-password.txt  首次生成的管理员密码（使用后删除）
/opt/soundguard/data/initial-stop-pin.txt        首次生成的采集端关闭密码
/opt/soundguard/data/initial-device-token        初始设备令牌
```

建议每日备份整个 `data/` 目录：

```bash
sudo tar -czf /var/backups/soundguard-$(date +%F).tar.gz -C /opt/soundguard data
```

Docker 使用命名卷 `soundguard-data`，备份示例：

```bash
docker run --rm -v soundguard-data:/data -v "$PWD":/backup alpine   tar -czf /backup/soundguard-$(date +%F).tar.gz -C /data .
```

## 防火墙

开放 8080（或只允许反向代理访问）：

```bash
sudo ufw allow 8080/tcp
```

生产环境建议只开放 443，8080 仅监听本机反向代理。

## Windows 采集端与安卓端

- Windows 采集端使用 `dist/soundguard-client-native.exe` 或客户端安装包 `dist/soundguard-client-setup-0.2.1.exe`，服务器地址填 Linux 服务器的局域网 IP 或 HTTPS 域名。
- 安卓端使用 `dist/soundguard-android-0.1.0.apk`，首次启动先检测服务器连接，再输入用户名和密码。
- 设备令牌在管理端「设备连接」中添加设备时生成一次，填入 Windows 客户端设置。

## 排查

| 现象 | 处理 |
| --- | --- |
| 打不开后台 | `systemctl status soundguard`；确认端口和防火墙 |
| 忘记管理员密码 | 停止服务，将 `data/state.json` 中 users 备份后联系维护人员；或从备份恢复 |
| 实时/通话连接失败 | 检查反向代理 WebSocket Upgrade 头 |
| 安卓收不到通知 | 允许通知权限与后台运行，检查厂商电池管理 |
| 时间不正确 | 服务器时区需为 `Asia/Shanghai`，执行 `timedatectl set-timezone Asia/Shanghai` |

## 卸载与重新安装

### systemd

停止并卸载程序但保留数据：

```bash
sudo /opt/soundguard/deploy/uninstall.sh
```

如果发布包解压目录还在，也可以执行：

```bash
sudo ./deploy/uninstall.sh
```

彻底删除程序、数据、密钥和证书：

```bash
sudo ./deploy/uninstall.sh --purge
```

重新安装：

```bash
tar -xzf soundguard-linux-x86_64.tar.gz
cd soundguard
sudo ./deploy/install.sh
```

### Docker

停止容器但保留数据卷：

```bash
docker compose down
```

停止并删除数据卷：

```bash
docker compose down -v
```

删除镜像：

```bash
docker image rm soundguard:latest
```

### Windows 本地

停止进程并保留数据：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\uninstall-local.ps1
```

同时删除运行数据：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\uninstall-local.ps1 -PurgeData
```

删除桌面上的客户端副本和配置：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\uninstall-local.ps1 -RemoveDesktopCopy
```

重新开始首次配置：删除 `data` 运行数据和各客户端配置后启动服务端/客户端。
