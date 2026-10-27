// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// Icono de barra de menús: estado del agente leído del socket de solo lectura. No envía órdenes.
import AppKit

let statusSocket = ProcessInfo.processInfo.environment["ATENEA_STATUS_SOCKET"] ?? "/var/run/atenea/status.sock"
let cliPath = "/Library/Atenea/bin/atenea"
let panelPort = 8790

enum Health: Int { case ok = 0, degraded, down, stopped }

struct Row { let title: String; let detail: String; let health: Health }
struct Snapshot { var health: Health; var headline: String; var addrs: String?; var rows: [Row] }

func readStatus() -> [String: Any]? {
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else { return nil }
    defer { close(fd) }
    var tv = timeval(tv_sec: 3, tv_usec: 0)
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
    var addr = sockaddr_un()
    addr.sun_family = sa_family_t(AF_UNIX)
    let path = Array(statusSocket.utf8)
    guard path.count < MemoryLayout.size(ofValue: addr.sun_path) else { return nil }
    withUnsafeMutableBytes(of: &addr.sun_path) { raw in
        raw.copyBytes(from: path)
        raw[path.count] = 0
    }
    let connected = withUnsafePointer(to: &addr) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
    }
    guard connected == 0 else { return nil }
    let request = Array("status\n".utf8)
    guard send(fd, request, request.count, 0) == request.count else { return nil }
    var data = Data()
    var buf = [UInt8](repeating: 0, count: 16384)
    while data.count < 4_000_000 {
        let n = recv(fd, &buf, buf.count, 0)
        if n <= 0 { break }
        data.append(contentsOf: buf[0..<n])
        if buf[n - 1] == 10 { break }
    }
    guard let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any], json["error"] == nil else { return nil }
    return json
}

func bytes(_ n: Double) -> String {
    var v = n
    var i = 0
    let units = ["B", "KiB", "MiB", "GiB", "TiB"]
    while v >= 1024 && i < units.count - 1 { v /= 1024; i += 1 }
    return String(format: i == 0 ? "%.0f %@" : "%.1f %@", v, units[i])
}

func ago(_ s: Double) -> String { s < 120 ? "\(Int(s)) s" : s < 7200 ? "\(Int(s / 60)) min" : "\(Int(s / 3600)) h" }

func snapshot() -> Snapshot {
    guard let s = readStatus() else {
        return Snapshot(health: .stopped, headline: "Agente parado o sin acceso al estado", addrs: nil, rows: [])
    }
    var health = Health.ok
    var problems: [String] = []
    func worsen(_ h: Health, _ why: String?) {
        if h.rawValue > health.rawValue { health = h }
        if let why = why { problems.append(why) }
    }
    let cfg = s["config"] as? [String: Any] ?? [:]
    if cfg["lease_active"] as? Bool != true { worsen(.down, "sin lease activo") }
    if let e = cfg["last_error"] as? String { worsen(.down, "configuración: \(e)") }
    if let e = (s["sync"] as? [String: Any])?["last_error"] as? String { worsen(.degraded, "control plane: \(e)") }
    if let e = (s["coordinator"] as? [String: Any])?["last_error"] as? String { worsen(.degraded, "coordinator: \(e)") }

    var rows: [Row] = []
    for p in s["peers"] as? [[String: Any]] ?? [] {
        let key = String((p["public_key"] as? String ?? "?").prefix(8))
        let title = (p["addrs"] as? [String])?.first ?? key
        let path = p["path"] as? String ?? "none"
        let h: Health = path == "direct" ? .ok : (path == "none" ? .down : .degraded)
        if h != .ok { worsen(.degraded, nil) }
        var detail = path
        detail += " · ↓\(bytes(p["rx_bytes"] as? Double ?? 0)) ↑\(bytes(p["tx_bytes"] as? Double ?? 0))"
        if p["session"] is String, let hs = p["last_handshake_secs_ago"] as? Double { detail += " · sesión hace \(ago(hs))" }
        rows.append(Row(title: title, detail: detail, health: h))
    }
    let headline: String
    switch health {
    case .ok: headline = rows.isEmpty ? "Conectado (sin pares)" : "Conectado"
    case .degraded: headline = problems.first.map { "Degradado: \($0)" } ?? "Degradado: algún par por relay o sin camino"
    default: headline = "Sin conexión: \(problems.first ?? "sin detalle")"
    }
    return Snapshot(health: health, headline: headline, addrs: (s["addrs"] as? [String])?.joined(separator: ", "), rows: rows)
}

func color(_ h: Health) -> NSColor {
    switch h {
    case .ok: return .systemGreen
    case .degraded: return .systemOrange
    case .down: return .systemRed
    case .stopped: return .systemGray
    }
}

final class Delegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    let helmet: NSImage? = Bundle.main.url(forResource: "icon", withExtension: "svg").flatMap { NSImage(contentsOf: $0) }
    var panel: Process?
    var menuOpen = false
    var last = Snapshot(health: .stopped, headline: "Comprobando…", addrs: nil, rows: [])

    func applicationDidFinishLaunching(_ notification: Notification) {
        item.button?.image = icon(.stopped)
        rebuild()
        Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in self?.poll() }
        poll()
    }

    func applicationWillTerminate(_ notification: Notification) { panel?.terminate() }

    func poll() {
        DispatchQueue.global().async {
            let snap = snapshot()
            DispatchQueue.main.async {
                self.last = snap
                self.item.button?.image = self.icon(snap.health)
                if !self.menuOpen { self.rebuild() }
            }
        }
    }

    func icon(_ h: Health) -> NSImage {
        let size = NSSize(width: 18, height: 18)
        let image = NSImage(size: size, flipped: false) { rect in
            if let helmet = self.helmet { helmet.draw(in: rect) }
            let dot = NSRect(x: rect.maxX - 6, y: 0, width: 6, height: 6)
            NSColor.black.withAlphaComponent(0.55).setFill()
            NSBezierPath(ovalIn: dot.insetBy(dx: -1, dy: -1)).fill()
            color(h).setFill()
            NSBezierPath(ovalIn: dot).fill()
            return true
        }
        image.isTemplate = false
        return image
    }

    func line(_ text: String, _ h: Health? = nil) -> NSMenuItem {
        let m = NSMenuItem(title: text, action: nil, keyEquivalent: "")
        if let h = h {
            let s = NSMutableAttributedString(string: "● ", attributes: [.foregroundColor: color(h)])
            s.append(NSAttributedString(string: text))
            m.attributedTitle = s
        }
        return m
    }

    func rebuild() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.delegate = self
        menu.addItem(line(last.headline, last.health))
        if let a = last.addrs, !a.isEmpty { menu.addItem(line("Mis direcciones: \(a)")) }
        if !last.rows.isEmpty { menu.addItem(.separator()) }
        for r in last.rows { menu.addItem(line("\(r.title) — \(r.detail)", r.health)) }
        menu.addItem(.separator())
        let open = NSMenuItem(title: "Abrir panel…", action: #selector(openPanel), keyEquivalent: "")
        open.target = self
        open.isEnabled = FileManager.default.isExecutableFile(atPath: cliPath)
        menu.addItem(open)
        let quit = NSMenuItem(title: "Salir", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        menu.addItem(quit)
        item.menu = menu
    }

    func menuWillOpen(_ menu: NSMenu) { menuOpen = true }
    func menuDidClose(_ menu: NSMenu) { menuOpen = false }

    @objc func openPanel() {
        if panel?.isRunning != true {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: cliPath)
            p.arguments = ["ui", "--socket", statusSocket, "--port", String(panelPort)]
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            try? p.run()
            panel = p
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            NSWorkspace.shared.open(URL(string: "http://127.0.0.1:\(panelPort)/")!)
        }
    }
}

// `--print`: una lectura del estado sin interfaz (prueba de humo y diagnóstico).
if CommandLine.arguments.dropFirst().first == "--print" {
    let s = snapshot()
    print("estado: \(s.health) | \(s.headline)")
    if let a = s.addrs { print("direcciones: \(a)") }
    for r in s.rows { print("par \(r.title): \(r.detail) [\(r.health)]") }
    exit(s.health == .stopped ? 1 : 0)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = Delegate()
app.delegate = delegate
app.run()
