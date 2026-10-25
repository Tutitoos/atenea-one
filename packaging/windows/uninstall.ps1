#Requires -RunAsAdministrator
[CmdletBinding()]
param([switch]$Purge)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$bin = Join-Path $env:ProgramFiles 'Atenea'
$config = Join-Path $env:ProgramData 'Atenea/agent.toml'
$service = Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue
if ($service) {
    if ($service.Status -ne 'Stopped') {
        Stop-Service -Name AteneaAgent
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }
}
if (Test-Path -LiteralPath $config) {
    & (Join-Path $bin 'atenea-agent.exe') cleanup --config $config
    if ($LASTEXITCODE -ne 0) { throw 'Falló cleanup; se conserva la instalación para poder repararla.' }
}
if ($service) {
    & sc.exe delete AteneaAgent | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'No se pudo eliminar el servicio.' }
}
Remove-Item -LiteralPath $bin -Recurse -Force
if ($Purge) {
    $data = Join-Path $env:ProgramData 'Atenea'
    if (Test-Path -LiteralPath $data) { Remove-Item -LiteralPath $data -Recurse -Force }
    Write-Output 'Desinstalación completa: eliminados servicio, binarios, configuración, claves y estado.'
} else {
    Write-Output 'Servicio y binarios retirados; identidad conservada en ProgramData\Atenea. Use -Purge para eliminar también los datos de forma irreversible.'
}
