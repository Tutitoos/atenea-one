param([string]$PeerLog)
$ErrorActionPreference = 'Continue'
function Show-Diagnostic([string]$Name, [scriptblock]$Read, [switch]$Local) {
    Write-Output "=== $Name ==="
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
    $process = $null
    try {
        $process = [Diagnostics.Process]::Start($info)
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(2000)) {
            Write-Output 'Timeout de diagnóstico (2s).'
            $process.Kill()
            if (-not $process.WaitForExit(250)) { Write-Output 'El diagnóstico no confirmó su terminación.' }
        }
        [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 100) | Out-Null
        if ($stdout.IsCompleted) { Write-Output $stdout.Result }
        if ($stderr.IsCompleted) { Write-Output $stderr.Result }
    } catch { Write-Output ($_ | Out-String) }
    finally { if ($process) { $process.Dispose() } }
}
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
