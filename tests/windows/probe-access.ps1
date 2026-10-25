#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$name = 'atenea' + [guid]::NewGuid().ToString('N').Substring(0, 10)
$password = ConvertTo-SecureString ([guid]::NewGuid().ToString('N') + '!aA8') -AsPlainText -Force
$work = Join-Path $env:PUBLIC $name
$created = $false
$process = $null
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
        $credential = [Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$name", $password)
        $process = Start-Process -FilePath "$env:SystemRoot/System32/WindowsPowerShell/v1.0/powershell.exe" `
            -Credential $credential -PassThru -WorkingDirectory $work `
            -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', "`"$probe`"", '-ResultPath', "`"$result`"")
    } catch {
        # Solo la imposibilidad de crear/iniciar otro usuario admite la prueba DACL de respaldo.
        Write-Output ('fallback_dacl: el runner no permite crear/iniciar usuario estándar: ' + $_.Exception.Message)
        return
    }
    if (-not $process.WaitForExit(20000)) { throw 'Timeout del usuario estándar.' }
    if (-not (Test-Path -LiteralPath $result)) { throw 'El usuario estándar no produjo resultado.' }
    $response = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
    if ($process.ExitCode -ne 0 -or -not $response.ok) {
        throw ('Falló el aislamiento real de IPC: ' + (Get-Content -LiteralPath $result -Raw))
    }
    Write-Output 'standard_user: control denegado, estado permitido y stop rechazado'
} finally {
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    if ($created) { Remove-LocalUser -Name $name }
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
}
