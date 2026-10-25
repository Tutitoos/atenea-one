# Descarga oficial fijada: nunca se incluye una DLL en el repositorio ni en el ZIP de Atenea.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$uri = 'https://www.wintun.net/builds/wintun-0.14.1.zip'
$sha256 = '07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51'
$arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
    'X64' { 'amd64' }
    'Arm64' { 'arm64' }
    default { throw 'Solo se admiten Windows x64 y ARM64.' }
}
$work = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Join-Path $work 'wintun.zip'
    Invoke-WebRequest -Uri $uri -OutFile $zip
    if ((Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant() -ne $sha256) {
        throw 'SHA256 de Wintun incorrecto; no se instala ningún fichero.'
    }
    Expand-Archive -LiteralPath $zip -DestinationPath $work
    $dll = Join-Path $work "wintun/bin/$arch/wintun.dll"
    $signature = Get-AuthenticodeSignature -FilePath $dll
    if ($signature.Status -ne 'Valid') { throw 'La firma Authenticode de Wintun no es válida.' }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item -LiteralPath $dll -Destination (Join-Path $Destination 'wintun.dll')
    Copy-Item -LiteralPath (Join-Path $work 'wintun/LICENSE.txt') -Destination (Join-Path $Destination 'WINTUN-LICENSE.txt')
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force
}
