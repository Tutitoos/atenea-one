"""Aceptación real: ejecutar exclusivamente mediante run.sh en el runner desechable."""
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import socket
import stat
import subprocess
import signal
import sys
import threading
import time

REPO = Path(__file__).resolve().parents[2]
BIN = REPO / "target/debug"
CLI = BIN / "atenea"
AGENT = BIN / "atenea-agent"
NETWORK = "77777777777777777777777777777777"
RESOLVER = Path("/etc/resolver/lab.internal")
OWNER = "# Atenea One: resolver administrado\n"


def run(*args, **kwargs):
    # -W de ping no es un deadline de proceso; todas las órdenes tienen uno externo.
    kwargs.setdefault("timeout", 10)
    return subprocess.run(*args, **kwargs)


def call(*args, **kwargs):
    return run([str(a) for a in args], check=True, stdout=subprocess.PIPE,
               text=True, **kwargs).stdout.strip()


def phase(message):
    print(f"macOS: {message}", file=sys.stderr, flush=True)


def until(check, seconds=30, *, description, process=None):
    phase(f"esperando {description} (máximo {seconds}s)")
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise AssertionError(f"{description}: proceso terminó con {process.returncode}")
        try:
            result = check()
            if result:
                return result
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(0.2)
    raise AssertionError(f"Timeout esperando {description}")


def terminate(process):
    try:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=5)


def background(args, log, work, name, seconds=240):
    phase(f"arrancando {name} (vida máxima {seconds}s)")
    env = dict(os.environ, NO_COLOR="1", CLICOLOR="0", FORCE_COLOR="0")
    process = subprocess.Popen([str(a) for a in args], stdout=log, env=env,
                               stderr=subprocess.STDOUT, start_new_session=True)
    (work / f"{name}.pid").write_text(str(process.pid))
    def expired():
        phase(f"timeout del proceso {name}")
        terminate(process)
    timer = threading.Timer(seconds, expired)
    timer.daemon = True
    timer.start()
    return process, timer


def diagnostics(work):
    # Este modo también lo invoca trap EXIT si la prueba recibe TERM/KILL o falla antes del try.
    for name in ("agent", "peer"):
        path = work / f"{name}.log"
        phase(f"log {name}")
        if path.is_file():
            text = path.read_text(errors="replace")
            text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
            phase(f"inicio log completo {name}: {len(text)} caracteres")
            print(text, file=sys.stderr, flush=True)
            phase(f"fin log completo {name}")
    for command in (["/sbin/ifconfig"], ["/usr/sbin/netstat", "-rn"],
                    ["/bin/ls", "-la", "/etc/resolver"]):
        phase("diagnóstico " + " ".join(command))
        try:
            result = run(command, capture_output=True, text=True, timeout=5)
            print(result.stdout, result.stderr, file=sys.stderr, flush=True)
        except subprocess.SubprocessError as error:
            phase(str(error))
    for name in ("agent", "peer"):
        path = work / f"{name}.pid"
        if path.exists():
            try:
                os.killpg(int(path.read_text()), signal.SIGKILL)
            except ProcessLookupError:
                pass
    config = work / "agent.toml"
    if config.exists():
        phase("cleanup offline (máximo 10s)")
        try:
            result = run([str(AGENT), "cleanup", "--config", str(config)],
                         capture_output=True, text=True, timeout=10)
            print(result.stdout, result.stderr, file=sys.stderr, flush=True)
            if result.returncode:
                raise RuntimeError(f"cleanup terminó con {result.returncode}")
        except subprocess.SubprocessError as error:
            raise RuntimeError("cleanup no terminó dentro del plazo") from error


def defaults():
    return "\n".join(line for family in ("inet", "inet6")
                     for line in call("/usr/sbin/netstat", "-rn", "-f", family).splitlines()
                     if line.split() and line.split()[0] == "default")


def route_interfaces(table, prefix):
    """Busca el prefijo exacto instalado, no un lookup de gateway con route get."""
    wanted = ipaddress.ip_network(prefix)
    interfaces = set()
    column = None
    for line in table.splitlines():
        fields = line.split()
        if fields and fields[0] == "Destination":
            column = fields.index("Netif")
            continue
        if not fields or column is None:
            continue
        if len(fields) <= column or len(fields) < 3:
            raise AssertionError(f"fila netstat inválida: {line}")
        if fields[0] == "default":
            continue
        address, slash, length = fields[0].partition("/")
        address = address.split("%")[0]
        if ":" not in address:
            octets = address.split(".")
            length = length if slash else str(32 if "H" in fields[2] else len(octets) * 8)
            address = ".".join(octets + ["0"] * (4 - len(octets)))
        elif not slash:
            length = "128"
        network = ipaddress.ip_network(f"{address}/{length}", strict=False)
        if network == wanted and "U" in fields[2]:
            interfaces.add(fields[column])
    if column is None:
        raise AssertionError("netstat sin cabecera Destination/Netif")
    return interfaces


def check_route(prefix, interface):
    family = "inet6" if ":" in prefix else "inet"
    table = call("/usr/sbin/netstat", "-rn", "-f", family)
    actual = route_interfaces(table, prefix)
    assert actual == {interface}, f"ruta {prefix}: esperado {interface}, obtenido {actual}\n{table}"
    phase(f"ruta {prefix} confirmada por {interface} en netstat {family}")


def echo(command):
    result = run(command, capture_output=True, text=True, timeout=8)
    phase(f"{Path(command[0]).name}: exit={result.returncode}\n"
          f"{result.stdout}{result.stderr}")
    return result.returncode == 0


def ask(path, command):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(3)
        client.connect(str(path))
        client.sendall((command + "\n").encode())
        return json.loads(client.recv(65536))


def main(work):
    assert os.getuid() == 0 and os.environ.get("GITHUB_ACTIONS") == "true"
    phase("generando identidad y configuración efímeras")
    # El directorio permite llegar al socket al usuario ajeno: se prueba el socket, no solo el padre.
    work.chmod(0o755)
    before = defaults()
    assert not RESOLVER.exists(), "El runner ya contiene lab.internal; no se sobrescribe"
    root = work / "root.key"
    anchor = call(CLI, "root-init", "--out", root)
    call(CLI, "issuer-init", "--root", root, "--network-id", NETWORK, "--serial", "1", "--out-dir", work / "issuer")
    device = call(CLI, "device-init", "--dir", work / "device")
    device_id = re.search(r"device_id=([0-9a-f]+)", device)[1]
    call(CLI, "device-init", "--dir", work / "peer")
    config = work / "agent.toml"
    config.write_text(f'''interface = "utun"
listen = "127.0.0.1:51820"
ipc_socket = "{work}/control.sock"
status_socket = "{work}/status.sock"
status_gid = 0
noise_key_file = "{work}/device/noise.key"
identity_key_file = "{work}/device/identity.key"
config_file = "{work}/config.bin"
state_dir = "{work}/state"
network_id = "{NETWORK}"
device_id = "{device_id}"
trust_anchor = "{anchor}"
''')
    config.chmod(0o600)

    def issue(version, extended):
        spec = work / "spec.toml"
        spec.write_text(f'''network_id = "{NETWORK}"
device_bundle = "{work}/device/bundle.bin"
config_version = {version}
validity_secs = 3600
mtu = 1280
tunnel_addrs = ["10.77.0.2", "fd77::2"]
network_prefixes = ["10.77.0.0/24", "fd77::/64"]
[[peer]]
bundle = "{work}/peer/bundle.bin"
addrs = ["10.77.0.1", "fd77::1"]
endpoints = ["127.0.0.1:51821"]
routes = {('["10.88.0.0/24", "fd88::/64"]' if extended else '[]')}
[[rule]]
src = "any"
dst = "any"
''' + ('[dns]\nservers = ["10.77.0.1"]\ndomains = ["lab.internal"]\n' if extended else ''))
        call(CLI, "issue", "--issuer-key", work / "issuer/issuer.key", "--issuer-cert", work / "issuer/issuer.cert", "--spec", spec, "--out", work / "config.bin")

    timers = []
    agent = None
    peer = None
    log = (work / "agent.log").open("w+")
    peerlog = (work / "peer.log").open("w+")
    interface = None
    try:
        issue(1, False)  # Sin extensiones: wire v1; v2 añade rutas y DNS en caliente.
        peer, peer_timer = background([str(BIN / "atenea-testpeer"), "--device-dir", str(work / "peer"), "--agent-bundle", str(work / "device/bundle.bin"), "--listen", "127.0.0.1:51821", "--own-addr", "10.77.0.1", "--own-addr6", "fd77::1", "--agent-addr", "10.77.0.2", "--agent-addr6", "fd77::2", "--network-id", NETWORK, "--count", "2", "--timeout-secs", "90"], peerlog, work, "peer", seconds=100)
        timers.append(peer_timer)
        until(lambda: "ready" in (work / "peer.log").read_text(), description="ready del testpeer", process=peer)

        def start():
            process, timer = background([AGENT, "run", "--config", config], log, work, "agent")
            timers.append(timer)
            return process

        agent = start()
        until(lambda: (work / "control.sock").exists(), description="socket de control", process=agent)
        until(lambda: "inet 10.77.0.2" in call("/sbin/ifconfig"), description="IPv4 en utun", process=agent)
        interface = next(line.split(":")[0] for line in call("/sbin/ifconfig").splitlines()
                         if line.startswith("utun") and "mtu 1280" in line
                         and "inet 10.77.0.2" in call("/sbin/ifconfig", line.split(":")[0]))
        assert "fd77::2" in call("/sbin/ifconfig", interface)
        assert stat.S_IMODE((work / "control.sock").stat().st_mode) == 0o600
        assert stat.S_IMODE((work / "status.sock").stat().st_mode) == 0o660
        assert stat.S_IMODE((work / "state/state.bin").stat().st_mode) == 0o600
        assert (work / "state/state.bin").stat().st_uid == 0
        phase("comprobando aislamiento del socket con sonda Rust")
        probe = work / "atenea-ipc-probe"
        shutil.copyfile(BIN / "atenea-ipc-probe", probe)
        probe.chmod(0o755)
        def check_probe(path, expected, foreign=False):
            identity = dict(user=65534, group=65534, extra_groups=[]) if foreign else {}
            result = run([str(probe), str(path), expected], capture_output=True,
                         text=True, timeout=5, **identity)
            phase(f"sonda {path.name} {expected}, foreign={foreign}: "
                  f"exit={result.returncode}, stdout={result.stdout!r}, stderr={result.stderr!r}")
            result.check_returncode()
            assert result.stdout.strip() == ("permission_denied" if expected == "deny" else "connected")
        # Control positivo como nobody: verifica ejecución, recorrido de padres y socket Unix real.
        with socket.socket(socket.AF_UNIX) as public:
            public.bind(str(work / "public.sock"))
            (work / "public.sock").chmod(0o666)
            public.listen(1)
            check_probe(work / "public.sock", "allow", foreign=True)
        check_probe(work / "control.sock", "allow")
        check_probe(work / "control.sock", "deny", foreign=True)
        assert agent.poll() is None
        assert "error" in ask(work / "status.sock", "stop")
        phase("aplicando configuración v2")
        issue(2, True)
        call(CLI, "reload", "--socket", work / "control.sock")
        until(lambda: ask(work / "control.sock", "status").get("dns") == "applied", description="rutas y DNS v2", process=agent)
        status = ask(work / "status.sock", "status")
        phase("estado v2: " + json.dumps(status, sort_keys=True))
        assert {r["prefix"] for r in status["routes"] if r["state"] == "installed"} == {"10.88.0.0/24", "fd88::/64"}, status
        for prefix in ("10.88.0.0/24", "fd88::/64"):
            check_route(prefix, interface)
        assert "mtu 1280" in call("/sbin/ifconfig", interface)
        assert RESOLVER.read_text() == OWNER + "nameserver 10.77.0.1\n"
        assert not Path("/etc/resolver/.").is_file()
        assert defaults() == before
        # Ambos ecos atraviesan la utun real y Noise; reintento solo para esperar el handshake.
        until(lambda: echo(["/sbin/ping", "-n", "-t", "3", "-c", "1", "-W", "1000", "10.77.0.1"]), 40, description="eco IPv4", process=agent)
        phase("eco IPv4 confirmado")
        until(lambda: echo(["/sbin/ping6", "-n", "-c", "1", "fd77::1"]), 30, description="eco IPv6", process=agent)
        phase("eco IPv6 confirmado")
        assert peer.wait(timeout=15) == 0
        assert "echo_replies=2" in (work / "peer.log").read_text()
        phase("parada limpia del agente")
        call(CLI, "down", "--socket", work / "control.sock")
        assert agent.wait(timeout=10) == 0
        assert run(["/sbin/ifconfig", interface], capture_output=True).returncode != 0
        assert not RESOLVER.exists()
        assert defaults() == before
        # Reinicio carga v2 persistida; SIGKILL prueba la limpieza de arranque y cleanup offline.
        agent = start()
        until(lambda: RESOLVER.exists(), description="DNS tras reinicio", process=agent)
        text = call("/sbin/ifconfig")
        interface = next(line.split(":")[0] for line in text.splitlines() if line.startswith("utun") and "mtu 1280" in line and "inet 10.77.0.2" in call("/sbin/ifconfig", line.split(":")[0]))
        phase("SIGKILL y cleanup offline")
        agent.kill()
        agent.wait(timeout=10)
        call(AGENT, "cleanup", "--config", config)
        assert run(["/sbin/ifconfig", interface], capture_output=True).returncode != 0
        assert not RESOLVER.exists()
        for family in ("inet", "inet6"):
            assert not any(interface in line.split() for line in call("/usr/sbin/netstat", "-rn", "-f", family).splitlines())
        assert defaults() == before
        # También se retira un residuo propio al arrancar, antes de reaplicar la configuración.
        RESOLVER.write_text(OWNER + "nameserver 192.0.2.53\n")
        issue(3, False)
        agent = start()
        until(lambda: not RESOLVER.exists(), description="limpieza DNS de arranque", process=agent)
        until(lambda: ask(work / "control.sock", "status").get("dns") == "none", description="estado v1 tras limpiar", process=agent)
        phase("parada limpia del agente")
        call(CLI, "down", "--socket", work / "control.sock")
        assert agent.wait(timeout=10) == 0
        print("macOS: utun, v1/v2, IPv4/IPv6 Noise, rutas, DNS, IPC y limpieza correctos")
    except BaseException:
        # Volcar mientras el agente sigue vivo; trap repetirá el diagnóstico si aquí hay otro fallo.
        log.flush()
        peerlog.flush()
        diagnostics(work)
        raise
    finally:
        for timer in timers:
            timer.cancel()
        for process in (agent, peer):
            if process is not None:
                terminate(process)
        for name in ("agent", "peer"):
            (work / f"{name}.pid").unlink(missing_ok=True)
        log.close()
        peerlog.close()


if __name__ == "__main__":
    def expired(signum, frame):
        raise TimeoutError("deadline global de la aceptación macOS")
    signal.signal(signal.SIGALRM, expired)
    signal.signal(signal.SIGTERM, expired)
    signal.alarm(40 if sys.argv[1] == "--diagnostics" else 540)
    if sys.argv[1] == "--diagnostics":
        diagnostics(Path(sys.argv[2]))
    else:
        main(Path(sys.argv[1]))
