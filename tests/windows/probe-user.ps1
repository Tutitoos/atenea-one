# Se ejecuta sin elevación; el resultado se recoge desde probe-access.ps1.
param([Parameter(Mandatory)][string]$ResultPath)
$ErrorActionPreference = 'Stop'
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
    $cli = Join-Path $env:ProgramFiles 'Atenea/atenea.exe'
    $status = & $cli status --socket '\\.\pipe\atenea-status' | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or -not ($status.PSObject.Properties.Name -contains 'dns')) {
        throw 'El usuario no pudo leer el estado.'
    }
    $ErrorActionPreference = 'Continue'
    $reply = & $cli down --socket '\\.\pipe\atenea-status' 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -eq 0 -or "$reply" -notmatch 'solo se permite status') {
        throw 'La canalización de estado no rechazó stop mediante su política de solo lectura.'
    }
    @{ok=$true} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
} catch {
    @{ok=$false; error=$_.Exception.Message} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
    exit 1
}
