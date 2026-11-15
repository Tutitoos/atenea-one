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
$appExe = Join-Path $bin 'atenea-desktop.exe'
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Atenea One.lnk'

function Invoke-Setup([string]$Path, [string[]]$Arguments) {
    # `_?=` hace que el desinstalador espere en lugar de copiarse a %TEMP% y volver al instante.
    $p = Start-Process -FilePath $Path -ArgumentList $Arguments -Wait -PassThru
    $log = Join-Path $env:TEMP 'atenea-one-installer.log'
    if ($p.ExitCode -ne 0 -and (Test-Path $log)) { Write-Host "Salida $($p.ExitCode) de $Path; registro:"; Write-Host ((Get-Content $log) -join "`n"); Remove-Item $log }
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
foreach ($f in 'atenea.exe', 'atenea-agent.exe', 'atenea-desktop.exe', 'WINTUN-LICENSE.txt', 'BUILD-INFO.json', 'uninstall.exe') {
    Assert-True (Test-Path (Join-Path $bin $f)) "falta $f"
}
# Solo SYSTEM y Administradores controlan los datos (el SDDL de ADR-017/023).
$sddl = (Get-Acl -LiteralPath $data).GetSecurityDescriptorSddlForm('Owner,Group,Access')
Assert-True ($sddl -match '^O:BAG:BAD:P(AI)?\(A;OICI;FA;;;SY\)\(A;OICI;FA;;;BA\)$') "DACL de ProgramData: $sddl"
# El instalador ya no registra ningún servicio (ADR-017 §9) y los usuarios pueden ejecutar la CLI, no sustituirla.
Assert-True (-not (Get-Service AteneaAgent -ErrorAction SilentlyContinue)) 'el instalador registró AteneaAgent'
Assert-True (-not (Get-Service 'AteneaNetwork-*' -ErrorAction SilentlyContinue)) 'el instalador registró un servicio de red'
Assert-True ((Get-Acl -LiteralPath $bin).Sddl -match 'D:P.*\(A;[^)]*;;;BU\)') "DACL de Archivos de programa: $((Get-Acl -LiteralPath $bin).Sddl)"
# La aplicación: ejecutable en la carpeta protegida, acceso directo para todos los usuarios y el mismo inventario.
Assert-True (Test-Path $shortcut) 'falta el acceso directo del menú Inicio'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
Assert-True ($link.TargetPath -eq $appExe -and $link.Arguments -eq '--open') "acceso directo: $($link.TargetPath) $($link.Arguments)"
$appHash = (Get-FileHash -Algorithm SHA256 $appExe).Hash
Assert-True ((Get-ItemProperty $key).DisplayIcon -eq $appExe) 'la entrada de desinstalación no apunta al icono de la aplicación'
Assert-True ((& (Join-Path $bin 'atenea.exe') network legacy --json) -match '"present":false') '`network legacy` veía una instalación heredada que no existe'
# Una instalación anterior (que sí registraba AteneaAgent) se respeta en las actualizaciones: se simula registrándolo.
& "$PSScriptRoot/register-legacy.ps1"
$service = Get-CimInstance Win32_Service -Filter "Name='AteneaAgent'"
Assert-True ($service.StartName -eq 'LocalSystem' -and $service.StartMode -eq 'Manual' -and $service.State -eq 'Stopped') "servicio: $($service.StartName)/$($service.StartMode)/$($service.State)"
$failure = & "$env:SystemRoot/System32/sc.exe" qfailure AteneaAgent | Out-String
Assert-True ($failure -match '5000' -and $failure -match '15000' -and $failure -match '60000') "recuperación SCM: $failure"
Assert-True ((Get-ItemProperty $key).DisplayVersion -ne $null) 'falta la entrada de desinstalación'

# `atenea network` (ADR-017): sin redes, `list` lo dice; con un índice público, lo lista como detenida (sin servicio);
# el servicio AteneaAgent instalado cuenta como instalación heredada.
$cli = Join-Path $bin 'atenea.exe'
Assert-True ((& $cli network list) -match 'No hay redes unidas') '`network list` sin redes'
$public = Join-Path $data 'public'
$netId = '00000000-0000-4000-8000-00001111aaaa'
New-Item -ItemType Directory -Force -Path $public | Out-Null
$index = @{ network_id = $netId; name = 'prueba'; server = 'control.example.test'; url = 'https://control.example.test'; root_anchor_fingerprint = 'f'; device_id = 'd'; device_fingerprint = 'p'; interface = 'atn_1111aaaa'; listen_port = 51820; created = 1; join_status = 'aprobada' } | ConvertTo-Json
# Sin BOM: el lector JSON del CLI no lo admite y Set-Content -Encoding utf8 lo escribe en PowerShell 5.1.
[IO.File]::WriteAllText((Join-Path $public "$netId.json"), $index, [Text.UTF8Encoding]::new($false))
$listed = @(& $cli network list --json | ConvertFrom-Json)
$first = @($listed)[0]
Assert-True ($first.name -eq 'prueba' -and $first.state -eq 'detenida' -and $first.backend -eq 'scm') "`network list --json`: $($first | ConvertTo-Json -Compress)"
Remove-Item $public -Recurse -Force
Assert-True ((& $cli network legacy --json) -match '"present":true') '`network legacy` no vio el servicio AteneaAgent'

# Una versión anterior no sustituye a la instalada.
$before = (Get-ItemProperty $key).DisplayVersion
Assert-True ((Install $OlderInstaller) -eq 2) 'debía rechazar el retroceso de versión'
Assert-True ((Get-ItemProperty $key).DisplayVersion -eq $before) 'el rechazo cambió la versión'

# Actualización en el sitio: conserva datos y deja el servicio como estaba (parado). Con la aplicación en marcha
# (un ejecutable en uso no se sustituye) la cierra y deja la copia nueva; la simula un `ping` renombrado.
$marker = Join-Path $data 'conservar.txt'
Set-Content -LiteralPath $marker -Value 'x'
Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\PING.EXE') -Destination $appExe -Force
$fake = Start-Process -FilePath $appExe -ArgumentList '-n', '120', '127.0.0.1' -PassThru -WindowStyle Hidden
Assert-True ((Install $Installer) -eq 0) 'actualización en el sitio con la aplicación en marcha'
Assert-True ($fake.HasExited) 'la actualización no cerró la aplicación en marcha'
Assert-True ((Get-FileHash -Algorithm SHA256 $appExe).Hash -eq $appHash) 'la actualización no dejó la aplicación nueva'
Assert-True (Test-Path $marker) 'la actualización borró datos'
Assert-True ((Get-Service AteneaAgent).Status -eq 'Stopped') 'la actualización arrancó un servicio parado'

# Desinstalar conserva los datos; /PURGE los borra.
Assert-True ((Uninstall @()) -eq 0) 'desinstalación'
Assert-True (-not (Test-Path $appExe) -and -not (Test-Path $shortcut)) 'la desinstalación dejó la aplicación o su acceso directo'
Remove-Item (Join-Path $bin 'uninstall.exe') -ErrorAction SilentlyContinue
Remove-Item $bin -Recurse -Force -ErrorAction SilentlyContinue
Assert-True (Test-Path $marker) 'la desinstalación borró datos sin /PURGE'
Assert-True (-not (Get-Service AteneaAgent -ErrorAction SilentlyContinue)) 'el servicio sigue registrado'
Assert-True (-not (Test-Path $key)) 'sigue la entrada de desinstalación'
Remove-Item $marker

Assert-True ((Install $Installer) -eq 0) 'reinstalación'
Write-Output 'INSTALADOR WINDOWS: OK'
