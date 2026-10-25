# Se ejecuta sin elevación; el resultado se recoge desde probe-access.ps1.
param([Parameter(Mandatory)][string]$ResultPath)
$ErrorActionPreference = 'Stop'
function Invoke-Cli([string]$Verb) {
    # Process.Start conserva el handle: Start-Process puede devolver ExitCode=$null (#5421).
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $env:ProgramFiles 'Atenea/atenea.exe'
    $info.Arguments = "$Verb --socket \\.\pipe\atenea-status"
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(5000)) {
            $process.Kill()
            if (-not $process.WaitForExit(5000)) { throw 'La CLI no terminó.' }
            throw "Timeout de la CLI: $Verb"
        }
        $code = $process.ExitCode
        if ($null -eq $code) { throw 'No se obtuvo un código de salida de la CLI.' }
        return @{Code=$code; Output=$stdout.GetAwaiter().GetResult(); Error=$stderr.GetAwaiter().GetResult()}
    } finally {
        $process.Dispose()
    }
}
try {
    $control = [IO.Pipes.NamedPipeClientStream]::new('.', 'atenea-control', [IO.Pipes.PipeDirection]::InOut)
    try {
        try {
            $control.Connect(5000)
            throw 'El usuario sin privilegios pudo abrir el control.'
        } catch [UnauthorizedAccessException] {
            # La DACL del objeto real impide conectar, antes de interpretar órdenes.
        }
    } finally { $control.Dispose() }
    # La CLI pide ReadData/WriteData, no GENERIC_WRITE (que incluye crear instancias).
    $status = Invoke-Cli 'status'
    if ($status.Code -ne 0) {
        throw "CLI status falló (salida $($status.Code)): stderr=$($status.Error); stdout=$($status.Output)"
    }
    $response = $status.Output | ConvertFrom-Json
    if (-not ($response.PSObject.Properties.Name -contains 'dns')) {
        throw "CLI status no devolvió un estado válido: stdout=$($status.Output)"
    }
    $reply = Invoke-Cli 'down'
    if ($reply.Code -eq 0 -or $reply.Error -notmatch 'solo se permite status') {
        throw "La canalización de estado no rechazó stop: salida=$($reply.Code); stderr=$($reply.Error); stdout=$($reply.Output)"
    }
    @{ok=$true} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
} catch {
    @{ok=$false; error=$_.Exception.Message} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
    exit 1
}
