@echo off
chcp 65001 >nul
setlocal
cd /d "%~dp0"

rem Keep runtime data outside the program directory so upgrades and uninstall
rem never delete the database, media, or media key.
if not exist "%PROGRAMDATA%\SoundGuard" mkdir "%PROGRAMDATA%\SoundGuard"
set "DATA_DIR=%PROGRAMDATA%\SoundGuard\data"
set "WEB_DIST=%~dp0web\dist"
if not exist "%DATA_DIR%" mkdir "%DATA_DIR%"

if not exist ".env" (
  copy /Y ".env.example" ".env" >nul
  echo [声垣] 已创建 .env，可按需修改端口、SMTP 与 TURN 配置。
)

if not exist "%DATA_DIR%\initial-admin-password.txt" (
  echo [声垣] 首次启动会自动生成管理员密码，请稍后查看：
  echo        %DATA_DIR%\initial-admin-password.txt
)

echo [声垣] 管理后台：http://127.0.0.1:8080
start "" "http://127.0.0.1:8080"
".\soundguard-server.exe"
if errorlevel 1 (
  echo.
  echo [声垣] 服务端异常退出，请检查上方错误信息后按任意键关闭。
  pause >nul
)
