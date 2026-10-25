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
    $cleanup = Start-Process -FilePath (Join-Path $bin 'atenea-agent.exe') `
        -ArgumentList @('cleanup', '--config', "`"$config`"") -NoNewWindow -PassThru
    try {
        if (-not $cleanup.WaitForExit(30000)) {
            $cleanup.Kill()
            if (-not $cleanup.WaitForExit(5000)) { throw 'cleanup no terminó tras la terminación forzada.' }
            throw 'Timeout de cleanup; se conserva la instalación para poder repararla.'
        }
        if ($cleanup.ExitCode -ne 0) { throw 'Falló cleanup; se conserva la instalación para poder repararla.' }
    } finally { $cleanup.Dispose() }
}
if ($service) {
    $service.Dispose()
    $delete = Start-Process -FilePath "$env:SystemRoot/System32/sc.exe" `
        -ArgumentList @('delete', 'AteneaAgent') -NoNewWindow -PassThru
    try {
        if (-not $delete.WaitForExit(10000)) {
            $delete.Kill()
            $delete.WaitForExit(2000) | Out-Null
            throw 'Timeout al eliminar el servicio.'
        }
        if ($delete.ExitCode -ne 0) { throw 'No se pudo eliminar el servicio.' }
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
