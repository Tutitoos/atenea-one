; Copyright 2026 Tutitoos
; SPDX-License-Identifier: Apache-2.0
; Instalador único de Windows (ADR-023): agente, CLI, aplicación de escritorio y Wintun, por equipo (administrador). No
; registra ningún servicio ni configura nada: cada red crea el suyo con `atenea network join/up` (ADR-017). Un servicio
; heredado `AteneaAgent` de una instalación anterior se respeta. Las comprobaciones privilegiadas están en installer-ops.ps1.
;
; Variables de compilación (/D): VERSION, VIVERSION (a.b.c.d), BINDIR (atenea.exe y atenea-agent.exe), APPBIN
; (atenea-desktop.exe), WINTUNDIR (wintun.dll y WINTUN-LICENSE.txt de get-wintun.ps1), BUILDINFO, OUTFILE.
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WordFunc.nsh"
!include "FileFunc.nsh"
!include "TextFunc.nsh"

!define APP_NAME "Atenea"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\AteneaOne"
!define APP_SHORTCUT "Atenea One"
; Identificador del runtime «Evergreen» de WebView2 en EdgeUpdate.
!define WEBVIEW2_KEY "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define WEBVIEW2_KEY_32 "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define PS '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer-ops.ps1"'

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Atenea"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${APP_NAME} ${VERSION}"
VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "Instalador de ${APP_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright 2026 Tutitoos"

!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Spanish"
!insertmacro GetOptions
!insertmacro un.GetOptions
!insertmacro TrimNewLines

Var Running
Var Existing
!insertmacro un.GetParameters

; Muestra el motivo (nada en modo silencioso, pero sí código de salida 2) y detiene la instalación.
!macro Fail text
    MessageBox MB_OK|MB_ICONSTOP "${text}" /SD IDOK
    ; En silencio no hay ventana: el motivo queda en %TEMP% (de quien lanzó el instalador) para quien lo automatiza.
    ${If} ${Silent}
        FileOpen $9 "$TEMP\atenea-one-installer.log" a
        FileWrite $9 "${text}$\r$\n"
        FileClose $9
    ${EndIf}
    SetErrorLevel 2
    Abort
!macroend

; Ejecuta una acción de installer-ops.ps1; en fallo, muestra su salida y se detiene. Deja la salida en $0.
!macro Ops action extra
    nsExec::ExecToStack '${PS} -Action ${action} -Bin "$INSTDIR" -Data "$APPDATA\Atenea" ${extra}'
    Pop $1
    Pop $0
    ${If} $1 != 0
        ; Una instalación nueva a medias no queda como «anterior» que el siguiente intento rechazaría.
        ${If} $Existing == ""
            RMDir /r "$INSTDIR"
        ${EndIf}
        !insertmacro Fail "Falló la operación ${action} (código $1).$\r$\n$\r$\n$0"
    ${EndIf}
!macroend

; Quien lanza el instalador desde PowerShell 7 deja en el entorno un PSModulePath que el Windows PowerShell 5.1 hijo hereda
; y con el que no carga Microsoft.PowerShell.Security (`Set-Acl` desaparece). Se borra: el 5.1 recompone el suyo.
!macro ResetModulePath
    System::Call 'Kernel32::SetEnvironmentVariable(t "PSModulePath", p 0)i'
!macroend

Function .onInit
    SetRegView 64
    SetShellVarContext all
    !insertmacro ResetModulePath

    ReadRegStr $Existing HKLM "${UNINST_KEY}" "DisplayVersion"
    ${If} $Existing != ""
        ReadRegStr $INSTDIR HKLM "${UNINST_KEY}" "InstallLocation"
    ${EndIf}
    nsExec::Exec '"$SYSDIR\sc.exe" query AteneaAgent'
    Pop $1
    ${If} $Existing == ""
        ; Un servicio o una carpeta que este instalador no creó (install.ps1 antiguo): no se adopta.
        ${If} $1 == 0
        ${OrIf} ${FileExists} "$INSTDIR\*.*"
            !insertmacro Fail "Hay una instalación anterior hecha con install.ps1. Ejecute su uninstall.ps1 (sin -Purge conserva la identidad) y vuelva a lanzar este instalador."
        ${EndIf}
    ${Else}
        ${VersionCompare} "${VERSION}" $Existing $2
        ${If} $2 == 2
            !insertmacro Fail "Ya está instalada la versión $Existing, posterior a ${VERSION}. No se admite volver a una versión anterior."
        ${EndIf}
    ${EndIf}
FunctionEnd

; WebView2 es del sistema, no de Atenea: si falta, el cliente y los servicios funcionan igual y solo la ventana no abre.
; Se avisa sin detener la instalación y sin descargar nada.
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
        MessageBox MB_OK|MB_ICONEXCLAMATION "Falta el runtime WebView2 de Microsoft, que la aplicación necesita para mostrar su ventana. El cliente ya está instalado.$\r$\n$\r$\nInstálelo desde https://developer.microsoft.com/microsoft-edge/webview2/" /SD IDOK
        ${If} ${Silent}
            FileOpen $9 "$TEMP\atenea-one-installer.log" a
            FileWrite $9 "Falta WebView2: la aplicación no abrirá su ventana hasta instalarlo.$\r$\n"
            FileClose $9
        ${EndIf}
    ${EndIf}
FunctionEnd

Section "Instalar"
    SetRegView 64
    SetShellVarContext all
    ; El asistente no se extrae a %TEMP% (un usuario podría sustituirlo antes de ejecutarse elevado): se crea el directorio
    ; con la DACL protegida y se ejecuta desde allí. En una actualización el directorio ya es protegido.
    ${If} $Existing == ""
        CreateDirectory "$INSTDIR"
        nsExec::ExecToStack '"$SYSDIR\icacls.exe" "$INSTDIR" /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F'
        Pop $1
        Pop $0
        ${If} $1 != 0
            RMDir /r "$INSTDIR"
            !insertmacro Fail "No se pudo proteger $INSTDIR (código $1).$\r$\n$\r$\n$0"
        ${EndIf}
    ${EndIf}
    SetOutPath "$INSTDIR"
    File "/oname=installer-ops.ps1" "installer-ops.ps1"
    !insertmacro Ops Check ""
    StrCpy $Running ""
    ${If} $Existing != ""
        !insertmacro Ops Stop ""
        ${TrimNewLines} "$0" $Running
    ${EndIf}
    !insertmacro Ops Prepare ""
    SetOutPath "$INSTDIR"
    File "/oname=atenea.exe" "${BINDIR}\atenea.exe"
    File "/oname=atenea-agent.exe" "${BINDIR}\atenea-agent.exe"
    File "/oname=atenea-desktop.exe" "${APPBIN}"
    File "/oname=wintun.dll" "${WINTUNDIR}\wintun.dll"
    File "/oname=WINTUN-LICENSE.txt" "${WINTUNDIR}\WINTUN-LICENSE.txt"
    File "/oname=BUILD-INFO.json" "${BUILDINFO}"
    WriteUninstaller "$INSTDIR\uninstall.exe"

    !insertmacro Ops Protect ""
    ; Acceso directo para todos los usuarios; la aplicación no tiene privilegios (ADR-019) y se abre sin elevar.
    CreateShortCut "$SMPROGRAMS\${APP_SHORTCUT}.lnk" "$INSTDIR\atenea-desktop.exe" "--open" "$INSTDIR\atenea-desktop.exe" 0
    Call CheckWebView2
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\atenea-desktop.exe"
    WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "Tutitoos"
    WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
    ; Solo se vuelven a arrancar los servicios que estaban en marcha antes de actualizar.
    ${If} $Running != ""
        !insertmacro Ops Start "-Names $Running"
    ${EndIf}
SectionEnd

Function un.onInit
    SetRegView 64
    SetShellVarContext all
    !insertmacro ResetModulePath
FunctionEnd

; Sin /PURGE se conservan los datos (identidades, claves, configuración) de %ProgramData%\Atenea. Los servicios de las
; redes quedan parados y deshabilitados. /PURGE borra los datos y rechaza hacerlo si siguen unidas redes, salvo /FORCE.
Section "Uninstall"
    ${un.GetParameters} $2
    StrCpy $4 ""
    ${un.GetOptions} $2 "/PURGE" $3
    ${IfNot} ${Errors}
        StrCpy $4 "-Purge"
    ${EndIf}
    ClearErrors
    ${un.GetOptions} $2 "/FORCE" $3
    ${IfNot} ${Errors}
        StrCpy $4 "$4 -Force"
    ${EndIf}
    ClearErrors
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer-ops.ps1" -Action Remove -Bin "$INSTDIR" -Data "$APPDATA\Atenea" $4'
    Pop $1
    Pop $0
    ${If} $1 != 0
        MessageBox MB_OK|MB_ICONSTOP "No se pudo desinstalar (código $1); se conserva la instalación.$\r$\n$\r$\n$0" /SD IDOK
        ; En silencio no hay ventana: el motivo queda en %TEMP% para quien lo automatiza.
        ${If} ${Silent}
            FileOpen $9 "$TEMP\atenea-one-installer.log" a
            FileWrite $9 "$0$\r$\n"
            FileClose $9
        ${EndIf}
        SetErrorLevel 2
        Abort
    ${EndIf}
    Delete "$INSTDIR\atenea.exe"
    Delete "$INSTDIR\atenea-agent.exe"
    Delete "$INSTDIR\atenea-desktop.exe"
    Delete "$SMPROGRAMS\${APP_SHORTCUT}.lnk"
    Delete "$INSTDIR\wintun.dll"
    Delete "$INSTDIR\WINTUN-LICENSE.txt"
    Delete "$INSTDIR\BUILD-INFO.json"
    Delete "$INSTDIR\installer-ops.ps1"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKLM "${UNINST_KEY}"
SectionEnd
