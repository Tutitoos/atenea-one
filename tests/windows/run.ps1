#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Solo se permite la VM desechable de GitHub Actions.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$zip = Join-Path $repo 'target/windows-package/atenea-client-unsigned.zip'
$stage = Join-Path $repo 'target/windows-install'
$windowsPS = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
$failure = $null
$cleanupFailures = @()
function Invoke-Bounded([string]$Command, [int]$Seconds) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $windowsPS
    $info.UseShellExecute = $false
    $info.Arguments = '-NoProfile -NonInteractive -OutputFormat Text -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("`$ProgressPreference='SilentlyContinue'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new(`$false); `$ErrorActionPreference='Stop'; " + $Command))
    $info.EnvironmentVariables['PSModulePath'] = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/Modules'
    $info.EnvironmentVariables.Remove('PSHOME')
    $info.EnvironmentVariables.Remove('POWERSHELL_DISTRIBUTION_CHANNEL')
    $process = [Diagnostics.Process]::Start($info)
    try {
        if (-not $process.WaitForExit(($Seconds - 5) * 1000)) {
            # Mata el árbol, incluidos auxiliares Python bloqueados en ctypes o pipes.
            $killer = Start-Process -FilePath "$env:SystemRoot/System32/taskkill.exe" -ArgumentList @('/PID', $process.Id, '/T', '/F') -PassThru -NoNewWindow
            try {
                if (-not $killer.WaitForExit(2000)) { $killer.Kill(); $killer.WaitForExit(1000) | Out-Null }
            } finally { $killer.Dispose() }
            if (-not $process.WaitForExit(2000)) { throw "No terminó el árbol tras el watchdog: $Command" }
            throw "Watchdog de ${Seconds}s: $Command"
        }
        if ($process.ExitCode -ne 0) { throw "Salida $($process.ExitCode): $Command" }
    } finally { $process.Dispose() }
}
function Show-FinalDiagnostics {
    Write-Output "Registro del agente: $env:ProgramData/Atenea/agent.log"
    try { Invoke-Bounded "& '$PSScriptRoot/diagnostics.ps1'" 25 }
    catch { Write-Warning "Falló la recogida de diagnóstico: $_" }
}
if (Test-Path -LiteralPath $stage) {
    Show-FinalDiagnostics
    throw 'El directorio de instalación de prueba ya existe.'
}
try {
    # Un solo plazo cubre la recogida PS5.1 real y todos los lectores, antes de instalar.
    Invoke-Bounded "& '$PSScriptRoot/json-fixtures.ps1' -Out '$repo/target/windows-json-fixtures.json'; & python -u '$PSScriptRoot/check_ps51_fixtures.py' '$repo/target/windows-json-fixtures.json'; if (`$LASTEXITCODE -ne 0) {exit `$LASTEXITCODE}" 30
    Expand-Archive -LiteralPath $zip -DestinationPath $stage
    Invoke-Bounded "& '$stage/install.ps1'" 90
    Invoke-Bounded "& python -u '$PSScriptRoot/smoke.py'; if (`$LASTEXITCODE -ne 0) {exit `$LASTEXITCODE}" 360
} catch {
    $failure = $_
    Write-Output ($_ | Format-List * -Force | Out-String)
} finally {
    Show-FinalDiagnostics
    # El watchdog puede haber matado Python antes de su finally; SCM no es hijo suyo.
    try {
        Invoke-Bounded '$s=Get-Service AteneaAgent -ErrorAction SilentlyContinue; if ($null -eq $s) {exit 0}; if ($s.Status -ne "Stopped") {$s.Stop(); $s.WaitForStatus("Stopped", [TimeSpan]::FromSeconds(30))}' 40
    } catch {
        $cleanupFailures += $_
        Write-Warning "Fallo de parada SCM: $_"
        Show-FinalDiagnostics
        try {
            Invoke-Bounded '$s=Get-CimInstance Win32_Service -Filter "Name=''AteneaAgent''"; if ($null -ne $s -and $s.ProcessId -gt 0) {$p=Get-Process -Id $s.ProcessId -ErrorAction SilentlyContinue; if ($null -ne $p) {$p.Kill(); if (-not $p.WaitForExit(10000)) {throw "El agente no terminó"}}}' 20
        } catch { $cleanupFailures += $_; Write-Warning "Fallo de terminación del agente: $_" }
    }
    try { if (Test-Path -LiteralPath "$stage/uninstall.ps1") { Invoke-Bounded "& '$stage/uninstall.ps1' -Purge" 60 } }
    catch { $cleanupFailures += $_; Write-Warning "Falló la desinstalación: $_" }
    Show-FinalDiagnostics
}
if ($failure) { throw $failure }
if (@($cleanupFailures).Count -gt 0) { throw ('Falló la limpieza: ' + ($cleanupFailures -join '; ')) }
if (Get-Service AteneaAgent -ErrorAction SilentlyContinue) { throw 'Quedó registrado el servicio.' }
if (Test-Path -LiteralPath (Join-Path $env:ProgramFiles 'Atenea')) { throw 'Quedaron binarios instalados.' }
if (Test-Path -LiteralPath (Join-Path $env:ProgramData 'Atenea')) { throw 'Quedaron claves, configuración o estado tras -Purge.' }
