# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
# `atenea network up/down` (ADR-017 fase 2) con el cliente ya instalado. SOLO para la VM desechable de CI: crea y borra
# un servicio AteneaNetwork-<id8> y los directorios de una red de prueba en %ProgramData%\Atenea.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Installer)
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
function Service-Info-Of([string]$Name) { Get-CimInstance Win32_Service -Filter "Name='$Name'" }
function Service-Info { Service-Info-Of $svc }
$script:servers = @()

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

    # up: crea el servicio por red (LocalSystem, inicio manual con disparador de primera IP, recuperación 5/15/60 s) y lo inicia.
    Invoke-Cli network up $short | Out-Null
    $s = Service-Info
    Assert-True ($null -ne $s) 'up no creó el servicio'
    Assert-True ($s.StartName -eq 'LocalSystem' -and $s.State -eq 'Running' -and $s.StartMode -eq 'Manual') "servicio: $($s.StartName)/$($s.State)/$($s.StartMode)"
    Assert-True ($s.DisplayName -eq "Atenea One ($short)") "nombre visible: $($s.DisplayName)"
    $expected = "`"$bin\atenea-agent.exe`" run --service --config $config"
    Assert-True ($s.PathName -ieq $expected) "línea de órdenes: $($s.PathName) (esperada: $expected)"
    Assert-True ((Get-Item "HKLM:\SYSTEM\CurrentControlSet\Services\$svc").GetValue('DelayedAutostart') -ne 1) 'quedó la marca de arranque automático retrasado'
    $trigger = & "$env:SystemRoot/System32/sc.exe" qtriggerinfo $svc | Out-String
    Assert-True ($trigger -match '4f27f2de-14e2-430b-a549-7cd48cbc8245') "falta el disparador de primera IP (ADR-033): $trigger"
    $failure = & "$env:SystemRoot/System32/sc.exe" qfailure $svc | Out-String
    Assert-True ($failure -match '5000' -and $failure -match '15000' -and $failure -match '60000') "recuperación SCM: $failure"
    # El agente contesta por su tubería de estado y el adaptador lleva como GUID el UUID de la red.
    Assert-True (Wait-Until { try { Invoke-Cli status --socket $statusPipe | Out-Null; $true } catch { $false } }) 'la tubería de estado no responde'
    $nic = Get-NetAdapter -Name $adapter -IncludeHidden -ErrorAction SilentlyContinue
    Assert-True ($null -ne $nic -and $nic.InterfaceGuid -ieq "{$netId}") "adaptador: $($nic | Out-String)"
    $listed = @(Invoke-Cli network list --json | ConvertFrom-Json)
    Assert-True (@($listed)[0].state -eq 'iniciada_sin_configuracion') "list tras up: $(Invoke-Cli network list)"
    Invoke-Cli network up $short | Out-Null  # idempotente
    # Un servicio de una versión anterior (automático retrasado) se migra en el siguiente up (ADR-033).
    & "$env:SystemRoot/System32/sc.exe" config $svc start= delayed-auto | Out-Null
    & "$env:SystemRoot/System32/sc.exe" triggerinfo $svc delete | Out-Null
    Invoke-Cli network up $short | Out-Null
    $s = Service-Info
    $delayed = (Get-Item "HKLM:\SYSTEM\CurrentControlSet\Services\$svc").GetValue('DelayedAutostart')
    Assert-True ($s.StartMode -eq 'Manual' -and $delayed -ne 1) "migración: $($s.StartMode)/$delayed"
    $trigger = & "$env:SystemRoot/System32/sc.exe" qtriggerinfo $svc | Out-String
    Assert-True ($trigger -match '4f27f2de-14e2-430b-a549-7cd48cbc8245') "migración sin disparador: $trigger"

    # down: para, espera la limpieza y deshabilita; up lo vuelve a habilitar.
    Invoke-Cli network down $short | Out-Null
    $s = Service-Info
    Assert-True ($s.State -eq 'Stopped' -and $s.StartMode -eq 'Disabled') "tras down: $($s.State)/$($s.StartMode)"
    Assert-True (-not (Get-NetAdapter -Name $adapter -IncludeHidden -ErrorAction SilentlyContinue)) 'down dejó el adaptador'
    Assert-True ((Invoke-Cli network list) -match 'detenida') 'list tras down'
    Invoke-Cli network down $short | Out-Null  # idempotente
    Invoke-Cli network up $short | Out-Null
    $s = Service-Info
    Assert-True ($s.State -eq 'Running' -and $s.StartMode -eq 'Manual') "tras up: $($s.State)/$($s.StartMode)"
    Invoke-Cli network down $short | Out-Null

    # Un servicio con ese nombre y otra línea de órdenes no se toca.
    & "$env:SystemRoot/System32/sc.exe" config $svc binPath= "`"$env:SystemRoot\System32\cmd.exe`"" | Out-Null
    $refused = $false
    # Solo texto ASCII: la consola de PowerShell 5.1 decodifica la salida UTF-8 del CLI con la página OEM.
    try { Invoke-Cli network up $short | Out-Null } catch { $refused = $_.Exception.Message -match 'ya existe con otra' }
    Assert-True $refused 'up debía rechazar un servicio con otra línea de órdenes'
    Write-Output 'NETWORK UP/DOWN: OK'

    # ---- join / leave contra un control plane falso, con DOS redes a la vez (aceptación de la fase 2) ----
    # Se retira la red de prueba manual: este bloque parte de un cliente sin redes.
    & "$env:SystemRoot/System32/sc.exe" delete $svc | Out-Null
    Remove-Item $dir, $public -Recurse -Force
    Assert-True ((Invoke-Cli network list) -match 'No hay redes unidas') 'quedaron redes tras retirar la de prueba'
    $repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $maker = Join-Path $repo 'target\debug\examples\make_join_code.exe'
    $python = if (Get-Command python3 -ErrorAction SilentlyContinue) { 'python3' } else { 'python' }
    $nets = @(
        @{ Id = '00000000-0000-4000-8000-0000aaaa1111'; Short = 'aaaa1111'; Port = 18091 },
        @{ Id = '00000000-0000-4000-8000-0000bbbb2222'; Short = 'bbbb2222'; Port = 18092 })
    foreach ($n in $nets) {
        $n.Log = Join-Path $env:RUNNER_TEMP "fake-control-$($n.Short).log"
        $script:servers += @(Start-Process -FilePath $python -ArgumentList @((Join-Path $PSScriptRoot 'fake_control.py'), $n.Port, $n.Id.Replace('-', ''), $n.Log) -PassThru -WindowStyle Hidden)
        $n.Alias = "atn_$($n.Short)"
        $n.Service = "AteneaNetwork-$($n.Short)"
        $n.Pipe = "\\.\pipe\atenea-$($n.Short)-status"
    }
    foreach ($n in $nets) {
        $port = $n.Port
        Assert-True (Wait-Until { try { $c = [Net.Sockets.TcpClient]::new('127.0.0.1', $port); $c.Close(); $true } catch { $false } }) "el control plane falso no escucha en $port"
    }
    $sddlSids = { param($path) ((Get-Acl -LiteralPath $path).Sddl | Select-String -AllMatches '\(A;[^)]*;;;([^;)]+)\)').Matches | ForEach-Object { $_.Groups[1].Value } | Sort-Object }

    foreach ($n in $nets) {
        $code = (& $maker "https://127.0.0.1:$($n.Port)" $n.Id.Replace('-', '') "red-$($n.Short)" 'token-sintetico' | Out-String).Trim()
        $out = Invoke-Cli network join $code --yes --insecure-http --name equipo-prueba
        Assert-True ($out -match 'estado=aprobada') "join $($n.Short): $out"
        $cfg = Join-Path $data "networks\$($n.Id)\device\agent.toml"
        $toml = Get-Content -LiteralPath $cfg -Raw
        Assert-True ($toml -match "atenea-$($n.Short)-control" -and $toml -match "atenea-$($n.Short)-status" -and $toml -match "interface = `"$($n.Alias)`"") "agent.toml de $($n.Short): $toml"
        Assert-True (((& $sddlSids $cfg) -join ',') -eq 'BA,SY' -and (Get-Acl -LiteralPath $cfg).Sddl -match 'D:P') "DACL de agent.toml de $($n.Short): $((Get-Acl -LiteralPath $cfg).Sddl)"
        $index = Join-Path $public "$($n.Id).json"
        Assert-True ((Get-Acl -LiteralPath $index).Sddl -match 'D:P.*FR;;;BU\)') "DACL del índice público: $((Get-Acl -LiteralPath $index).Sddl)"
        Assert-True ((Get-Content -LiteralPath $n.Log -Raw) -match 'POST /api/v1/enroll') "el control plane falso no recibió el enrolamiento de $($n.Short)"
        $text = (Get-Content -LiteralPath $index -Raw)
        Assert-True ($text -notmatch 'token-sintetico') 'el índice público no debe contener el token'
    }
    # Repetir la unión no cambia nada; el puerto de cada red es distinto.
    $again = (& $maker "https://127.0.0.1:$($nets[0].Port)" $nets[0].Id.Replace('-', '') 'otra' 'token-sintetico' | Out-String).Trim()
    $refused = $false
    try { Invoke-Cli network join $again --yes --insecure-http --name equipo-prueba | Out-Null } catch { $refused = $_.Exception.Message -match 'unido a la red' }
    Assert-True $refused 'una segunda unión a la misma red debía rechazarse'
    # PowerShell 5.1 entrega la lista JSON como un único objeto array: sin @(...) para no envolverla otra vez.
    $listed = Invoke-Cli network list --json | ConvertFrom-Json
    Assert-True (@($listed).Count -eq 2) "list: se esperaban 2 redes: $(Invoke-Cli network list)"
    $ports = @($listed | ForEach-Object { $_.listen_port })
    Assert-True ($ports[0] -ne $ports[1] -and @($ports | Where-Object { $_ -lt 51820 }).Count -eq 0) "puertos: $ports"
    foreach ($n in $nets) { Assert-True (-not (Service-Info-Of $n.Service)) "el servicio $($n.Service) existe antes de up" }

    # Las dos redes a la vez: dos servicios, dos adaptadores con su GUID, dos tuberías.
    foreach ($n in $nets) { Invoke-Cli network up $n.Short | Out-Null }
    foreach ($n in $nets) {
        $s = Service-Info-Of $n.Service
        Assert-True ($s.State -eq 'Running' -and $s.StartMode -eq 'Manual') "servicio $($n.Service): $($s.State)/$($s.StartMode)"
        $pipe = $n.Pipe
        Assert-True (Wait-Until { try { Invoke-Cli status --socket $pipe | Out-Null; $true } catch { $false } }) "la tubería $pipe no responde"
        $nic = Get-NetAdapter -Name $n.Alias -IncludeHidden -ErrorAction SilentlyContinue
        Assert-True ($null -ne $nic -and $nic.InterfaceGuid -ieq "{$($n.Id)}") "adaptador de $($n.Short): $($nic | Out-String)"
    }
    # down de una red no toca la otra.
    Invoke-Cli network down $nets[0].Short | Out-Null
    $a = Service-Info-Of $nets[0].Service
    $b = Service-Info-Of $nets[1].Service
    Assert-True ($a.State -eq 'Stopped' -and $a.StartMode -eq 'Disabled' -and $b.State -eq 'Running') "tras down de A: $($a.State)/$($a.StartMode) y B $($b.State)"
    Assert-True (-not (Get-NetAdapter -Name $nets[0].Alias -IncludeHidden -ErrorAction SilentlyContinue)) 'down de A dejó su adaptador'
    Assert-True ($null -ne (Get-NetAdapter -Name $nets[1].Alias -IncludeHidden -ErrorAction SilentlyContinue)) 'down de A quitó el adaptador de B'
    $pipeB = $nets[1].Pipe
    Invoke-Cli status --socket $pipeB | Out-Null
    Invoke-Cli network up $nets[0].Short | Out-Null

    # leave de B (con revocación en el servidor) no toca A.
    Invoke-Cli network leave $nets[1].Short --yes | Out-Null
    Assert-True ((Get-Content -LiteralPath $nets[1].Log -Raw) -match 'POST /api/v1/device/leave') 'leave no revocó el dispositivo en el servidor'
    Assert-True (-not (Service-Info-Of $nets[1].Service)) 'leave dejó el servicio de B'
    Assert-True (-not (Test-Path (Join-Path $data "networks\$($nets[1].Id)"))) 'leave dejó las claves de B'
    Assert-True (-not (Test-Path (Join-Path $public "$($nets[1].Id).json"))) 'leave dejó el índice de B'
    Assert-True (-not (Get-NetAdapter -Name $nets[1].Alias -IncludeHidden -ErrorAction SilentlyContinue)) 'leave dejó el adaptador de B'
    Assert-True ((Service-Info-Of $nets[0].Service).State -eq 'Running') 'leave de B paró A'
    # leave --offline de A estando en marcha: la para, limpia y borra; no habla con el servidor.
    $before = (Get-Content -LiteralPath $nets[0].Log -Raw)
    Invoke-Cli network leave $nets[0].Short --yes --offline | Out-Null
    Assert-True ((Get-Content -LiteralPath $nets[0].Log -Raw) -eq $before) 'leave --offline contactó con el servidor'
    Assert-True (-not (Service-Info-Of $nets[0].Service) -and -not (Test-Path (Join-Path $data "networks\$($nets[0].Id)"))) 'leave --offline dejó restos de A'
    Assert-True ((Invoke-Cli network list) -match 'No hay redes unidas') 'quedaron redes tras los leave'
    Write-Output 'NETWORK JOIN/LEAVE: OK'

    # ---- la aplicación (ADR-017 §8): canal de resultado del CLI y elevación real de la app ----
    $resultDir = Join-Path $data 'ui-results'
    $nonceOk = [guid]::NewGuid().ToString('N')
    Invoke-Cli network list --result $nonceOk | Out-Null
    $resultFile = Join-Path $resultDir "$nonceOk\result.json"
    $ok = Get-Content -LiteralPath $resultFile -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($ok.ok -eq $true) "result.json de una orden correcta: $(Get-Content -LiteralPath $resultFile -Raw)"
    # Lo escribe el lado elevado: solo SYSTEM y Administradores escriben, los usuarios leen.
    foreach ($p in @($resultFile, (Split-Path $resultFile))) {
        $sddl = (Get-Acl -LiteralPath $p).Sddl
        Assert-True ($sddl -match 'D:P' -and $sddl -match ';;;BU\)' -and $sddl -notmatch '(FA|FW|GA|GW|0x1f01ff)[^;]*;;;BU\)') "DACL de ${p}: $sddl"
    }
    $nonceBad = [guid]::NewGuid().ToString('N')
    $failed = $false
    try { Invoke-Cli network up 00000000-0000-4000-8000-00000000dead --result $nonceBad | Out-Null } catch { $failed = $true }
    Assert-True $failed 'up de una red inexistente debía fallar'
    $bad = Get-Content -LiteralPath (Join-Path $resultDir "$nonceBad\result.json") -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($bad.ok -eq $false -and $bad.message -match 'ninguna red') "result.json de un fallo: $($bad | ConvertTo-Json -Compress)"
    $invalid = $false
    try { Invoke-Cli network list --result ABC | Out-Null } catch { $invalid = $true }
    Assert-True ($invalid -and -not (Test-Path (Join-Path $resultDir 'ABC'))) 'un nonce inválido debía rechazarse sin crear nada'

    # La app de verdad (Go): comprobación del CLI, powershell Start-Process -Verb RunAs y result.json. En el runner UAC no
    # pide confirmación; el diálogo y su cancelación solo se pueden probar en un Windows real.
    $acc = $nets[0]
    $accCode = (& $maker "https://127.0.0.1:$($acc.Port)" $acc.Id.Replace('-', '') "red-$($acc.Short)" 'token-sintetico' | Out-String).Trim()
    Invoke-Cli network join $accCode --yes --insecure-http --name equipo-prueba | Out-Null
    Push-Location (Join-Path $repo 'apps\desktop')
    try {
        $env:ATENEA_ACCEPTANCE_NETWORK = $acc.Id
        $previous = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try { $goOut = & go test ./internal/backend -run TestWindowsElevationAcceptance -count=1 -v 2>&1 | Out-String; $goCode = $LASTEXITCODE } finally { $ErrorActionPreference = $previous }
    } finally {
        Remove-Item Env:ATENEA_ACCEPTANCE_NETWORK -ErrorAction SilentlyContinue
        Pop-Location
    }
    Assert-True ($goCode -eq 0 -and $goOut -match 'PASS: TestWindowsElevationAcceptance') "go test: $goOut"
    Assert-True ((Invoke-Cli network list) -match 'No hay redes unidas') 'la prueba de la aplicación dejó redes unidas'
    Write-Output 'NETWORK APLICACION: OK'

    # ---- actualizar y desinstalar con redes unidas (ADR-017 §9, ADR-023) ----
    function Setup([string]$Path, [string[]]$Arguments) {
        $p = Start-Process -FilePath $Path -ArgumentList $Arguments -Wait -PassThru
        $log = Join-Path $env:TEMP 'atenea-one-installer.log'
        if ($p.ExitCode -ne 0 -and (Test-Path $log)) { Write-Host ((Get-Content $log) -join "`n"); Remove-Item $log }
        return $p.ExitCode
    }
    function Uninstall-Atenea([string[]]$Extra) {
        $code = Setup (Join-Path $bin 'uninstall.exe') (@('/S') + $Extra + @("_?=$bin"))
        if ($code -eq 0) {
            # `_?=` deja el desinstalador en su sitio para esperar a que termine; se retira a mano.
            Remove-Item (Join-Path $bin 'uninstall.exe') -ErrorAction SilentlyContinue
            Remove-Item $bin -Recurse -Force -ErrorAction SilentlyContinue
        }
        return $code
    }
    foreach ($n in $nets) {
        $code = (& $maker "https://127.0.0.1:$($n.Port)" $n.Id.Replace('-', '') "red-$($n.Short)" 'token-sintetico' | Out-String).Trim()
        Invoke-Cli network join $code --yes --insecure-http --name equipo-prueba | Out-Null
    }
    # A en marcha y B parada y deshabilitada (up + down): la actualización solo debe volver a arrancar A.
    Invoke-Cli network up $nets[0].Short | Out-Null
    Invoke-Cli network up $nets[1].Short | Out-Null
    Invoke-Cli network down $nets[1].Short | Out-Null
    Assert-True ((Setup $Installer @('/S')) -eq 0) 'actualización con redes unidas'
    $a = Service-Info-Of $nets[0].Service
    $b = Service-Info-Of $nets[1].Service
    Assert-True ($a.State -eq 'Running' -and $b.State -eq 'Stopped' -and $b.StartMode -eq 'Disabled') "tras actualizar: A $($a.State), B $($b.State)/$($b.StartMode)"
    $failureA = & "$env:SystemRoot/System32/sc.exe" qfailure $nets[0].Service | Out-String
    Assert-True ($failureA -match '5000' -and $failureA -match '15000' -and $failureA -match '60000') "la actualización no restableció la recuperación SCM de A: $failureA"
    $pipeA = $nets[0].Pipe
    Assert-True (Wait-Until { try { Invoke-Cli status --socket $pipeA | Out-Null; $true } catch { $false } }) 'A no responde tras actualizar'
    Assert-True (-not (Get-Service AteneaAgent -ErrorAction SilentlyContinue)) 'la actualización registró AteneaAgent'
    # /PURGE con redes unidas se rechaza antes de tocar nada.
    Assert-True ((Uninstall-Atenea @('/PURGE')) -eq 2) '/PURGE con redes unidas debía rechazarse'
    Assert-True ((Test-Path (Join-Path $data "networks\$($nets[0].Id)\device")) -and (Test-Path (Join-Path $data "networks\$($nets[1].Id)\device")) -and (Test-Path (Join-Path $bin 'atenea.exe'))) 'el rechazo de /PURGE tocó datos o binarios'
    Assert-True ((Service-Info-Of $nets[0].Service).State -eq 'Running') 'el rechazo de /PURGE paró A'
    # Desinstalar sin /PURGE: servicios parados y deshabilitados, identidades conservadas.
    Assert-True ((Uninstall-Atenea @()) -eq 0) 'desinstalación con redes unidas'
    foreach ($n in $nets) {
        $s = Service-Info-Of $n.Service
        Assert-True ($s.State -eq 'Stopped' -and $s.StartMode -eq 'Disabled') "tras desinstalar, $($n.Service): $($s.State)/$($s.StartMode)"
        Assert-True (-not (Get-NetAdapter -Name $n.Alias -IncludeHidden -ErrorAction SilentlyContinue)) "la desinstalación dejó el adaptador de $($n.Short)"
        Assert-True (Test-Path (Join-Path $data "networks\$($n.Id)\device\agent.toml")) "la desinstalación borró las claves de $($n.Short)"
    }
    # Reinstalar: `up` reactiva el servicio que dejó deshabilitado la desinstalación.
    Assert-True ((Setup $Installer @('/S')) -eq 0) 'reinstalación con redes unidas'
    Invoke-Cli network up $nets[0].Short | Out-Null
    Assert-True ((Service-Info-Of $nets[0].Service).State -eq 'Running') 'up no reactivó la red tras reinstalar'
    # /PURGE /FORCE borra también las identidades de las redes unidas y sus servicios.
    Assert-True ((Uninstall-Atenea @('/PURGE', '/FORCE')) -eq 0) '/PURGE /FORCE'
    Assert-True (-not (Test-Path $data)) '/PURGE /FORCE dejó datos'
    Assert-True (@(Get-Service 'AteneaNetwork-*' -ErrorAction SilentlyContinue).Count -eq 0) '/PURGE /FORCE dejó servicios de red'
    Assert-True ((Setup $Installer @('/S')) -eq 0) 'reinstalación final'
    Write-Output 'NETWORK INSTALADOR: OK'
} finally {
    foreach ($p in $script:servers) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch {} }
    foreach ($name in @($svc) + @(Get-CimInstance Win32_Service -Filter "Name LIKE 'AteneaNetwork-%'" | ForEach-Object { $_.Name })) {
        if (Service-Info-Of $name) {
            & "$env:SystemRoot/System32/sc.exe" stop $name | Out-Null
            [void](Wait-Until { (Service-Info-Of $name).State -eq 'Stopped' } 40)
            & "$env:SystemRoot/System32/sc.exe" delete $name | Out-Null
        }
    }
    Remove-Item $dir, $public, (Join-Path $data 'networks') -Recurse -Force -ErrorAction SilentlyContinue
}
