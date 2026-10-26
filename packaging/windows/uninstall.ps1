# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
[CmdletBinding()]
param([switch]$Purge)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$bin = Join-Path $env:ProgramFiles 'Atenea'
$config = Join-Path $env:ProgramData 'Atenea/agent.toml'
$service = Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue
if ($service) {
    # Una salida con error durante la retirada no debe provocar un reinicio que compita con la limpieza.
    # Process.Start pasa la línea tal cual (PowerShell 5.1 descarta el argumento vacío de actions=).
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = "$env:SystemRoot/System32/sc.exe"
    $info.Arguments = 'failure AteneaAgent reset= 0 actions= ""'
    $info.UseShellExecute = $false
    $recovery = [Diagnostics.Process]::Start($info)
    try {
        if (-not $recovery.WaitForExit(10000)) { $recovery.Kill(); throw 'Timeout al desactivar la recuperación SCM.' }
        if ($recovery.ExitCode -ne 0) { throw "No se pudo desactivar la recuperación SCM (salida $($recovery.ExitCode))." }
    } finally { $recovery.Dispose() }
    if ($service.Status -ne 'Stopped') {
        try {
            # Stop() evita el bucle de espera sin plazo de Stop-Service.
            $service.Stop()
            $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
        } catch {
            $failure = $_
            Write-Warning ($failure | Format-List * -Force | Out-String)
            try { Write-Warning (Get-CimInstance Win32_Service -Filter "Name='AteneaAgent'" | Format-List Name,State,AcceptStop,ProcessId,ExitCode,ServiceSpecificExitCode,CheckPoint,WaitHint | Out-String) }
            catch { Write-Warning "Falló el diagnóstico SCM: $_" }
            throw $failure
        }
    }
}
if (Test-Path -LiteralPath $config) {
    # Process.Start conserva el handle y ExitCode; Start-Process falla con #5421.
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $bin 'atenea-agent.exe'
    $info.Arguments = 'cleanup --config "{0}"' -f $config
    $info.UseShellExecute = $false
    $cleanup = [Diagnostics.Process]::Start($info)
    try {
        if (-not $cleanup.WaitForExit(30000)) {
            $cleanup.Kill()
            if (-not $cleanup.WaitForExit(5000)) { throw 'cleanup no terminó tras la terminación forzada.' }
            throw 'Timeout de cleanup; se conserva la instalación para poder repararla.'
        }
        $code = $cleanup.ExitCode
        if ($null -eq $code -or $code -ne 0) { throw "Falló cleanup (salida '$code'); se conserva la instalación para poder repararla." }
    } finally { $cleanup.Dispose() }
}
if ($service) {
    $service.Dispose()
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = "$env:SystemRoot/System32/sc.exe"
    $info.Arguments = 'delete AteneaAgent'
    $info.UseShellExecute = $false
    $delete = [Diagnostics.Process]::Start($info)
    try {
        if (-not $delete.WaitForExit(10000)) {
            $delete.Kill()
            $delete.WaitForExit(2000) | Out-Null
            throw 'Timeout al eliminar el servicio.'
        }
        $code = $delete.ExitCode
        if ($null -eq $code -or $code -ne 0) { throw "No se pudo eliminar el servicio (salida '$code'); se conserva la instalación." }
    } finally { $delete.Dispose() }
}
Remove-Item -LiteralPath $bin -Recurse -Force
if ($Purge) {
    $data = Join-Path $env:ProgramData 'Atenea'
    if (Test-Path -LiteralPath $data) { Remove-Item -LiteralPath $data -Recurse -Force }
    Write-Output 'Desinstalación completa: eliminados servicio, binarios, configuración, claves y estado.'
} else {
    Write-Output 'Servicio y binarios retirados; identidad conservada en ProgramData\Atenea. Use -Purge para eliminar también los datos de forma irreversible.'
}
