# Solo construye el ZIP: no instala ni descarga controladores.
[CmdletBinding()]
param([string]$BinDir = (Join-Path $PSScriptRoot '../../target/release'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$out = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target/windows-package'))
New-Item -ItemType Directory -Force -Path $out | Out-Null
$stage = Join-Path $out ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    foreach ($name in @('atenea.exe', 'atenea-agent.exe')) {
        Copy-Item -LiteralPath (Join-Path $BinDir $name) -Destination $stage
    }
    foreach ($name in @('install.ps1', 'uninstall.ps1', 'get-wintun.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../../docs/operations/instalacion-windows.md') -Destination $stage
    $commit = if ($env:ATENEA_BUILD_COMMIT) { $env:ATENEA_BUILD_COMMIT } else { 'unknown' }
    $version = if ($env:ATENEA_VERSION) { $env:ATENEA_VERSION } else { '0.1.0' }
    @{ commit = $commit; version = $version } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $stage 'BUILD-INFO.json')
    $zip = Join-Path $out 'atenea-client-unsigned.zip'
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
    Write-Output $zip
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
