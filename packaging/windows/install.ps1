#Requires -RunAsAdministrator
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$bin = Join-Path $env:ProgramFiles 'Atenea'
$data = Join-Path $env:ProgramData 'Atenea'
if (Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue) {
    throw 'AteneaAgent ya está registrado. Desinstale antes de actualizar; se conservan las claves.'
}
if (Test-Path -LiteralPath $bin) { throw "Ya existe $bin; no se sobrescribe." }
$trusted = @('S-1-5-18', 'S-1-5-32-544')
foreach ($path in @($bin, $data)) {
    if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "No se admiten enlaces ni puntos de reanálisis: $path"
    }
}
# ProgramData lo puede crear cualquier usuario: se reutiliza solo si todo su contenido es de SYSTEM/Administradores.
if (Test-Path -LiteralPath $data) {
    foreach ($item in @(Get-Item -LiteralPath $data -Force) + @(Get-ChildItem -LiteralPath $data -Recurse -Force)) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "No se admiten enlaces ni puntos de reanálisis: $($item.FullName)" }
        $owner = (Get-Acl -LiteralPath $item.FullName).GetOwner([Security.Principal.SecurityIdentifier]).Value
        if ($trusted -notcontains $owner) { throw "$($item.FullName) no pertenece a SYSTEM ni a Administradores; revise y retire $data antes de instalar." }
    }
}
foreach ($path in @($bin, $data)) {
    New-Item -ItemType Directory -Force -Path $path | Out-Null
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)')
    Set-Acl -LiteralPath $path -AclObject $acl
}
try {
    foreach ($name in @('atenea.exe', 'atenea-agent.exe', 'uninstall.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $bin
    }
    & (Join-Path $PSScriptRoot 'get-wintun.ps1') -Destination $bin
    # Los usuarios pueden ejecutar la CLI, pero no sustituir el servicio ni la DLL.
    $acl = Get-Acl -LiteralPath $bin
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'), 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    Set-Acl -LiteralPath $bin -AclObject $acl
    $config = Join-Path $data 'agent.toml'
    New-Service -Name AteneaAgent -DisplayName 'Atenea Client' -StartupType Manual `
        -BinaryPathName ('"{0}" run --service --config "{1}"' -f (Join-Path $bin 'atenea-agent.exe'), $config) `
        -Description 'Cliente Atenea: túnel cifrado, rutas firmadas y DNS dividido.' | Out-Null
    # Tras una caída el DNS dividido queda hasta la limpieza de arranque: reinicio con espera creciente,
    # también ante salidas con error (no solo cierres abruptos); una parada ordenada no reinicia.
    & "$env:SystemRoot/System32/sc.exe" failure AteneaAgent reset= 86400 actions= restart/5000/restart/15000/restart/60000 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "sc failure falló ($LASTEXITCODE)" }
    & "$env:SystemRoot/System32/sc.exe" failureflag AteneaAgent 1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "sc failureflag falló ($LASTEXITCODE)" }
} catch {
    $failure = $_
    if (Get-Service -Name AteneaAgent -ErrorAction SilentlyContinue) { & "$env:SystemRoot/System32/sc.exe" delete AteneaAgent | Out-Null }
    try { Remove-Item -LiteralPath $bin -Recurse -Force }
    catch { Write-Warning "No se pudo retirar la instalación incompleta: $_" }
    throw $failure
}
Write-Output "Instalado sin arrancar. Configure $data\agent.toml y sus claves; después: Start-Service AteneaAgent"
