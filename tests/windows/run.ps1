#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Solo se permite la VM desechable de GitHub Actions.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$zip = Join-Path $repo 'target/windows-package/atenea-client-unsigned.zip'
$stage = Join-Path $repo 'target/windows-install'
if (Test-Path -LiteralPath $stage) { throw 'El directorio de instalación de prueba ya existe.' }
Expand-Archive -LiteralPath $zip -DestinationPath $stage
& (Join-Path $stage 'install.ps1')
try {
    & python (Join-Path $PSScriptRoot 'smoke.py')
    if ($LASTEXITCODE -ne 0) { throw 'Falló la aceptación privilegiada de Windows.' }
} finally {
    & (Join-Path $stage 'uninstall.ps1') -Purge
}
if (Get-Service AteneaAgent -ErrorAction SilentlyContinue) { throw 'Quedó registrado el servicio.' }
if (Test-Path -LiteralPath (Join-Path $env:ProgramFiles 'Atenea')) { throw 'Quedaron binarios instalados.' }
if (Test-Path -LiteralPath (Join-Path $env:ProgramData 'Atenea')) { throw 'Quedaron claves, configuración o estado tras -Purge.' }
