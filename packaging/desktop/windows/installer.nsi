; Copyright 2026 Tutitoos
; SPDX-License-Identifier: Apache-2.0
; Instalador NSIS de la aplicación de escritorio. Se instala POR USUARIO (sin UAC ni privilegios): la aplicación no
; tiene privilegios (ADR-019). No incluye el agente ni el CLI (paquete propio, ADR-017) ni el arranque de WebView2:
; si el runtime no está, el instalador lo dice y se detiene en lugar de descargar un binario sin verificar.
;
; Variables de compilación (/D): VERSION, VIVERSION (a.b.c.d), BINARY, ICON, BUILDINFO, OUTFILE.
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

!define APP_NAME "Atenea One"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\AteneaOneDesktop"
; Identificador del runtime «Evergreen» de WebView2 en EdgeUpdate.
!define WEBVIEW2_KEY "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define WEBVIEW2_KEY_32 "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APP_NAME} ${VERSION}"
VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "Instalador de ${APP_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright 2026 Tutitoos"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Spanish"

; WebView2 está si algún de las dos vistas del registro (por equipo o por usuario) trae una versión distinta de 0.0.0.0.
Function CheckWebView2
    ReadRegStr $0 HKLM "${WEBVIEW2_KEY}" "pv"
    ${If} $0 == ""
        ReadRegStr $0 HKLM "${WEBVIEW2_KEY_32}" "pv"
    ${EndIf}
    ${If} $0 == ""
        ReadRegStr $0 HKCU "${WEBVIEW2_KEY}" "pv"
    ${EndIf}
    ${If} $0 == ""
    ${OrIf} $0 == "0.0.0.0"
        MessageBox MB_OK|MB_ICONSTOP "Falta el runtime WebView2 de Microsoft, que ${APP_NAME} necesita para mostrar su ventana.$\r$\n$\r$\nInstálalo desde https://developer.microsoft.com/microsoft-edge/webview2/ y vuelve a ejecutar este instalador." /SD IDOK
        SetErrorLevel 3
        Abort
    ${EndIf}
FunctionEnd

Section "Instalar"
    Call CheckWebView2
    SetOutPath "$INSTDIR"
    File "/oname=atenea-desktop.exe" "${BINARY}"
    File "/oname=BUILD-INFO.json" "${BUILDINFO}"
    WriteUninstaller "$INSTDIR\uninstall.exe"
    CreateShortCut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\atenea-desktop.exe" "--open" "$INSTDIR\atenea-desktop.exe" 0
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "Tutitoos"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\atenea-desktop.exe"
    WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

; Solo quita lo que instaló: las preferencias del usuario (notifications.json) y su identidad de red no se tocan.
Section "Uninstall"
    Delete "$SMPROGRAMS\${APP_NAME}.lnk"
    Delete "$INSTDIR\atenea-desktop.exe"
    Delete "$INSTDIR\BUILD-INFO.json"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKCU "${UNINST_KEY}"
SectionEnd
