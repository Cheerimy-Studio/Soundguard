Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!define APPNAME "声垣服务端"
!define APPNAME_EN "SoundGuard Server"
!define COMPANY "Cheerimy Studio"
!define VERSION "0.2.0"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\SoundGuardServer"

Name "${APPNAME}"
OutFile "..\..\dist\soundguard-server-setup-${VERSION}.exe"
InstallDir "$PROGRAMFILES64\SoundGuard Server"
InstallDirRegKey HKLM "Software\SoundGuard\Server" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
Icon "..\..\client\src-tauri\icons\icon.ico"
UninstallIcon "..\..\client\src-tauri\icons\icon.ico"
BrandingText "${APPNAME} ${VERSION} · ${COMPANY}"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\start-server.bat"
!define MUI_FINISHPAGE_RUN_TEXT "立即启动声垣服务端"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!define MUI_UNCONFIRMPAGE_TEXT_TOP "卸载只会删除程序文件。数据库、录音和媒体密钥保存在 %PROGRAMDATA%\SoundGuard，不会被删除。"
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "服务端" SecServer
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "..\..\server\target\release\soundguard-server.exe"
  File "..\..\.env.example"
  File "..\..\README.md"
  File "start-server.bat"

  SetOutPath "$INSTDIR\web\dist"
  File /r "..\..\web\dist\*.*"

  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\start-server.bat"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\卸载 ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\start-server.bat"

  WriteRegStr HKLM "Software\SoundGuard\Server" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${COMPANY}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\soundguard-server.exe"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\卸载 ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$INSTDIR\soundguard-server.exe"
  Delete "$INSTDIR\.env.example"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\start-server.bat"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\web"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINST_KEY}"
  DeleteRegKey HKLM "Software\SoundGuard\Server"
SectionEnd
