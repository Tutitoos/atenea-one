# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Aceptación real: ejecutar exclusivamente mediante run.sh en el runner desechable."""
import ipaddress
import json
import os
from pathlib import Path
import re
import plistlib
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
    if (work / "multi-enabled").exists():
        base = Path("/Library/Application Support/Atenea")
        for index in range(3):
            network = f"77777777-7777-4777-8777-77770000000{index}"
            label = "one.atenea.agent" if index == 0 else f"one.atenea.network.{network[-8:]}"
            network_root = base / ("networks" if index == 1 else "test-networks")
            device = base / "legacy-device" if index == 0 else network_root / network / "device"
            config = base / "agent.toml" if index == 0 else device / "agent.toml"
            for path in (work / f"multi-{index}/peer.log",
                         Path("/var/log/atenea-agent.log") if not index else device / "agent.log"):
                if path.exists():
                    phase(f"log {path}")
                    print(path.read_text(errors="replace"), file=sys.stderr, flush=True)
            pid = work / f"multi-{index}/peer.pid"
            if pid.exists():
                try:
                    os.killpg(int(pid.read_text()), signal.SIGKILL)
                except ProcessLookupError:
                    pass
            run(["/bin/launchctl", "bootout", f"system/{label}"],
                capture_output=True, timeout=5)
            if config.exists():
                result = run([str(AGENT), "cleanup", "--config", str(config)],
                             capture_output=True, text=True, timeout=5)
                print(result.stdout, result.stderr, file=sys.stderr, flush=True)
                result.check_returncode()


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



def multi_networks(work):
    """Solo CI: legacy + dos servicios launchd, cada uno con utun y DNS reales."""
    assert sys.platform == "darwin" and os.getuid() == 0 and os.environ.get("GITHUB_ACTIONS") == "true"
    base = Path("/Library/Application Support/Atenea")
    root = base / "test-networks"
    installed = Path("/Library/Atenea/bin")
    daemons = Path("/Library/LaunchDaemons")
    assert not base.exists() and not installed.parent.exists(), "Runner no desechable"
    base.mkdir(mode=0o700)
    root.mkdir(mode=0o700)
    default_root = base / "networks"
    default_root.mkdir(mode=0o700)
    installed.mkdir(parents=True, mode=0o755)
    for name in ("atenea", "atenea-agent"):
        shutil.copyfile(BIN / name, installed / name)
        (installed / name).chmod(0o755)
    for name in ("atenea-launchd.sh", "atenea-uninstall.sh", "atenea-network.sh"):
        shutil.copyfile(REPO / "packaging/macos" / name, installed / name)
        (installed / name).chmod(0o755)
    legacy_plist = daemons / "one.atenea.agent.plist"
    assert not legacy_plist.exists()
    shutil.copyfile(REPO / "packaging/macos/one.atenea.agent.plist", legacy_plist)
    legacy_plist.chmod(0o644)
    before = defaults()
    instances = []
    timers = []
    peers = []
    logs = []
    foreign = daemons / "one.atenea.network.ffffffff.plist"
    foreign_data = plistlib.dumps({"Label": "one.someone.else", "ProgramArguments": ["/usr/bin/true"]})
    assert not foreign.exists()
    foreign.write_bytes(foreign_data)
    foreign.chmod(0o644)
    foreign_resolver = Path("/etc/resolver/foreign.internal")
    assert not foreign_resolver.exists()
    foreign_resolver.parent.mkdir(exist_ok=True)
    foreign_resolver.write_text("# Ajeno\nnameserver 192.0.2.53\n")
    (work / "multi-enabled").touch()
    unsafe = base / "unsafe-root"
    unsafe.mkdir()
    unsafe.chmod(0o777)
    linked = base / "linked-root"
    linked.symlink_to(root, target_is_directory=True)
    for args in (["77777777-7777-4777-8777-77770000000A", str(root)],
                 ["77777777-7777-4777-8777-777700000001", "/tmp"],
                 ["77777777-7777-4777-8777-777700000001", str(root) + "/../networks"],
                 ["77777777-7777-4777-8777-777700000001", str(unsafe)],
                 ["77777777-7777-4777-8777-777700000001", str(linked)]):
        result = run([str(installed / "atenea-launchd.sh"), *args], capture_output=True)
        assert result.returncode == 78, result
    try:
        for index in range(3):
            network = f"77777777-7777-4777-8777-77770000000{index}"
            short = network[-8:]
            home = work / f"multi-{index}"
            home.mkdir(mode=0o700)
            anchor = call(CLI, "root-init", "--out", home / "root.key")
            call(CLI, "issuer-init", "--root", home / "root.key", "--network-id",
                 network.replace("-", ""), "--serial", "1", "--out-dir", home / "issuer")
            network_root = default_root if index == 1 else root
            device_dir = base / "legacy-device" if index == 0 else network_root / network / "device"
            device_dir.mkdir(parents=True, mode=0o700)
            # mkdir(parents) puede adoptar el umask; fijar explícitamente cada red.
            if index:
                device_dir.parent.chmod(0o711)
            device = call(CLI, "device-init", "--dir", device_dir)
            device_id = re.search(r"device_id=([0-9a-f]+)", device)[1]
            call(CLI, "device-init", "--dir", home / "peer")
            offset = index * 64
            addr, peer_addr = f"10.77.0.{offset + 2}", f"10.77.0.{offset + 1}"
            addr6, peer6 = f"fd77::{index + 1}:2", f"fd77::{index + 1}:1"
            prefix6 = f"fd77::{index + 1}:0/112"
            domain = f"multi-{index}.internal"
            resolver = Path("/etc/resolver") / domain
            assert not resolver.exists()
            runtime = Path("/var/run/atenea") / short if index else Path("/var/run/atenea")
            config = device_dir / "agent.toml" if index else base / "agent.toml"
            config.write_text(f'''interface = "utun"
listen = "127.0.0.1:{51900 + index * 2}"
ipc_socket = "{runtime}/control.sock"
status_socket = "{runtime}/status.sock"
status_gid = 0
noise_key_file = "{device_dir}/noise.key"
identity_key_file = "{device_dir}/identity.key"
config_file = "{device_dir}/config.bin"
state_dir = "{device_dir}/state"
network_id = "{network.replace("-", "")}"
device_id = "{device_id}"
trust_anchor = "{anchor}"
''' + (f'resolver_owner = "{short}"\n' if index else ""))
            config.chmod(0o600)
            spec = home / "spec.toml"
            spec.write_text(f'''network_id = "{network.replace("-", "")}"
device_bundle = "{device_dir}/bundle.bin"
config_version = 1
validity_secs = 3600
mtu = 1280
tunnel_addrs = ["{addr}", "{addr6}"]
network_prefixes = ["10.77.0.{offset}/26", "{prefix6}"]
[[peer]]
bundle = "{home}/peer/bundle.bin"
addrs = ["{peer_addr}", "{peer6}"]
endpoints = ["127.0.0.1:{51901 + index * 2}"]
routes = []
[[rule]]
src = "any"
dst = "any"
[dns]
servers = ["{peer_addr}"]
domains = ["{domain}"]
''')
            call(CLI, "issue", "--issuer-key", home / "issuer/issuer.key",
                 "--issuer-cert", home / "issuer/issuer.cert", "--spec", spec,
                 "--out", device_dir / "config.bin")
            label = "one.atenea.agent" if index == 0 else f"one.atenea.network.{short}"
            plist = legacy_plist if index == 0 else daemons / f"{label}.plist"
            if index:
                metadata = device_dir.parent / "network.json"
                metadata.write_text(json.dumps(dict(
                    network_id=network, name=f"sintetica-{index}", server="control.example.test",
                    url="https://control.example.test", root_anchor_fingerprint="00" * 32,
                    device_id=device_id, device_fingerprint="00" * 16, interface="utun",
                    listen_port=51900 + index * 2, created=1, join_status="aprobada")))
                metadata.chmod(0o644)
            log = (home / "peer.log").open("w")
            logs.append(log)
            peer, timer = background([
                BIN / "atenea-testpeer", "--device-dir", home / "peer",
                "--agent-bundle", device_dir / "bundle.bin",
                "--listen", f"127.0.0.1:{51901 + index * 2}", "--own-addr", peer_addr,
                "--own-addr6", peer6, "--agent-addr", addr, "--agent-addr6", addr6,
                "--network-id", network.replace("-", ""), "--count", "2",
                "--timeout-secs", "180"], log, home, "peer", seconds=190)
            peers.append(peer)
            timers.append(timer)
            until(lambda: "ready" in (home / "peer.log").read_text(),
                  description=f"testpeer {index}", process=peer)
            instance = dict(label=label, plist=plist, runtime=runtime, config=config,
                            resolver=resolver, device=device_dir, addr=addr, root=network_root, network=network,
                            peer_addr=peer_addr, peer6=peer6, short=short, index=index)
            instances.append(instance)
            if index:
                call(CLI, "network", "--backend", "system", "--root", network_root, "up", network)
                generated = plistlib.loads(plist.read_bytes())
                assert generated["Label"] == label
                assert generated["KeepAlive"] == {"PathState": {str(config): True}}
                assert stat.S_IMODE(plist.stat().st_mode) == 0o644
                assert (plist.stat().st_uid, plist.stat().st_gid) == (0, 0)
            else:
                call("/bin/launchctl", "bootstrap", "system", plist)
            until(lambda: resolver.exists(), description=f"DNS red {index}")
            owner = OWNER if not index else f"# Atenea One: resolver administrado ({short})\n"
            assert resolver.read_text() == owner + f"nameserver {peer_addr}\n"
            interface = next(line.split(":")[0] for line in call("/sbin/ifconfig").splitlines()
                             if line.startswith("utun") and f"inet {addr} " in
                             call("/sbin/ifconfig", line.split(":")[0]))
            instance["interface"] = interface
            check_route(f"10.77.0.{offset}/26", interface)
            check_route(prefix6, interface)
            assert stat.S_IMODE((runtime / "control.sock").stat().st_mode) == 0o600
            assert stat.S_IMODE((runtime / "status.sock").stat().st_mode) == 0o660
        assert len({item["interface"] for item in instances}) == 3
        assert defaults() == before
        for item, peer in zip(instances, peers):
            until(lambda: echo(["/sbin/ping", "-n", "-t", "3", "-c", "1", "-W", "1000",
                               item["peer_addr"]]), description=f"IPv4 {item['label']}")
            until(lambda: echo(["/sbin/ping6", "-n", "-c", "1", item["peer6"]]),
                  description=f"IPv6 {item['label']}")
            assert peer.wait(timeout=15) == 0
        a, b = instances[1:]
        preserved = {path: path.read_bytes() for item in instances for path in
                     (item["config"], item["device"] / "noise.key", item["device"] / "identity.key")}
        def others_live(stopped):
            for item in instances:
                if item is stopped:
                    continue
                assert item["resolver"].exists()
                assert ask(item["runtime"] / "status.sock", "status")
                assert f"inet {item['addr']} " in call("/sbin/ifconfig", item["interface"])
        for item in (a, b):
            call(CLI, "network", "--root", item["root"], "down", item["network"])
            until(lambda: not item["resolver"].exists(), description=f"parada {item['label']}")
            call(AGENT, "cleanup", "--config", item["config"])
            assert run(["/sbin/ifconfig", item["interface"]], capture_output=True).returncode != 0
            others_live(item)
            call(CLI, "network", "--root", item["root"], "up", item["network"])
            until(lambda: item["resolver"].exists(), description=f"reinicio {item['label']}")
            item["interface"] = next(
                line.split(":")[0] for line in call("/sbin/ifconfig").splitlines()
                if line.startswith("utun") and f"inet {item['addr']} " in
                call("/sbin/ifconfig", line.split(":")[0]))
        # Actualización: reinicia servicios activos, pero no conecta una red detenida.
        call(CLI, "network", "--root", a["root"], "down", a["network"])
        until(lambda: not a["resolver"].exists(), description="red A detenida antes de actualización")
        call("/bin/sh", REPO / "packaging/macos/postinstall", "", "", "/")
        until(lambda: b["resolver"].exists() and instances[0]["resolver"].exists(),
              description="servicios tras actualización")
        assert run(["/bin/launchctl", "print", f"system/{a['label']}"],
                   capture_output=True).returncode != 0
        disabled = call("/bin/launchctl", "print-disabled", "system")
        assert f'"{a["label"]}" => true' in disabled, disabled
        call(CLI, "network", "--root", a["root"], "up", a["network"])
        until(lambda: a["resolver"].exists(), description="red A tras actualización")
        # El reinicio puede renumerar utun: recordar la identidad real antes de uninstall.
        for item in instances:
            item["interface"] = next(
                line.split(":")[0] for line in call("/sbin/ifconfig").splitlines()
                if line.startswith("utun") and f"inet {item['addr']} " in
                call("/sbin/ifconfig", line.split(":")[0]))
        call("/bin/sh", installed / "atenea-uninstall.sh")
        assert not installed.exists(), "Uninstall dejó ejecutables instalados"
        for item in instances:
            assert not item["plist"].exists()
            assert not item["resolver"].exists()
            assert not (item["runtime"] / "control.sock").exists()
            assert not (item["runtime"] / "status.sock").exists()
            assert item["device"].is_dir()
            assert run(["/sbin/ifconfig", item["interface"]], capture_output=True).returncode != 0
            assert run(["/bin/launchctl", "print", f"system/{item['label']}"],
                       capture_output=True).returncode != 0
        assert all(path.read_bytes() == data for path, data in preserved.items())
        assert foreign.read_bytes() == foreign_data
        assert foreign_resolver.read_text() == "# Ajeno\nnameserver 192.0.2.53\n"
        assert defaults() == before
        print("macOS: legacy + dos redes, Noise v4/v6, launchd independiente, actualización y uninstall")
    except BaseException:
        diagnostics(work)
        raise
    finally:
        for timer in timers:
            timer.cancel()
        for peer in peers:
            terminate(peer)
        for log in logs:
            log.close()
        for item in instances:
            run(["/bin/launchctl", "bootout", f"system/{item['label']}"], capture_output=True)
            run([str(AGENT), "cleanup", "--config", str(item["config"])], capture_output=True)
            item["plist"].unlink(missing_ok=True)
        foreign.unlink(missing_ok=True)
        foreign_resolver.unlink(missing_ok=True)
        shutil.rmtree(base)
        shutil.rmtree(installed.parent, ignore_errors=True)
        (work / "multi-enabled").unlink(missing_ok=True)


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
    if sys.platform != "darwin" or os.getuid() != 0 or os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Solo root en un runner macOS desechable de GitHub Actions")
    def expired(signum, frame):
        raise TimeoutError("deadline global de la aceptación macOS")
    signal.signal(signal.SIGALRM, expired)
    signal.signal(signal.SIGTERM, expired)
    signal.alarm(90 if sys.argv[1] == "--diagnostics" else 840)
    if sys.argv[1] == "--diagnostics":
        diagnostics(Path(sys.argv[2]))
    else:
        main(Path(sys.argv[1]))
        multi_networks(Path(sys.argv[1]))
