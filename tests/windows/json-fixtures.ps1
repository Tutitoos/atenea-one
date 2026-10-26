# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -Version 5.1
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1 -or $PSVersionTable.PSEdition -ne 'Desktop') {
    throw 'Estas muestras requieren Windows PowerShell 5.1 real.'
}
$target = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target'))
$outputPath = [IO.Path]::GetFullPath($Out)
if (-not $outputPath.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'El archivo de muestras debe quedar dentro de target.'
}
$cases = [Collections.Generic.List[object]]::new()
function Add-Fixture([string]$Name, [string]$Parser, [object]$Value, [string]$Expected) {
    # PS5.1 no emite ningún objeto al serializar $null; capturarlo siempre como array.
    $parts = @(ConvertTo-Json -InputObject $Value -Depth 12 -Compress)
    [string]$json = [string]::Join("`n", [string[]]$parts)
    $cases.Add([pscustomobject]@{name = $Name; parser = $Parser; json = $json; expected = $Expected})
    # Conserva la evidencia nativa sin volcar objetos o documentos ilimitados.
    $bounded = [regex]::Match($json, '\A[\s\S]{0,1000}').Value
    Write-Output ('PS5.1 {0}/{1}: {2}' -f $Parser, $Name, $bounded)
}
$rule = [pscustomobject]@{
    Name = 'regla'; Namespace = [string[]]@('.lab.internal');
    NameServers = [string[]]@('192.0.2.53', '2001:db8::53'); Comment = 'Atenea prueba'
}
$expectedRule = '[{"Name":"regla","Namespace":[".lab.internal"],"NameServers":["192.0.2.53","2001:db8::53"],"Comment":"Atenea prueba"}]'
Add-Fixture 'vacio' 'nrpt' ([object[]]@()) '[]'
Add-Fixture 'nulo' 'nrpt' $null '[]'
Add-Fixture 'objeto' 'nrpt' $rule $expectedRule
Add-Fixture 'lista' 'nrpt' ([object[]]@($rule)) $expectedRule
$scalarRule = [pscustomobject]@{Name = 'regla'; Namespace = '.lab.internal'; NameServers = '192.0.2.53'; Comment = 'Atenea prueba'}
Add-Fixture 'propiedades-escalares' 'nrpt' $scalarRule '[{"Name":"regla","Namespace":[".lab.internal"],"NameServers":["192.0.2.53"],"Comment":"Atenea prueba"}]'
# Reproduce la proyeccion anterior con PSObject y colecciones tipadas de PS5.1.
$projected = @($rule | Select-Object Name,@{Name = 'Namespace'; Expression = {,@($_.Namespace | ForEach-Object {[string]$_})}},@{Name = 'NameServers'; Expression = {,@($_.NameServers | ForEach-Object {[string]$_})}},Comment)
Add-Fixture 'proyeccion-psobject' 'nrpt' $projected $expectedRule
$wrappedRule = [pscustomobject]@{
    Name = 'regla'; Namespace = [pscustomobject]@{value = [string[]]@('.lab.internal'); Count = 1};
    NameServers = [pscustomobject]@{value = [string[]]@('192.0.2.53', '2001:db8::53'); Count = 2}; Comment = 'Atenea prueba'
}
Add-Fixture 'namespace-nameservers-value-count' 'nrpt' ([object[]]@($wrappedRule)) $expectedRule
Add-Fixture 'lista-value-count' 'nrpt' ([pscustomobject]@{value = [object[]]@($wrappedRule); Count = 1}) $expectedRule
$route = [pscustomobject]@{DestinationPrefix = '0.0.0.0/0'; NextHop = '192.0.2.1'; InterfaceIndex = 7; RouteMetric = 25}
$dns = [pscustomobject]@{InterfaceIndex = 7; AddressFamily = 2; ServerAddresses = [string[]]@('192.0.2.53', '198.51.100.53')}
$expectedSnapshot = '{"routes":[{"DestinationPrefix":"0.0.0.0/0","NextHop":"192.0.2.1","InterfaceIndex":7,"RouteMetric":25}],"dns":[{"InterfaceIndex":7,"AddressFamily":2,"ServerAddresses":["192.0.2.53","198.51.100.53"]}]}'
Add-Fixture 'vacio' 'snapshot' ([pscustomobject]@{routes = [object[]]@(); dns = [object[]]@()}) '{"routes":[],"dns":[]}'
Add-Fixture 'listas' 'snapshot' ([pscustomobject]@{routes = [object[]]@($route); dns = [object[]]@($dns)}) $expectedSnapshot
Add-Fixture 'objetos' 'snapshot' ([pscustomobject]@{routes = $route; dns = $dns}) $expectedSnapshot
$wrappedDns = [pscustomobject]@{InterfaceIndex = 7; AddressFamily = 2; ServerAddresses = [pscustomobject]@{value = [string[]]@('192.0.2.53', '198.51.100.53'); Count = 2}}
Add-Fixture 'value-count' 'snapshot' ([pscustomobject]@{routes = [pscustomobject]@{value = [object[]]@($route); Count = 1}; dns = [pscustomobject]@{value = [object[]]@($wrappedDns); Count = 1}}) $expectedSnapshot
$scalarDns = [pscustomobject]@{InterfaceIndex = 7; AddressFamily = 2; ServerAddresses = '192.0.2.53'}
Add-Fixture 'servidor-escalar' 'snapshot' ([pscustomobject]@{routes = [object[]]@(); dns = [object[]]@($scalarDns)}) '{"routes":[],"dns":[{"InterfaceIndex":7,"AddressFamily":2,"ServerAddresses":["192.0.2.53"]}]}'
$service = [pscustomobject]@{State = 'Running'; StartMode = 'Auto'; StartName = 'LocalSystem'; PathName = 'C:\Atenea\atenea-agent.exe'}
$expectedService = '{"State":"Running","StartMode":"Auto","StartName":"LocalSystem","PathName":"C:\\Atenea\\atenea-agent.exe"}'
Add-Fixture 'objeto' 'service' $service $expectedService
Add-Fixture 'lista' 'service' ([object[]]@($service)) $expectedService
Add-Fixture 'value-count' 'service' ([pscustomobject]@{value = [object[]]@($service); Count = 1}) $expectedService
Add-Fixture 'vacio' 'strings' ([string[]]@()) '[]'
Add-Fixture 'nulo' 'strings' $null '[]'
Add-Fixture 'escalar' 'strings' '192.0.2.53' '["192.0.2.53"]'
Add-Fixture 'lista-unitaria' 'strings' ([string[]]@('192.0.2.53')) '["192.0.2.53"]'
Add-Fixture 'lista-multiple' 'strings' ([string[]]@('192.0.2.53', '2001:db8::53')) '["192.0.2.53","2001:db8::53"]'
Add-Fixture 'value-count' 'strings' ([pscustomobject]@{value = [string[]]@('192.0.2.53', '2001:db8::53'); Count = 2}) '["192.0.2.53","2001:db8::53"]'
Add-Fixture 'value-count-vacio' 'strings' ([pscustomobject]@{value = [string[]]@(); Count = 0}) '[]'
Add-Fixture 'value-count-escalar' 'strings' ([pscustomobject]@{value = '192.0.2.53'; Count = 1}) '["192.0.2.53"]'
Add-Fixture 'cero' 'count' 0 '0'
Add-Fixture 'positivo' 'count' 2 '2'
Add-Fixture 'dacl-protegida' 'sddl' 'O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)' '["BA","SY"]'
Add-Fixture 'principal-repetido' 'sddl' 'D:P(A;;FA;;;SY)(A;;FR;;;SY)(A;;FA;;;BA)' '["BA","SY"]'
$document = [pscustomobject]@{realPSversion = $PSVersionTable.PSVersion.ToString(); cases = $cases.ToArray()}
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($outputPath)) | Out-Null
[IO.File]::WriteAllText($outputPath, (ConvertTo-Json -InputObject $document -Depth 16 -Compress), [Text.UTF8Encoding]::new($false))
Write-Output "Muestras recogidas con Windows PowerShell $($document.realPSversion): $($cases.Count)"
