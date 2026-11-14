; Copyright 2026 Tutitoos
; SPDX-License-Identifier: Apache-2.0
; Instalador único de Windows (ADR-023): agente, CLI y Wintun, por equipo (administrador). Instala sin arrancar y sin
; configurar: la identidad y la configuración las crea el usuario en %ProgramData%\Atenea. Las comprobaciones
; privilegiadas están en installer-ops.ps1 (las mismas que install.ps1/uninstall.ps1).
;
; Variables de compilación (/D): VERSION, VIVERSION (a.b.c.d), BINDIR (atenea.exe y atenea-agent.exe), WINTUNDIR
; (wintun.dll y WINTUN-LICENSE.txt de get-wintun.ps1), BUILDINFO, OUTFILE.
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WordFunc.nsh"
!include "FileFunc.nsh"

!define APP_NAME "Atenea"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\AteneaOne"
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

Var WasRunning
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

Function .onInit
    SetRegView 64
    SetShellVarContext all

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
    StrCpy $WasRunning "0"
    ${If} $Existing != ""
        !insertmacro Ops Stop ""
        ${If} $0 == "running"
            StrCpy $WasRunning "1"
        ${EndIf}
    ${EndIf}
    !insertmacro Ops Prepare ""
    SetOutPath "$INSTDIR"
    File "/oname=atenea.exe" "${BINDIR}\atenea.exe"
    File "/oname=atenea-agent.exe" "${BINDIR}\atenea-agent.exe"
    File "/oname=wintun.dll" "${WINTUNDIR}\wintun.dll"
    File "/oname=WINTUN-LICENSE.txt" "${WINTUNDIR}\WINTUN-LICENSE.txt"
    File "/oname=BUILD-INFO.json" "${BUILDINFO}"
    WriteUninstaller "$INSTDIR\uninstall.exe"

    !insertmacro Ops Register ""
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "Tutitoos"
    WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
    ; Solo se vuelve a arrancar el servicio si estaba en marcha antes de actualizar.
    ${If} $WasRunning == "1"
        !insertmacro Ops Start ""
    ${EndIf}
SectionEnd

Function un.onInit
    SetRegView 64
    SetShellVarContext all
FunctionEnd

; Sin /PURGE se conservan los datos (identidad, claves, configuración) de %ProgramData%\Atenea.
Section "Uninstall"
    ${un.GetParameters} $2
    ${un.GetOptions} $2 "/PURGE" $3
    StrCpy $4 ""
    ${IfNot} ${Errors}
        StrCpy $4 "-Purge"
    ${EndIf}
    ClearErrors
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer-ops.ps1" -Action Remove -Bin "$INSTDIR" -Data "$APPDATA\Atenea" $4'
    Pop $1
    Pop $0
    ${If} $1 != 0
        MessageBox MB_OK|MB_ICONSTOP "No se pudo retirar el servicio (código $1); se conserva la instalación.$\r$\n$\r$\n$0" /SD IDOK
        SetErrorLevel 2
        Abort
    ${EndIf}
    Delete "$INSTDIR\atenea.exe"
    Delete "$INSTDIR\atenea-agent.exe"
    Delete "$INSTDIR\wintun.dll"
    Delete "$INSTDIR\WINTUN-LICENSE.txt"
    Delete "$INSTDIR\BUILD-INFO.json"
    Delete "$INSTDIR\installer-ops.ps1"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKLM "${UNINST_KEY}"
SectionEnd
