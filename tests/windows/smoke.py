"""Aceptación real; run.ps1 la limita al runner elevado y desechable."""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import sys
from smoke_support import ask, call, ps
import zipfile

def pipe_sddl(pipe):
    # Propietario, DACL y etiqueta MIC: READ_CONTROL basta, sin leer la SACL completa.
    advapi = ctypes.WinDLL('advapi32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    handle = kernel.CreateFileW(pipe, 0x20000, 3, None, 3, 0, None)
    if handle == wintypes.HANDLE(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    descriptor = ctypes.c_void_p()
    dacl = ctypes.c_void_p()
    text = wintypes.LPWSTR()
    advapi.GetSecurityInfo.argtypes = [wintypes.HANDLE, ctypes.c_int, wintypes.DWORD, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
    advapi.ConvertSecurityDescriptorToStringSecurityDescriptorW.argtypes = [ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(wintypes.LPWSTR), ctypes.c_void_p]
    advapi.GetAce.argtypes = [ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(ctypes.c_void_p)]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    security_info = 0x1 | 0x4 | 0x10  # OWNER | DACL | LABEL, no SACL_SECURITY_INFORMATION.
    try:
        code = advapi.GetSecurityInfo(handle, 1, security_info, None, None, ctypes.byref(dacl), None, ctypes.byref(descriptor))
        if code:
            raise ctypes.WinError(code)
        if not advapi.ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, 1, security_info, ctypes.byref(text), None):
            raise ctypes.WinError(ctypes.get_last_error())
        sddl = text.value
        print(f'{pipe}: owner+DACL+MIC={sddl}', flush=True)
        if dacl:
            # ACL.AceCount está en offset 4; cada ACE empieza por tipo, flags y tamaño.
            count = ctypes.c_ushort.from_address(dacl.value + 4).value
            for index in range(count):
                ace = ctypes.c_void_p()
                if not advapi.GetAce(dacl, index, ctypes.byref(ace)):
                    raise ctypes.WinError(ctypes.get_last_error())
                header = (ctypes.c_ubyte * 4).from_address(ace.value)
                mask = wintypes.DWORD.from_address(ace.value + 4).value
                print(f'  ACE[{index}] type={header[0]} flags=0x{header[1]:02x} mask=0x{mask:08x}', flush=True)
        return sddl
    finally:
        if text:
            kernel.LocalFree(ctypes.cast(text, ctypes.c_void_p))
        if descriptor:
            kernel.LocalFree(descriptor)
        kernel.CloseHandle(handle)


if __name__ == '__main__' and sys.argv[1:] == ['--pipe-sddl']:
    # diagnostics.ps1 ejecuta este lector en un hijo acotado, antes de parar SCM.
    for pipe in (r'\\.\pipe\atenea-control', r'\\.\pipe\atenea-status'):
        try:
            pipe_sddl(pipe)
        except OSError as error:
            print(f'{pipe}: descriptor no disponible: {error}', flush=True)
    sys.exit(0)


REPO = Path(__file__).resolve().parents[2]
BIN = Path(os.environ['ProgramFiles']) / 'Atenea'
CLI = BIN / 'atenea.exe'
AGENT = BIN / 'atenea-agent.exe'
DATA = Path(os.environ['ProgramData']) / 'Atenea'
CONTROL = r'\\.\pipe\atenea-control'
STATUS = r'\\.\pipe\atenea-status'
NETWORK = '77777777777777777777777777777777'
assert os.environ.get('GITHUB_ACTIONS') == 'true'
assert ctypes.windll.shell32.IsUserAnAdmin()


with zipfile.ZipFile(REPO / 'target/windows-package/atenea-client-unsigned.zip') as package:
    names = package.namelist()
    assert not any(name.lower().endswith('.dll') for name in names), names
    assert {'atenea.exe', 'atenea-agent.exe', 'install.ps1', 'uninstall.ps1', 'get-wintun.ps1'} <= set(names), names




def until(check, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
            pass
        time.sleep(0.2)
    raise AssertionError('Condición no cumplida dentro del plazo')


def snapshot():
    return ps("@{routes=@(Get-NetRoute | Where-Object {$_.DestinationPrefix -in @('0.0.0.0/0','::/0')} | Select-Object DestinationPrefix,NextHop,InterfaceIndex,RouteMetric | Sort-Object DestinationPrefix,InterfaceIndex,NextHop);dns=@(Get-DnsClientServerAddress | Select-Object InterfaceIndex,AddressFamily,ServerAddresses | Sort-Object InterfaceIndex,AddressFamily)} | ConvertTo-Json -Depth 5 -Compress")






def nrpt():
    return ps("@(Get-DnsClientNrptRule | Where-Object {$_.Namespace -contains '.lab.internal' -or $_.Namespace -contains 'lab.internal'}) | ConvertTo-Json -Depth 5 -Compress")


def root_nrpt():
    return ps("@(Get-DnsClientNrptRule | Where-Object {$_.Namespace -contains '.' -or $_.Namespace -contains ''}) | ConvertTo-Json -Depth 5 -Compress")


temp = tempfile.TemporaryDirectory(prefix='atenea-windows-')
work = Path(temp.name)
peer = None
peerlog = None
config = DATA / 'agent.toml'
cleanup_errors = []


def cleanup_step(label, action):
    try:
        action()
    except Exception as error:
        cleanup_errors.append(f'{label}: {error}')
        print(f'ERROR DE LIMPIEZA {label}: {error}', file=sys.stderr, flush=True)


try:
    # Las claves, el TOML y el estado nunca heredan acceso de usuarios ajenos.
    ps(f"icacls '{work}' /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null; if ($LASTEXITCODE) {{throw 'icacls'}}")
    assert not nrpt(), 'No se sobrescribe una regla NRPT existente'
    assert not root_nrpt(), 'El runner ya tiene una regla NRPT raíz'
    before = snapshot()
    anchor = call(CLI, 'root-init', '--out', work / 'root.key')
    call(CLI, 'issuer-init', '--root', work / 'root.key', '--network-id', NETWORK, '--serial', '1', '--out-dir', work / 'issuer')
    device = call(CLI, 'device-init', '--dir', DATA / 'device')
    device_id = re.search(r'device_id=([0-9a-f]+)', device)[1]
    call(CLI, 'device-init', '--dir', work / 'peer')
    config = DATA / 'agent.toml'
    def toml_path(path):
        return str(path).replace('\\', '/')
    config.write_text(f'''interface = "atenea0"
listen = "127.0.0.1:51820"
ipc_socket = '\\\\.\\pipe\\atenea-control'
status_socket = '\\\\.\\pipe\\atenea-status'
status_gid = 545
noise_key_file = "{toml_path(DATA / 'device/noise.key')}"
identity_key_file = "{toml_path(DATA / 'device/identity.key')}"
config_file = "{toml_path(DATA / 'config.bin')}"
state_dir = "{toml_path(DATA / 'state')}"
network_id = "{NETWORK}"
device_id = "{device_id}"
trust_anchor = "{anchor}"
''', encoding='utf-8')
    ps(f"icacls '{config}' /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null; if ($LASTEXITCODE) {{throw 'icacls'}}")

    def issue(version, extended):
        spec = work / 'spec.toml'
        spec.write_text(f'''network_id = "{NETWORK}"
device_bundle = "{toml_path(DATA / 'device/bundle.bin')}"
config_version = {version}
validity_secs = 3600
mtu = 1280
tunnel_addrs = ["10.77.0.2", "fd77::2"]
network_prefixes = ["10.77.0.0/24", "fd77::/64"]
[[peer]]
bundle = "{toml_path(work / 'peer/bundle.bin')}"
addrs = ["10.77.0.1", "fd77::1"]
endpoints = ["127.0.0.1:51821"]
routes = {('["10.88.0.0/24", "fd88::/64"]' if extended else '[]')}
[[rule]]
src = "any"
dst = "any"
''' + ('[dns]\nservers = ["10.77.0.1"]\ndomains = ["lab.internal"]\n' if extended else ''), encoding='utf-8')
        call(CLI, 'issue', '--issuer-key', work / 'issuer/issuer.key', '--issuer-cert', work / 'issuer/issuer.cert', '--spec', spec, '--out', DATA / 'config.bin')

    peer = None
    try:
        service = json.loads(ps("Get-CimInstance Win32_Service -Filter \"Name='AteneaAgent'\" | Select-Object State,StartMode,StartName,PathName | ConvertTo-Json -Compress"))
        assert service['State'] == 'Stopped' and service['StartMode'] == 'Manual', service
        assert service['StartName'] == 'LocalSystem', service
        assert 'run --service --config' in service['PathName']
        issue(1, False)
        peerlog = (work / 'peer.log').open('w+')
        peer = subprocess.Popen([str(REPO / 'target/debug/atenea-testpeer.exe'), '--device-dir', str(work / 'peer'), '--agent-bundle', str(DATA / 'device/bundle.bin'), '--listen', '127.0.0.1:51821', '--own-addr', '10.77.0.1', '--own-addr6', 'fd77::1', '--agent-addr', '10.77.0.2', '--agent-addr6', 'fd77::2', '--network-id', NETWORK, '--count', '2', '--timeout-secs', '120'], stdout=peerlog, stderr=subprocess.STDOUT)
        until(lambda: 'ready' in (work / 'peer.log').read_text())
        ps('Start-Service AteneaAgent')
        until(lambda: ask(CONTROL, 'status'))
        until(lambda: '10.77.0.2' in ps("Get-NetIPAddress -InterfaceAlias atenea0 | Select-Object -ExpandProperty IPAddress"))
        assert 'fd77::2' in ps("Get-NetIPAddress -InterfaceAlias atenea0 | Select-Object -ExpandProperty IPAddress")
        assert ps("@(Get-NetIPInterface -InterfaceAlias atenea0 | Where-Object {$_.NlMtu -ne 1280}).Count") == '0'
        for private in (config, DATA / 'device/noise.key', DATA / 'device/identity.key', DATA / 'state/state.bin'):
            sddl = ps(f"(Get-Acl -LiteralPath '{private}').Sddl")
            allowed = set(re.findall(r'\(A;[^)]*;;;([^;)]+)\)', sddl))
            assert 'D:P' in sddl and allowed == {'SY', 'BA'}, (private, sddl)
        for pipe, readers in ((CONTROL, {'SY', 'BA'}), (STATUS, {'SY', 'BA', 'BU'})):
            sddl = pipe_sddl(pipe)
            allowed = re.findall(r'\(A;[^)]*;;;([^;)]+)\)', sddl)
            assert set(allowed) == readers, sddl
        print(ps(f"& '{REPO / 'tests/windows/probe-access.ps1'}'", timeout=45))
        assert ask(CONTROL, 'status'), 'El intento sin privilegios no debe parar el servicio'
        assert 'error' in ask(STATUS, 'stop')
        assert ask(CONTROL, 'status')['dns'] == 'none'
        issue(2, True)
        call(CLI, 'reload', '--socket', CONTROL)
        until(lambda: ask(STATUS, 'status').get('dns') == 'applied')
        status = ask(STATUS, 'status')
        assert {r['prefix'] for r in status['routes'] if r['state'] == 'installed'} == {'10.88.0.0/24', 'fd88::/64'}, status
        for prefix in ('10.88.0.0/24', 'fd88::/64'):
            assert ps(f"(Get-NetRoute -DestinationPrefix '{prefix}').InterfaceAlias") == 'atenea0'
        rules = json.loads(nrpt())
        if isinstance(rules, dict):
            rules = [rules]
        assert len(rules) == 1 and rules[0]['NameServers'] == '10.77.0.1', rules
        assert not root_nrpt(), 'Se creó una regla NRPT para el dominio raíz'
        # El adaptador nuevo no debe alterar las rutas por defecto ni los DNS de otros enlaces.
        current = json.loads(snapshot())
        original = json.loads(before)
        own_index = int(ps('(Get-NetAdapter -Name atenea0).ifIndex'))
        current['dns'] = [r for r in current['dns'] if r['InterfaceIndex'] != own_index]
        assert current == original, (current, original)
        for family, address in (('-4', '10.77.0.1'), ('-6', 'fd77::1')):
            until(lambda: subprocess.run(['ping.exe', family, '-n', '1', '-w', '1000', address], capture_output=True, timeout=5).returncode == 0, 40)
        assert peer.wait(timeout=15) == 0
        assert 'echo_replies=2' in (work / 'peer.log').read_text()
        # SCM entrega stop al Lifecycle; se espera limpieza antes de declarar Stopped.
        ps('Stop-Service AteneaAgent; (Get-Service AteneaAgent).WaitForStatus("Stopped", [TimeSpan]::FromSeconds(30))', timeout=40)
        assert not nrpt()
        assert not ps('Get-NetAdapter -Name atenea0 -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name')
        assert snapshot() == before
        # Caída sin destructor y limpieza offline, seguida de reinicio desde v2 persistida.
        ps('Start-Service AteneaAgent')
        until(nrpt)
        ps("$p=(Get-CimInstance Win32_Service -Filter \"Name='AteneaAgent'\").ProcessId; Stop-Process -Id $p -Force")
        until(lambda: ps('(Get-Service AteneaAgent).Status') == 'Stopped')
        call(AGENT, 'cleanup', '--config', config)
        assert not nrpt()
        assert not ps('Get-NetAdapter -Name atenea0 -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name')
        assert snapshot() == before
        ps('Start-Service AteneaAgent')
        until(nrpt)
        ps('Stop-Service AteneaAgent')
        assert not nrpt()
        assert snapshot() == before
    finally:
        # No se permite que un fallo de SCM tape el error de aceptación ni salte el reap.
        cleanup_step('diagnóstico previo', lambda: print(ps(f"& '{REPO / 'tests/windows/diagnostics.ps1'}' -PeerLog '{work / 'peer.log'}'", timeout=25), flush=True))
        cleanup_step('Stop-Service', lambda: ps('$s=Get-Service AteneaAgent; if ($s.Status -ne "Stopped") {$s.Stop(); $s.WaitForStatus("Stopped", [TimeSpan]::FromSeconds(30))}', timeout=40))
        # Solo en esta VM desechable: si SCM falla, se mata y espera al proceso real.
        cleanup_step('reap agente', lambda: ps("$s=Get-CimInstance Win32_Service -Filter \"Name='AteneaAgent'\"; if ($s.ProcessId) {$p=Get-Process -Id $s.ProcessId -ErrorAction SilentlyContinue; if ($p) {$p.Kill(); if (-not $p.WaitForExit(10000)) {throw 'El agente no terminó'}}}"))
        if peer:
            def reap_peer():
                if peer.poll() is None:
                    peer.kill()
                peer.wait(timeout=10)
            cleanup_step('reap testpeer', reap_peer)
        if peerlog:
            cleanup_step('cerrar peer.log', peerlog.close)
        cleanup_step('diagnóstico final', lambda: print(ps(f"& '{REPO / 'tests/windows/diagnostics.ps1'}' -PeerLog '{work / 'peer.log'}'", timeout=25), flush=True))
        cleanup_step('cleanup offline', lambda: call(AGENT, 'cleanup', '--config', config))
finally:
    # Diagnóstico también si fallan las precondiciones anteriores al arranque.
    cleanup_step('diagnóstico de salida', lambda: print(ps(f"& '{REPO / 'tests/windows/diagnostics.ps1'}' -PeerLog '{work / 'peer.log'}'", timeout=25), flush=True))
    cleanup_step('directorio temporal', temp.cleanup)
    if cleanup_errors and sys.exc_info()[0] is None:
        raise RuntimeError('Falló la limpieza: ' + '; '.join(cleanup_errors))
print('Windows: servicio, Wintun, v1/v2, IPv4/IPv6 Noise, rutas, NRPT, DACL IPC y limpieza correctos')
