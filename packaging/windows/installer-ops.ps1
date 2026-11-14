# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
# Operaciones con privilegios que el instalador NSIS (installer.nsi) delega aquí; reproduce las comprobaciones de
# install.ps1/uninstall.ps1 (ADR-023). Se extrae a un directorio temporal del instalador y se ejecuta elevado.
# Salida distinta de 0 = fallo; el motivo va a stderr/stdout y el instalador lo muestra.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Check', 'Prepare', 'Stop', 'Register', 'Start', 'Remove')][string]$Action,
    [Parameter(Mandatory)][string]$Bin,
    [Parameter(Mandatory)][string]$Data,
    [switch]$Purge
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$sc = Join-Path $env:SystemRoot 'System32/sc.exe'
$sddl = 'O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)'
$trusted = @('S-1-5-18', 'S-1-5-32-544')

function Invoke-Sc([string]$Arguments) {
    # Process.Start pasa la línea tal cual (PowerShell 5.1 descarta el argumento vacío de actions=).
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $sc
    $info.Arguments = $Arguments
    $info.UseShellExecute = $false
    $process = [Diagnostics.Process]::Start($info)
    try {
        if (-not $process.WaitForExit(15000)) { $process.Kill(); throw "sc ${Arguments}: plazo agotado" }
        if ($process.ExitCode -ne 0) { throw "sc $Arguments falló ($($process.ExitCode))" }
    } finally { $process.Dispose() }
}

function Set-Protected([string]$Path) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetSecurityDescriptorSddlForm($sddl)
    Set-Acl -LiteralPath $Path -AclObject $acl
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
        # Imprime «running» si el servicio estaba en marcha, para que el instalador lo vuelva a arrancar tras actualizar.
        $service = Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue
        if (-not $service) { Write-Output 'absent'; break }
        $was = $service.Status -ne 'Stopped'
        if ($was) {
            # Una salida con error durante la parada no debe provocar un reinicio por recuperación SCM.
            Invoke-Sc 'failure AteneaAgent reset= 0 actions= ""'
            $service.Stop()
            $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
        }
        Write-Output $(if ($was) { 'running' } else { 'stopped' })
    }
    'Register' {
        # Los usuarios pueden ejecutar la CLI, pero no sustituir el servicio ni la DLL.
        $acl = Get-Acl -LiteralPath $Bin
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'), 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
        Set-Acl -LiteralPath $Bin -AclObject $acl
        $path = '"{0}" run --service --config "{1}"' -f (Join-Path $Bin 'atenea-agent.exe'), (Join-Path $Data 'agent.toml')
        if (Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue) {
            Invoke-Sc "config AteneaAgent binPath= `"$($path.Replace('"', '\"'))`""
        } else {
            New-Service -Name AteneaAgent -DisplayName 'Atenea Client' -StartupType Manual -BinaryPathName $path `
                -Description 'Cliente Atenea: túnel cifrado, rutas firmadas y DNS dividido.' | Out-Null
        }
        # Tras una caída el DNS dividido queda hasta la limpieza de arranque: reinicio con espera creciente.
        Invoke-Sc 'failure AteneaAgent reset= 86400 actions= restart/5000/restart/15000/restart/60000'
        Invoke-Sc 'failureflag AteneaAgent 1'
    }
    'Start' { Start-Service -Name AteneaAgent }
    'Remove' {
        $service = Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue
        $config = Join-Path $Data 'agent.toml'
        if ($service) {
            Invoke-Sc 'failure AteneaAgent reset= 0 actions= ""'
            if ($service.Status -ne 'Stopped') { $service.Stop(); $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30)) }
        }
        if (Test-Path -LiteralPath $config) {
            $info = [Diagnostics.ProcessStartInfo]::new()
            $info.FileName = Join-Path $Bin 'atenea-agent.exe'
            $info.Arguments = 'cleanup --config "{0}"' -f $config
            $info.UseShellExecute = $false
            $cleanup = [Diagnostics.Process]::Start($info)
            try {
                if (-not $cleanup.WaitForExit(30000)) { $cleanup.Kill(); throw 'Timeout de cleanup; se conserva la instalación para poder repararla.' }
                if ($cleanup.ExitCode -ne 0) { throw "Falló cleanup (salida $($cleanup.ExitCode)); se conserva la instalación para poder repararla." }
            } finally { $cleanup.Dispose() }
        }
        if ($service) { Invoke-Sc 'delete AteneaAgent' }
        # Los binarios los retira el desinstalador NSIS; los datos solo con /PURGE (guarda de ADR-017 §9).
        if ($Purge -and (Test-Path -LiteralPath $Data)) { Remove-Item -LiteralPath $Data -Recurse -Force }
    }
}
