# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Construye el instalador NSIS de la aplicación de escritorio a partir del binario ya compilado. No instala nada.
# Requiere NSIS (`makensis` en el PATH o en Program Files (x86), donde lo deja Chocolatey; en CI, a una versión fija).
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.-]+)?$') { throw "Versión inválida: $Version" }
$numeric = "$($Matches[1]).$($Matches[2]).$($Matches[3]).0"
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "Falta el ejecutable $Binary" }
$arch = switch ($env:PROCESSOR_ARCHITECTURE) { 'AMD64' { 'amd64' } 'ARM64' { 'arm64' } default { throw "Arquitectura no admitida: $env:PROCESSOR_ARCHITECTURE" } }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$out = (Resolve-Path -LiteralPath $OutDir).Path
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$work = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $commit = if ($env:ATENEA_BUILD_COMMIT) { $env:ATENEA_BUILD_COMMIT } else { 'unknown' }
    $info = Join-Path $work 'BUILD-INFO.json'
    @{ commit = $commit; version = $Version } | ConvertTo-Json | Set-Content -Encoding utf8 $info
    # Sin «setup», «install» ni «update» en el nombre: Windows aplica a esos ejecutables heurísticas de compatibilidad.
    $installer = Join-Path $out "atenea-desktop-$Version-windows-$arch.exe"
    # Chocolatey instala NSIS en Program Files (x86) y no actualiza el PATH de la sesión en curso.
    $found = Get-Command makensis -ErrorAction SilentlyContinue
    $makensis = if ($found) { $found.Source } else { Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe' }
    if (-not (Test-Path -LiteralPath $makensis)) { throw 'No se encuentra makensis (NSIS)' }
    & $makensis -V2 "-DVERSION=$Version" "-DVIVERSION=$numeric" "-DBINARY=$((Resolve-Path -LiteralPath $Binary).Path)" `
        "-DICON=$repo\apps\desktop\icons\icon.ico" "-DBUILDINFO=$info" "-DOUTFILE=$installer" (Join-Path $PSScriptRoot 'installer.nsi')
    if ($LASTEXITCODE) { throw "makensis falló ($LASTEXITCODE)" }
    Write-Output $installer
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force
}
