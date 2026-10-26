# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$name = 'atenea' + [guid]::NewGuid().ToString('N').Substring(0, 10)
$password = ConvertTo-SecureString ([guid]::NewGuid().ToString('N') + '!aA8') -AsPlainText -Force
$work = Join-Path $env:PUBLIC $name
$created = $false
$process = $null
$failure = $null
try {
    New-Item -ItemType Directory -Path $work | Out-Null
    $output = Join-Path $work 'result'
    New-Item -ItemType Directory -Path $output | Out-Null
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)')
    Set-Acl -LiteralPath $work -AclObject $acl
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;BU)')
    Set-Acl -LiteralPath $output -AclObject $acl
    $probe = Join-Path $work 'probe.ps1'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'probe-user.ps1') -Destination $probe
    $result = Join-Path $output 'result.json'
    try {
        New-LocalUser -Name $name -Password $password -AccountNeverExpires | Out-Null
        $created = $true
        $group = Get-LocalGroup -SID 'S-1-5-32-545'
        if (-not (Get-LocalGroupMember -Group $group | Where-Object {$_.Name -eq "$env:COMPUTERNAME\$name"})) {
            Add-LocalGroupMember -Group $group -Member $name
        }
        $info = [Diagnostics.ProcessStartInfo]::new()
        $info.FileName = "$env:SystemRoot/System32/WindowsPowerShell/v1.0/powershell.exe"
        $info.Arguments = '-NoProfile -NonInteractive -OutputFormat Text -ExecutionPolicy Bypass -File "{0}" -ResultPath "{1}"' -f $probe,$result
        $info.UseShellExecute = $false
        $info.WorkingDirectory = $work
        $info.Domain = $env:COMPUTERNAME
        $info.UserName = $name
        $info.Password = $password
        $process = [Diagnostics.Process]::Start($info)
    } catch {
        # Solo la imposibilidad de crear/iniciar otro usuario admite la prueba DACL de respaldo.
        Write-Output ('fallback_dacl: el runner no permite crear/iniciar usuario estándar: ' + $_.Exception.Message)
        return
    }
    if (-not $process.WaitForExit(30000)) { throw 'Timeout del usuario estándar.' }
    if (-not (Test-Path -LiteralPath $result)) { throw 'El usuario estándar no produjo resultado.' }
    $response = Get-Content -LiteralPath $result -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($null -eq $response -or $null -eq $response.PSObject.Properties['ok'] -or $response.ok -isnot [bool]) {
        throw 'El usuario estándar devolvió un resultado sin el campo booleano ok.'
    }
    $code = $process.ExitCode
    if ($null -eq $code) { throw 'No se obtuvo un código de salida del usuario estándar.' }
    if ($code -ne 0 -or -not $response.ok) {
        throw ('Falló el aislamiento real de IPC (salida ' + $code + '): ' + (Get-Content -LiteralPath $result -Raw -Encoding UTF8))
    }
    Write-Output 'standard_user: control denegado, estado permitido y stop rechazado'
} catch {
    $failure = $_
    throw
} finally {
    $cleanupErrors = @()
    if ($process) {
        try {
            if (-not $process.HasExited) { $process.Kill() }
            if (-not $process.WaitForExit(10000)) { throw 'El proceso del usuario no terminó.' }
        } catch { $cleanupErrors += $_; Write-Warning "Reap del usuario: $_" }
        finally { $process.Dispose() }
    }
    try { if ($created) { Remove-LocalUser -Name $name } }
    catch { $cleanupErrors += $_; Write-Warning "Retirada del usuario: $_" }
    try { if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force } }
    catch { $cleanupErrors += $_; Write-Warning "Retirada de ficheros del usuario: $_" }
    if (@($cleanupErrors).Count -gt 0 -and -not $failure) { throw ($cleanupErrors -join '; ') }
}
