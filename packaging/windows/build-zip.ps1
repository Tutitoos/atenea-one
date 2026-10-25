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
    $zip = Join-Path $out 'atenea-client-unsigned.zip'
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
    Write-Output $zip
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
