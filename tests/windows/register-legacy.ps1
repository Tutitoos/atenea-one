# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
# Registra el servicio heredado `AteneaAgent` como lo hacían las instalaciones anteriores (ADR-008): LocalSystem, arranque
# manual, reinicio a los 5/15/60 s también ante salidas con error. El instalador actual ya no lo registra (ADR-017 §9),
# pero el smoke del cliente heredado y las pruebas de actualización de una instalación existente lo necesitan.
# SOLO para la VM desechable de CI.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Solo se permite la VM desechable de GitHub Actions.' }
$bin = Join-Path $env:ProgramFiles 'Atenea'
$config = Join-Path $env:ProgramData 'Atenea\agent.toml'
$sc = Join-Path $env:SystemRoot 'System32/sc.exe'
$path = '"{0}" run --service --config "{1}"' -f (Join-Path $bin 'atenea-agent.exe'), $config
New-Service -Name AteneaAgent -DisplayName 'Atenea Client' -StartupType Manual -BinaryPathName $path `
    -Description 'Cliente Atenea: túnel cifrado, rutas firmadas y DNS dividido.' | Out-Null
& $sc failure AteneaAgent reset= 86400 actions= restart/5000/restart/15000/restart/60000 | Out-Null
if ($LASTEXITCODE) { throw "sc failure falló ($LASTEXITCODE)" }
& $sc failureflag AteneaAgent 1 | Out-Null
if ($LASTEXITCODE) { throw "sc failureflag falló ($LASTEXITCODE)" }
