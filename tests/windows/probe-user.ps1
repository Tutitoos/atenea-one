# Se ejecuta sin elevación; el resultado se recoge desde probe-access.ps1.
param([Parameter(Mandatory)][string]$ResultPath)
$ErrorActionPreference = 'Stop'
function Invoke-Cli([string]$Verb) {
    $stdout = "$ResultPath.$Verb.out"
    $stderr = "$ResultPath.$Verb.err"
    $process = Start-Process -FilePath (Join-Path $env:ProgramFiles 'Atenea/atenea.exe') `
        -ArgumentList @($Verb, '--socket', '\\.\pipe\atenea-status') -PassThru -NoNewWindow `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        if (-not $process.WaitForExit(5000)) {
            $process.Kill()
            if (-not $process.WaitForExit(5000)) { throw 'La CLI no terminó.' }
            throw "Timeout de la CLI: $Verb"
        }
        return @{Code=$process.ExitCode; Output=(Get-Content -LiteralPath $stdout -Raw); Error=(Get-Content -LiteralPath $stderr -Raw)}
    } finally {
        $process.Dispose()
        Remove-Item -LiteralPath $stdout,$stderr -Force -ErrorAction Continue
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
    $response = $status.Output | ConvertFrom-Json
    if ($status.Code -ne 0 -or -not ($response.PSObject.Properties.Name -contains 'dns')) {
        throw 'El usuario no pudo leer el estado.'
    }
    $reply = Invoke-Cli 'down'
    if ($reply.Code -eq 0 -or $reply.Error -notmatch 'solo se permite status') {
        throw 'La canalización de estado no rechazó stop mediante su política de solo lectura.'
    }
    @{ok=$true} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
} catch {
    @{ok=$false; error=$_.Exception.Message} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
    exit 1
}
