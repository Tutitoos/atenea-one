# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
# Comprobaciones del instalador único (ADR-023) antes de la aceptación del agente. SOLO para la VM desechable de CI:
# instala y desinstala en %ProgramFiles% y %ProgramData%. Termina con Atenea INSTALADO, sin arrancar, para el smoke.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$OlderInstaller
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Solo se permite la VM desechable de GitHub Actions.' }
$bin = Join-Path $env:ProgramFiles 'Atenea'
$data = Join-Path $env:ProgramData 'Atenea'
$key = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AteneaOne'

function Invoke-Setup([string]$Path, [string[]]$Arguments) {
    # `_?=` hace que el desinstalador espere en lugar de copiarse a %TEMP% y volver al instante.
    $p = Start-Process -FilePath $Path -ArgumentList $Arguments -Wait -PassThru
    return $p.ExitCode
}
function Assert-True($Condition, [string]$Message) { if (-not $Condition) { throw "FALLO: $Message" } }
function Install([string]$Path) { Invoke-Setup $Path @('/S') }
function Uninstall([string[]]$Extra) { Invoke-Setup (Join-Path $bin 'uninstall.exe') (@('/S') + $Extra + @("_?=$bin")) }

Assert-True (-not (Test-Path $bin) -and -not (Test-Path $data) -and -not (Get-Service AteneaAgent -ErrorAction SilentlyContinue)) 'el runner no está limpio'

# Negativo: una carpeta que este instalador no creó (instalación antigua con install.ps1) no se adopta.
New-Item -ItemType Directory -Path $bin | Out-Null
Assert-True ((Install $Installer) -eq 2) 'debía rechazar una instalación previa no gestionada'
Assert-True (-not (Get-Service AteneaAgent -ErrorAction SilentlyContinue)) 'no debía registrar el servicio al rechazar'
Remove-Item $bin -Recurse -Force

Assert-True ((Install $Installer) -eq 0) 'instalación limpia'
# DLL oficial sin modificar: firma Authenticode válida y licencia junto a ella.
$dll = Join-Path $bin 'wintun.dll'
$signature = Get-AuthenticodeSignature -FilePath $dll
Assert-True ($signature.Status -eq 'Valid') "firma de wintun.dll: $($signature.Status)"
Write-Output "wintun.dll SHA-256 $((Get-FileHash -Algorithm SHA256 $dll).Hash)"
foreach ($f in 'atenea.exe', 'atenea-agent.exe', 'WINTUN-LICENSE.txt', 'BUILD-INFO.json', 'uninstall.exe') {
    Assert-True (Test-Path (Join-Path $bin $f)) "falta $f"
}
# Solo SYSTEM y Administradores controlan los datos (el SDDL de ADR-017/023).
$sddl = (Get-Acl -LiteralPath $data).GetSecurityDescriptorSddlForm('Owner,Group,Access')
Assert-True ($sddl -match '^O:BAG:BAD:P(AI)?\(A;OICI;FA;;;SY\)\(A;OICI;FA;;;BA\)$') "DACL de ProgramData: $sddl"
$service = Get-CimInstance Win32_Service -Filter "Name='AteneaAgent'"
Assert-True ($service.StartName -eq 'LocalSystem' -and $service.StartMode -eq 'Manual' -and $service.State -eq 'Stopped') "servicio: $($service.StartName)/$($service.StartMode)/$($service.State)"
$failure = & "$env:SystemRoot/System32/sc.exe" qfailure AteneaAgent | Out-String
Assert-True ($failure -match '5000' -and $failure -match '15000' -and $failure -match '60000') "recuperación SCM: $failure"
Assert-True ((Get-ItemProperty $key).DisplayVersion -ne $null) 'falta la entrada de desinstalación'

# Una versión anterior no sustituye a la instalada.
$before = (Get-ItemProperty $key).DisplayVersion
Assert-True ((Install $OlderInstaller) -eq 2) 'debía rechazar el retroceso de versión'
Assert-True ((Get-ItemProperty $key).DisplayVersion -eq $before) 'el rechazo cambió la versión'

# Actualización en el sitio: conserva datos y deja el servicio como estaba (parado).
$marker = Join-Path $data 'conservar.txt'
Set-Content -LiteralPath $marker -Value 'x'
Assert-True ((Install $Installer) -eq 0) 'actualización en el sitio'
Assert-True (Test-Path $marker) 'la actualización borró datos'
Assert-True ((Get-Service AteneaAgent).Status -eq 'Stopped') 'la actualización arrancó un servicio parado'

# Desinstalar conserva los datos; /PURGE los borra.
Assert-True ((Uninstall @()) -eq 0) 'desinstalación'
Remove-Item (Join-Path $bin 'uninstall.exe') -ErrorAction SilentlyContinue
Remove-Item $bin -Recurse -Force -ErrorAction SilentlyContinue
Assert-True (Test-Path $marker) 'la desinstalación borró datos sin /PURGE'
Assert-True (-not (Get-Service AteneaAgent -ErrorAction SilentlyContinue)) 'el servicio sigue registrado'
Assert-True (-not (Test-Path $key)) 'sigue la entrada de desinstalación'
Remove-Item $marker

Assert-True ((Install $Installer) -eq 0) 'reinstalación'
Write-Output 'INSTALADOR WINDOWS: OK'
