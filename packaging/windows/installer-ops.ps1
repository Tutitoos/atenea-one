# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Operaciones con privilegios que el instalador NSIS (installer.nsi) delega aquí (ADR-023, ADR-017 §9). Se ejecuta
# desde el directorio protegido de instalación, nunca desde %TEMP%.
# Salida distinta de 0 = fallo; el motivo va a stderr/stdout y el instalador lo muestra.
#
# El instalador NO registra ningún servicio: cada red crea el suyo (`atenea network up`, AteneaNetwork-<id8>). Si ya
# existe el servicio heredado `AteneaAgent` (instalaciones anteriores), se respeta.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Check', 'Prepare', 'Stop', 'Protect', 'Start', 'Remove')][string]$Action,
    [Parameter(Mandatory)][string]$Bin,
    [Parameter(Mandatory)][string]$Data,
    # Start: servicios que estaban en marcha antes de actualizar, separados por comas (salida de Stop).
    [string]$Names = '',
    [switch]$Purge,
    # Con -Purge, confirma borrar también las identidades de las redes que sigan unidas.
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$sc = Join-Path $env:SystemRoot 'System32/sc.exe'
$sddl = 'O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)'
$trusted = @('S-1-5-18', 'S-1-5-32-544')
$legacy = 'AteneaAgent'

function Invoke-Sc([string]$Arguments) {
    # Process.Start pasa la línea tal cual (PowerShell 5.1 descarta el argumento vacío de actions=).
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $sc
    $info.Arguments = $Arguments
    $info.UseShellExecute = $false
    # La salida de sc.exe («[SC] ... SUCCESS») no debe mezclarse con la del asistente: `Stop` imprime los nombres de
    # los servicios que el instalador leerá. Se lee aparte para no bloquear el proceso.
    $info.RedirectStandardOutput = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $output = $process.StandardOutput.ReadToEndAsync()
        if (-not $process.WaitForExit(15000)) { $process.Kill(); throw "sc ${Arguments}: plazo agotado" }
        if ($process.ExitCode -ne 0) { throw "sc $Arguments falló ($($process.ExitCode)): $($output.Result)" }
    } finally { $process.Dispose() }
}

function Set-Protected([string]$Path) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetSecurityDescriptorSddlForm($sddl)
    Set-Acl -LiteralPath $Path -AclObject $acl
}

# Servicios de Atenea: el heredado y uno por red.
function Get-AteneaServices {
    @(Get-Service -Name $legacy, 'AteneaNetwork-*' -ErrorAction SilentlyContinue)
}

# Redes unidas: directorios con UUID en %ProgramData%\Atenea\networks (las identidades). El nombre sale del índice público.
function Get-JoinedNetworks {
    $root = Join-Path $Data 'networks'
    if (-not (Test-Path -LiteralPath $root)) { return @() }
    @(Get-ChildItem -LiteralPath $root -Directory -Force | Where-Object { $_.Name -match '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$' } | ForEach-Object {
        $name = $_.Name
        try { $name = (Get-Content -LiteralPath (Join-Path $Data "public\$($_.Name).json") -Raw -Encoding UTF8 | ConvertFrom-Json).name } catch {}
        [pscustomobject]@{ Id = $_.Name; Name = $name; Config = (Join-Path $_.FullName 'device\agent.toml'); Service = "AteneaNetwork-$($_.Name.Substring(28))" }
    })
}

function Stop-AteneaService($service) {
    if ($service.Status -eq 'Stopped') { return }
    # Una salida con error durante la parada no debe provocar un reinicio por recuperación SCM.
    Invoke-Sc "failure $($service.Name) reset= 0 actions= `"`""
    $service.Stop()
    $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
}

# Limpia rutas, DNS y adaptador propios con el servicio parado; si falla, se conserva la instalación para repararla.
function Invoke-AgentCleanup([string]$Config) {
    if (-not (Test-Path -LiteralPath $Config)) { return }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $Bin 'atenea-agent.exe'
    $info.Arguments = 'cleanup --config "{0}"' -f $Config
    $info.UseShellExecute = $false
    $cleanup = [Diagnostics.Process]::Start($info)
    try {
        if (-not $cleanup.WaitForExit(30000)) { $cleanup.Kill(); throw 'Timeout de cleanup; se conserva la instalación para poder repararla.' }
        if ($cleanup.ExitCode -ne 0) { throw "Falló cleanup (salida $($cleanup.ExitCode)); se conserva la instalación para poder repararla." }
    } finally { $cleanup.Dispose() }
}

switch ($Action) {
    'Check' {
        foreach ($path in @($Bin, $Data)) {
            if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "No se admiten enlaces ni puntos de reanálisis: $path"
            }
        }
        # ProgramData lo puede crear cualquier usuario: se reutiliza solo si todo su contenido es de SYSTEM/Administradores.
        if (Test-Path -LiteralPath $Data) {
            foreach ($item in @(Get-Item -LiteralPath $Data -Force) + @(Get-ChildItem -LiteralPath $Data -Recurse -Force)) {
                if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "No se admiten enlaces ni puntos de reanálisis: $($item.FullName)" }
                $owner = (Get-Acl -LiteralPath $item.FullName).GetOwner([Security.Principal.SecurityIdentifier]).Value
                if ($trusted -notcontains $owner) { throw "$($item.FullName) no pertenece a SYSTEM ni a Administradores; revise y retire $Data antes de instalar." }
            }
        }
    }
    'Prepare' {
        foreach ($path in @($Bin, $Data)) {
            New-Item -ItemType Directory -Force -Path $path | Out-Null
            Set-Protected $path
        }
    }
    'Stop' {
        # Para los servicios en marcha (el heredado y los de las redes) y imprime sus nombres, separados por comas,
        # para que el instalador solo vuelva a arrancar esos tras actualizar.
        $running = @(Get-AteneaServices | Where-Object { $_.Status -ne 'Stopped' })
        foreach ($service in $running) { Stop-AteneaService $service }
        Write-Output (($running | ForEach-Object { $_.Name }) -join ',')
    }
    'Protect' {
        # Los usuarios pueden ejecutar la CLI, pero no sustituir los servicios ni la DLL.
        $acl = Get-Acl -LiteralPath $Bin
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'), 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
        Set-Acl -LiteralPath $Bin -AclObject $acl
    }
    'Start' {
        foreach ($name in ($Names -split ',' | Where-Object { $_ -match '^(AteneaAgent|AteneaNetwork-[0-9a-f]{8})$' })) {
            # `Stop` dejó sin recuperación el servicio; se restablece el reinicio con espera creciente, también ante salidas con error.
            Invoke-Sc "failure $name reset= 86400 actions= restart/5000/restart/15000/restart/60000"
            Invoke-Sc "failureflag $name 1"
            Start-Service -Name $name
        }
    }
    'Remove' {
        $networks = Get-JoinedNetworks
        # La guarda va primero: antes de parar ni tocar nada.
        if ($Purge -and -not $Force -and $networks.Count -gt 0) {
            $list = ($networks | ForEach-Object { "  $($_.Name) ($($_.Id))" }) -join [Environment]::NewLine
            throw ("Siguen unidas estas redes y /PURGE borraría sus identidades:$([Environment]::NewLine)$list$([Environment]::NewLine)" +
                'Salga de cada una con `atenea network leave <red>` (revoca el dispositivo en el servidor) o repita con /PURGE /FORCE para borrarlas sin revocar.')
        }
        $legacyService = Get-Service -Name $legacy -ErrorAction SilentlyContinue
        foreach ($service in Get-AteneaServices) { Stop-AteneaService $service }
        Invoke-AgentCleanup (Join-Path $Data 'agent.toml')
        foreach ($network in $networks) { Invoke-AgentCleanup $network.Config }
        if ($legacyService) { Invoke-Sc "delete $legacy" }
        foreach ($network in $networks) {
            if (Get-Service -Name $network.Service -ErrorAction SilentlyContinue) {
                # Con /PURGE desaparecen las identidades y el servicio no tiene a qué volver; si no, queda parado y
                # deshabilitado y `atenea network up` lo reactiva tras reinstalar.
                if ($Purge) { Invoke-Sc "delete $($network.Service)" } else { Invoke-Sc "config $($network.Service) start= disabled" }
            }
        }
        # Los binarios los retira el desinstalador NSIS; los datos solo con /PURGE.
        if ($Purge -and (Test-Path -LiteralPath $Data)) { Remove-Item -LiteralPath $Data -Recurse -Force }
    }
}
