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
foreach ($path in @($bin, $data)) {
    if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "No se admiten enlaces ni puntos de reanálisis: $path"
    }
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
} catch {
    Remove-Item -LiteralPath $bin -Recurse -Force
    throw
}
Write-Output "Instalado sin arrancar. Configure $data\agent.toml y sus claves; después: Start-Service AteneaAgent"
