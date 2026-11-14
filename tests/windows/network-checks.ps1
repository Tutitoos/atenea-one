# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
# `atenea network up/down` (ADR-017 fase 2) con el cliente ya instalado. SOLO para la VM desechable de CI: crea y borra
# un servicio AteneaNetwork-<id8> y los directorios de una red de prueba en %ProgramData%\Atenea.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Solo se permite la VM desechable de GitHub Actions.' }
$bin = Join-Path $env:ProgramFiles 'Atenea'
$data = Join-Path $env:ProgramData 'Atenea'
$cli = Join-Path $bin 'atenea.exe'
$netId = '00000000-0000-4000-8000-0000abcd1234'
$short = 'abcd1234'
$svc = "AteneaNetwork-$short"
$adapter = "atn_$short"
$dir = Join-Path $data "networks\$netId"
$device = Join-Path $dir 'device'
$public = Join-Path $data 'public'
$statusPipe = "\\.\pipe\atenea-$short-status"
function Assert-True($Condition, [string]$Message) { if (-not $Condition) { throw "FALLO: $Message" } }
function Wait-Until([scriptblock]$Check, [int]$Seconds = 40) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) { if (& $Check) { return $true }; Start-Sleep -Milliseconds 300 }
    return $false
}
function Invoke-Cli {
    # En PowerShell 5.1 el stderr de una orden nativa con `2>&1` y ErrorActionPreference=Stop es una excepción.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $cli @args 2>&1 | Out-String; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $previous }
    if ($code) { throw "atenea $args salió con ${code}: $out" }
    return $out
}
function Service-Info { Get-CimInstance Win32_Service -Filter "Name='$svc'" }

try {
    Assert-True (-not (Service-Info)) 'el servicio de prueba ya existe'
    # Red de prueba: claves nuevas, un agent.toml mínimo con tuberías por red y su entrada en el índice público.
    New-Item -ItemType Directory -Force -Path $device, $public | Out-Null
    $rootKey = Join-Path $env:RUNNER_TEMP 'net-root.key'
    $anchor = (Invoke-Cli root-init --out $rootKey).Trim()
    $deviceOut = Invoke-Cli device-init --dir $device
    $deviceId = [regex]::Match($deviceOut, 'device_id=([0-9a-f]+)').Groups[1].Value
    Assert-True ($deviceId.Length -eq 32) "device_id: $deviceOut"
    $toml = { param($p) $p.Replace('\', '/') }
    $config = Join-Path $device 'agent.toml'
    $text = @"
interface = "$adapter"
listen = "127.0.0.1:51899"
ipc_socket = '\\.\pipe\atenea-$short-control'
status_socket = '$statusPipe'
status_gid = 545
noise_key_file = "$(& $toml (Join-Path $device 'noise.key'))"
identity_key_file = "$(& $toml (Join-Path $device 'identity.key'))"
config_file = "$(& $toml (Join-Path $device 'config.bin'))"
state_dir = "$(& $toml (Join-Path $device 'state'))"
network_id = "$($netId.Replace('-', ''))"
device_id = "$deviceId"
trust_anchor = "$anchor"
"@
    [IO.File]::WriteAllText($config, $text, [Text.UTF8Encoding]::new($false))
    icacls $config /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
    if ($LASTEXITCODE) { throw 'icacls' }
    $index = @{ network_id = $netId; name = 'prueba-up'; server = 'control.example.test'; url = 'https://control.example.test'; root_anchor_fingerprint = 'f'; device_id = $deviceId; device_fingerprint = 'p'; interface = $adapter; listen_port = 51899; created = 1; join_status = 'aprobada' } | ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $public "$netId.json"), $index, [Text.UTF8Encoding]::new($false))

    # up: crea el servicio por red (LocalSystem, automático retrasado, recuperación 5/15/60 s) y lo inicia.
    Invoke-Cli network up $short | Out-Null
    $s = Service-Info
    Assert-True ($null -ne $s) 'up no creó el servicio'
    Assert-True ($s.StartName -eq 'LocalSystem' -and $s.State -eq 'Running' -and $s.StartMode -eq 'Auto') "servicio: $($s.StartName)/$($s.State)/$($s.StartMode)"
    Assert-True ($s.DisplayName -eq "Atenea One ($short)") "nombre visible: $($s.DisplayName)"
    $expected = "`"$bin\atenea-agent.exe`" run --service --config $config"
    Assert-True ($s.PathName -ieq $expected) "línea de órdenes: $($s.PathName) (esperada: $expected)"
    $reg = Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$svc"
    Assert-True ($reg.DelayedAutostart -eq 1) 'falta el arranque automático retrasado'
    $failure = & "$env:SystemRoot/System32/sc.exe" qfailure $svc | Out-String
    Assert-True ($failure -match '5000' -and $failure -match '15000' -and $failure -match '60000') "recuperación SCM: $failure"
    # El agente contesta por su tubería de estado y el adaptador lleva como GUID el UUID de la red.
    Assert-True (Wait-Until { try { Invoke-Cli status --socket $statusPipe | Out-Null; $true } catch { $false } }) 'la tubería de estado no responde'
    $nic = Get-NetAdapter -Name $adapter -IncludeHidden -ErrorAction SilentlyContinue
    Assert-True ($null -ne $nic -and $nic.InterfaceGuid -ieq "{$netId}") "adaptador: $($nic | Out-String)"
    $listed = @(Invoke-Cli network list --json | ConvertFrom-Json)
    Assert-True (@($listed)[0].state -eq 'iniciada_sin_configuracion') "list tras up: $(Invoke-Cli network list)"
    Invoke-Cli network up $short | Out-Null  # idempotente

    # down: para, espera la limpieza y deshabilita; up lo vuelve a habilitar.
    Invoke-Cli network down $short | Out-Null
    $s = Service-Info
    Assert-True ($s.State -eq 'Stopped' -and $s.StartMode -eq 'Disabled') "tras down: $($s.State)/$($s.StartMode)"
    Assert-True (-not (Get-NetAdapter -Name $adapter -IncludeHidden -ErrorAction SilentlyContinue)) 'down dejó el adaptador'
    Assert-True ((Invoke-Cli network list) -match 'detenida') 'list tras down'
    Invoke-Cli network down $short | Out-Null  # idempotente
    Invoke-Cli network up $short | Out-Null
    $s = Service-Info
    Assert-True ($s.State -eq 'Running' -and $s.StartMode -eq 'Auto') "tras up: $($s.State)/$($s.StartMode)"
    Invoke-Cli network down $short | Out-Null

    # Un servicio con ese nombre y otra línea de órdenes no se toca.
    & "$env:SystemRoot/System32/sc.exe" config $svc binPath= "`"$env:SystemRoot\System32\cmd.exe`"" | Out-Null
    $refused = $false
    try { Invoke-Cli network up $short | Out-Null } catch { $refused = $_.Exception.Message -match 'otra línea de órdenes' }
    Assert-True $refused 'up debía rechazar un servicio con otra línea de órdenes'
    Write-Output 'NETWORK UP/DOWN: OK'
} finally {
    if (Service-Info) {
        & "$env:SystemRoot/System32/sc.exe" stop $svc | Out-Null
        [void](Wait-Until { (Service-Info).State -eq 'Stopped' } 40)
        & "$env:SystemRoot/System32/sc.exe" delete $svc | Out-Null
    }
    Remove-Item $dir, $public -Recurse -Force -ErrorAction SilentlyContinue
}
