# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Construye el instalador NSIS único de Windows (ADR-023) con los binarios ya compilados y la DLL oficial de Wintun
# descargada y verificada por get-wintun.ps1. No instala nada. Requiere NSIS (`makensis`).
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BinDir,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.-]+)?$') { throw "Versión inválida: $Version" }
$numeric = "$($Matches[1]).$($Matches[2]).$($Matches[3]).0"
foreach ($name in 'atenea.exe', 'atenea-agent.exe') {
    if (-not (Test-Path -LiteralPath (Join-Path $BinDir $name) -PathType Leaf)) { throw "Falta $name en $BinDir" }
}
if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -ne 'X64') { throw 'Solo se publica Windows x64.' }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$out = (Resolve-Path -LiteralPath $OutDir).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    & (Join-Path $PSScriptRoot 'get-wintun.ps1') -Destination $work
    $commit = if ($env:ATENEA_BUILD_COMMIT) { $env:ATENEA_BUILD_COMMIT } else { 'unknown' }
    $info = Join-Path $work 'BUILD-INFO.json'
    @{ commit = $commit; version = $Version } | ConvertTo-Json | Set-Content -Encoding utf8 $info
    # Sin «setup», «install» ni «update» en el nombre: Windows aplica a esos ejecutables heurísticas de compatibilidad.
    $installer = Join-Path $out "atenea-one-$Version-windows-amd64.exe"
    # Chocolatey instala NSIS en Program Files (x86) y no actualiza el PATH de la sesión en curso.
    $found = Get-Command makensis -ErrorAction SilentlyContinue
    $makensis = if ($found) { $found.Source } else { Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe' }
    if (-not (Test-Path -LiteralPath $makensis)) { throw 'No se encuentra makensis (NSIS)' }
    & $makensis -V2 "-DVERSION=$Version" "-DVIVERSION=$numeric" "-DBINDIR=$((Resolve-Path -LiteralPath $BinDir).Path)" `
        "-DWINTUNDIR=$work" "-DBUILDINFO=$info" "-DOUTFILE=$installer" (Join-Path $PSScriptRoot 'installer.nsi')
    if ($LASTEXITCODE) { throw "makensis falló ($LASTEXITCODE)" }
    Write-Output $installer
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force
}
