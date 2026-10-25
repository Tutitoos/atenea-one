param([string]$PeerLog)
$ErrorActionPreference = 'Continue'
$diagnosticClock = [Diagnostics.Stopwatch]::StartNew()
function Show-Diagnostic([string]$Name, [scriptblock]$Read, [switch]$Local, [string]$Executable, [string]$Arguments) {
    Write-Output "=== $Name ==="
    $remaining = 23000 - [int]$diagnosticClock.ElapsedMilliseconds
    if ($remaining -lt 750) { Write-Output 'Plazo global de diagnóstico agotado.'; return }
    if ($Local) {
        try { & $Read | Out-String -Width 240 | Write-Output } catch { Write-Output ($_ | Out-String) }
        return
    }
    # Cada consulta nativa tiene su propio plazo; una bloqueada no salta las siguientes.
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.EnvironmentVariables['PSModulePath'] = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/Modules'
    $info.EnvironmentVariables.Remove('PSHOME')
    $info.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("& { $Read } | Out-String -Width 240"))
    if ($Executable) {
        $info.FileName = $Executable
        $info.Arguments = $Arguments
    }
    $process = $null
    try {
        $process = [Diagnostics.Process]::Start($info)
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit([Math]::Min(2000, $remaining - 750))) {
            Write-Output 'Timeout de diagnóstico (máximo 2s).'
            $previousPreference = $ErrorActionPreference
            try {
                $ErrorActionPreference = 'Stop'
                if (-not $process.HasExited) { $process.Kill() }
                if (-not $process.WaitForExit(250)) { Write-Output 'El diagnóstico no confirmó su terminación.' }
            } catch { Write-Output "No se pudo terminar el diagnóstico; se continúa: $_" }
            finally { $ErrorActionPreference = $previousPreference }
        }
        try { [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 100) | Out-Null }
        catch { Write-Output "Falló la captura del diagnóstico; se continúa: $_" }
        foreach ($stream in @($stdout, $stderr)) {
            if ($stream.Status -eq [Threading.Tasks.TaskStatus]::RanToCompletion) {
                Write-Output $stream.GetAwaiter().GetResult()
            } elseif ($stream.IsFaulted) {
                Write-Output "Falló la lectura del diagnóstico: $($stream.Exception)"
            } elseif ($stream.IsCanceled) {
                Write-Output 'Lectura de diagnóstico cancelada.'
            } else {
                Write-Output 'Captura incompleta; no se espera fuera del plazo.'
            }
        }
    } catch { Write-Output ($_ | Out-String) }
    finally { if ($process) { $process.Dispose() } }
}
# El mismo lector nativo que smoke imprime ambos objetos incluso si falla antes del probe.
try {
    Show-Diagnostic 'propietario, DACL, MIC y máscaras ACE de ambos pipes activos' -Executable (Get-Command python -ErrorAction Stop).Source -Arguments ('-u "{0}" --pipe-sddl' -f (Join-Path $PSScriptRoot 'smoke.py'))
} catch { Write-Output "No se pudo iniciar el lector de descriptores; se continúa: $_" }
$agentLog = Join-Path $env:ProgramData 'Atenea/agent.log'
Write-Output "Registro del agente: $agentLog"
Show-Diagnostic 'agent.log' { if (Test-Path -LiteralPath $agentLog) { Get-Content -LiteralPath $agentLog -Tail 200 } else { 'No existe todavía.' } } -Local
Show-Diagnostic 'peer.log' {
    if ($PeerLog) { Get-Content -LiteralPath $PeerLog -Tail 200 -ErrorAction Stop }
    else { Get-ChildItem -Path "$env:TEMP/atenea-windows-*/peer.log" -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName; Get-Content -LiteralPath $_.FullName -Tail 200 } }
} -Local
Show-Diagnostic 'SCM estado y códigos de salida' { Get-CimInstance Win32_Service -Filter "Name='AteneaAgent'" | Select-Object Name,State,Status,AcceptStop,ProcessId,ExitCode,ServiceSpecificExitCode,CheckPoint,WaitHint,PathName | Format-List }
Show-Diagnostic 'ServiceController' { Get-Service AteneaAgent -ErrorAction Stop | Format-List Name,Status,CanStop,ServiceType }
Show-Diagnostic 'adaptadores' { Get-NetAdapter -IncludeHidden | Format-Table -AutoSize }
Show-Diagnostic 'direcciones' { Get-NetIPAddress | Format-Table -AutoSize }
Show-Diagnostic 'rutas' { Get-NetRoute | Format-Table -AutoSize }
Show-Diagnostic 'NRPT' { Get-DnsClientNrptRule | Format-List * }
Show-Diagnostic 'Event Log Service Control Manager' { Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Service Control Manager'; StartTime=(Get-Date).AddMinutes(-30)} -MaxEvents 50 -ErrorAction Stop | Format-List TimeCreated,Id,LevelDisplayName,Message }
Show-Diagnostic 'Event Log Application Atenea' { Get-WinEvent -FilterHashtable @{LogName='Application'; StartTime=(Get-Date).AddMinutes(-30)} -MaxEvents 100 -ErrorAction Stop | Where-Object {$_.ProviderName -match 'Atenea' -or $_.Message -match 'atenea-agent'} | Format-List TimeCreated,Id,ProviderName,Message }
