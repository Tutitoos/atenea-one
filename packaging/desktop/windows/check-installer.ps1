# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Instala el instalador NSIS en silencio (por usuario, sin privilegios), arranca la aplicación instalada con la prueba de
# humo y la desinstala. SOLO para un runner desechable: toca el perfil del usuario y el registro de HKCU.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Installer)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Atenea One'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AteneaOneDesktop'
if (Test-Path $dir) { throw "$dir ya existe: este script no pisa instalaciones previas" }

$p = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "El instalador salió con $($p.ExitCode)" }
foreach ($f in 'atenea-desktop.exe', 'BUILD-INFO.json', 'uninstall.exe') {
    if (-not (Test-Path (Join-Path $dir $f))) { throw "Falta $f tras instalar" }
}
if (-not (Test-Path $key)) { throw 'Falta la entrada de desinstalación en HKCU' }
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Atenea One.lnk'
if (-not (Test-Path $shortcut)) { throw 'Falta el acceso directo del menú Inicio' }

$py = if (Get-Command python3 -ErrorAction SilentlyContinue) { 'python3' } else { 'python' }
& $py "$repo\apps\desktop\scripts\smoke.py" 120 (Join-Path $dir 'atenea-desktop.exe')

# `_?=` hace que el desinstalador espere en lugar de copiarse a %TEMP% y volver al instante.
$u = Start-Process -FilePath (Join-Path $dir 'uninstall.exe') -ArgumentList '/S', "_?=$dir" -Wait -PassThru
if ($u.ExitCode -ne 0) { throw "El desinstalador salió con $($u.ExitCode)" }
foreach ($f in 'atenea-desktop.exe', 'BUILD-INFO.json') {
    if (Test-Path (Join-Path $dir $f)) { throw "$f sigue instalado tras desinstalar" }
}
if (Test-Path $key) { throw 'La entrada de desinstalación sigue en HKCU' }
if (Test-Path $shortcut) { throw 'El acceso directo sigue en el menú Inicio' }
Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
Write-Output 'INSTALADOR: OK (instalar, arrancar, desinstalar)'
