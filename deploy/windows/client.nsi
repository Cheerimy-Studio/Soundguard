Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define APPNAME "声垣采集端"
!define APPNAME_EN "SoundGuard Collector"
!define COMPANY "Cheerimy Studio"
!define VERSION "0.2.0"
!define EXENAME "soundguard-client-native.exe"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define RUNVALUE "SoundGuard"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\SoundGuardClient"

Name "${APPNAME}"
OutFile "..\..\dist\soundguard-client-setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\SoundGuard"
InstallDirRegKey HKCU "Software\SoundGuard\Client" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
Icon "..\..\client-native\assets\icon.ico"
UninstallIcon "..\..\client-native\assets\icon.ico"
BrandingText "${APPNAME} ${VERSION} · ${COMPANY}"
VIProductVersion "0.2.0.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "CompanyName" "${COMPANY}"
VIAddVersionKey "FileDescription" "声垣声音监测采集端安装程序"
VIAddVersionKey "FileVersion" "0.2.0"
VIAddVersionKey "ProductVersion" "0.2.0"
VIAddVersionKey "LegalCopyright" "© 2026 Cheerimy Studio"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"
!define MUI_FINISHPAGE_RUN_TEXT "立即启动声垣采集端"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!define MUI_UNCONFIRMPAGE_TEXT_TOP "卸载会删除程序并移除开机自启。client-config.json（设备编号、令牌与本地密码）会保留在安装目录，便于重新安装后继续使用。"
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

; A running client locks the executable. The normal close path asks for the
; local password / dynamic code, so the installer must never kill it silently.
Function CheckRunning
  retry:
  nsExec::Exec 'cmd /C tasklist /FI "IMAGENAME eq ${EXENAME}" /NH | find /I "${EXENAME}" >nul'
  Pop $0
  ${If} $0 == 0
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "声垣采集端正在运行。$\r$\n$\r$\n请先在客户端点击关闭，并输入本地密码或管理端动态验证码退出，然后点击“重试”继续安装。" IDRETRY retry
    Abort
  ${EndIf}
FunctionEnd

Function .onInit
  Call CheckRunning
FunctionEnd

Section "声垣采集端（必需）" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "..\..\client-native\target\release\${EXENAME}"
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${EXENAME}"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\卸载 ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\SoundGuard\Client" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${COMPANY}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXENAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "开机自动启动" SecAutostart
  WriteRegStr HKCU "${RUNKEY}" "${RUNVALUE}" '"$INSTDIR\${EXENAME}"'
SectionEnd

Section "创建桌面快捷方式" SecDesktop
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXENAME}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "安装声垣原生采集端。首次启动需要填写管理端生成的设备编号、令牌和本地密码。"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "登录 Windows 后自动启动采集端，适合长期监测设备。可在 Windows 设置 → 应用 → 启动中关闭。"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "在桌面创建声垣采集端快捷方式。"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  DeleteRegValue HKCU "${RUNKEY}" "${RUNVALUE}"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\卸载 ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$INSTDIR\${EXENAME}"
  ; client-config.json is intentionally preserved for reinstall.
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\SoundGuard\Client"
SectionEnd
