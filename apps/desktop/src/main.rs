// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Aplicación de escritorio: icono en la bandeja y una ventana con el estado de las redes.
//!
//! Lee los sockets de estado sin privilegios (`status`). Las operaciones que cambian el sistema
//! (unirse, conectar, desconectar, salir de una red) no las hace la app: pide al sistema operativo la
//! autorización de administrador y ejecuta el CLI (`networks.rs`, ADR-013 §3.4 opción A). El frontend
//! solo puede invocar las órdenes listadas en `generate_handler!`, sin permisos de plugins (`capabilities/`).
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod networks;
mod notify;
mod panel;
mod ping;
mod tray_actions;

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent, Wry};

const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");
const POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Health {
    Ok,
    Degraded,
    Down,
    Stopped,
}

#[cfg(unix)]
fn ask(path: &str) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut s =
        std::os::unix::net::UnixStream::connect(path).map_err(|e| format!("{path}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    s.write_all(b"status\n").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s.take(4 << 20))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    checked(line)
}

#[cfg(windows)]
fn ask(path: &str) -> Result<String, String> {
    use std::io::Write;
    let mut pipe = atenea_windows::connect_pipe(std::path::Path::new(path))
        .map_err(|e| format!("{path}: {e}"))?;
    pipe.write_all(b"status\n").map_err(|e| e.to_string())?;
    checked(atenea_windows::pipe_reply(pipe, Duration::from_secs(5)).map_err(|e| e.to_string())?)
}

/// Rechaza una respuesta vacía, inválida o de error del agente.
fn checked(line: String) -> Result<String, String> {
    let v: Value =
        serde_json::from_str(&line).map_err(|_| "respuesta vacía o JSON inválido".to_string())?;
    match v.get("error") {
        Some(e) => Err(format!(
            "agente: {}",
            e.as_str().unwrap_or("orden rechazada")
        )),
        None => Ok(line.trim().to_string()),
    }
}

/// Socket de estado de una red unida: `<run>/<id8>/status.sock`, el mismo que usa el CLI.
fn network_socket(id: &str) -> Result<String, String> {
    if cfg!(windows) {
        return Err("las redes múltiples no están soportadas en Windows".into());
    }
    if !networks::valid_uuid(id) {
        return Err("identificador de red no válido".into());
    }
    let run = std::env::var("ATENEA_RUN_DIR").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/var/run/atenea".into()
        } else {
            "/run/atenea".into()
        }
    });
    Ok(format!("{run}/{}/status.sock", networks::short_id(id)))
}

/// Estado de una red unida con `atenea network join`. No hay instancia «predeterminada»: sin red, no hay estado.
#[tauri::command]
async fn status(network: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = network_socket(&network)?;
        let status: Value = serde_json::from_str(&ask(&path)?).map_err(|e| e.to_string())?;
        let a = assess(&status);
        Ok(serde_json::json!({
            "status": status,
            "health": { "level": a.level(), "text": a.text, "reasons": a.reasons },
        })
        .to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Redes unidas (`atenea network list --json`, sin privilegios).
#[tauri::command]
async fn network_list() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(networks::list)
        .await
        .map_err(|e| e.to_string())?
}

/// Datos de un código pegado, para la confirmación previa. No toca el disco ni la red.
#[tauri::command]
async fn network_inspect(code: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || networks::inspect(&code))
        .await
        .map_err(|e| e.to_string())?
}

/// ¿Queda una instalación heredada pendiente de importar? (`atenea network legacy --json`, sin privilegios).
#[tauri::command]
async fn network_legacy() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(networks::legacy)
        .await
        .map_err(|e| e.to_string())?
}

/// Unirse, conectar, desconectar, salir de una red o importar la instalación heredada (`op = "migrate"`,
/// con `name` opcional). El sistema operativo pide la autorización.
#[tauri::command]
async fn network_apply(
    op: String,
    network: Option<String>,
    code: Option<String>,
    name: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        networks::apply(&op, network.as_deref(), code.as_deref(), name.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn joined_network(network: &str) -> Result<Value, String> {
    if !networks::valid_uuid(network) {
        return Err("identificador de red no válido".into());
    }
    let list: Value = serde_json::from_str(&networks::list()?).map_err(|e| e.to_string())?;
    list.as_array()
        .and_then(|nets| {
            nets.iter()
                .find(|n| n["network_id"].as_str() == Some(network))
        })
        .cloned()
        .ok_or_else(|| "la red no está unida".into())
}

#[tauri::command]
async fn device_ping(network: String, address: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        joined_network(&network)?;
        let status: Value =
            serde_json::from_str(&ask(&network_socket(&network)?)?).map_err(|e| e.to_string())?;
        ping::run(ping::validate(&address, &status)?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_panel(network: String, device_id: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let net = joined_network(&network)?;
        let url = panel::url(
            net["url"].as_str().ok_or("falta la URL del servidor")?,
            &network,
            device_id.as_deref(),
        )?;
        panel::open(&url)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn notifications_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|p| p.join("notifications.json"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn notifications_get(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || notify::get(&notifications_path(&app)?))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn notifications_set(app: AppHandle, on: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || notify::set(&notifications_path(&app)?, on))
        .await
        .map_err(|e| e.to_string())?
}

struct Assessment {
    health: Health,
    text: String,
    reasons: Vec<String>,
}

impl Assessment {
    fn level(&self) -> &'static str {
        match self.health {
            Health::Ok => "ok",
            Health::Degraded => "degraded",
            Health::Down | Health::Stopped => "down",
        }
    }
}

/// Segundos que se tolera un lease perdido antes de darlo por caída. Tras suspender el equipo el agente
/// lo caduca y tarda en renovarlo (sincroniza cada ~60 s y la red tarda en volver).
const LEASE_GRACE_SECS: i64 = 90;

/// Estado global y su resumen: lo comparten la bandeja y la ventana (el frontend no lo recalcula).
fn assess(s: &Value) -> Assessment {
    static LOST_SINCE: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(Default::default);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs() as i64);
    // Una entrada por red (cada una tiene su propia clave pública) con el instante en que se vio el lease perdido.
    let key = s["public_key"].as_str().unwrap_or_default().to_string();
    let mut lost = LOST_SINCE.lock().unwrap_or_else(|e| e.into_inner());
    let since = if s["config"]["lease_active"].as_bool() == Some(true) {
        lost.remove(&key);
        None
    } else {
        Some(*lost.entry(key).or_insert(now))
    };
    drop(lost);
    assess_at(s, now, since)
}

fn assess_at(s: &Value, now: i64, lease_lost_since: Option<i64>) -> Assessment {
    let mut health = Health::Ok;
    let mut why: Vec<String> = Vec::new();
    let mut worsen = |h: Health, text: Option<String>| {
        health = health.max(h);
        why.extend(text);
    };
    let cfg = &s["config"];
    let lease_lost = cfg["lease_active"].as_bool() != Some(true);
    let reconnecting = lease_lost && lease_lost_since.is_some_and(|t| now - t < LEASE_GRACE_SECS);
    if reconnecting {
        worsen(Health::Degraded, None);
    } else if lease_lost {
        worsen(Health::Down, Some("sin lease activo".into()));
    }
    if let Some(e) = cfg["last_error"].as_str() {
        // «expired» es la propia caducidad del lease: se explica con «Reconectando…», no con un error.
        if !(reconnecting && e == "expired") {
            worsen(Health::Down, Some(format!("configuración: {e}")));
        }
    }
    if let Some(e) = s["sync"]["last_error"].as_str() {
        worsen(Health::Degraded, Some(format!("control plane: {e}")));
    }
    if let Some(e) = s["coordinator"]["last_error"].as_str() {
        worsen(Health::Degraded, Some(format!("coordinator: {e}")));
    }
    if let Some(expiry) = s["server"]["issuer_cert_not_after_unix"].as_i64() {
        let left = expiry.saturating_sub(now);
        if left <= 0 {
            worsen(
                Health::Down,
                Some("El certificado de la red ha caducado".into()),
            );
        } else if left <= 14 * 86400 {
            worsen(
                Health::Degraded,
                Some(format!(
                    "El certificado de la red caduca en {} días",
                    (left + 86399) / 86400
                )),
            );
        }
    }
    let peers = s["peers"].as_array().map_or(&[][..], Vec::as_slice);
    if peers
        .iter()
        .any(|p| matches!(p["path"].as_str(), Some("relay-udp" | "relay-tls")))
    {
        worsen(Health::Degraded, None);
    }
    let text = match health {
        Health::Ok if peers.is_empty() => "Conectado (sin otros dispositivos)".to_string(),
        Health::Ok => "Conectado".to_string(),
        Health::Degraded if reconnecting => "Reconectando…".to_string(),
        Health::Degraded => format!(
            "Degradado: {}",
            why.first()
                .map_or("algún dispositivo por relay", String::as_str)
        ),
        _ => format!(
            "Sin conexión: {}",
            why.first().map_or("sin detalle", String::as_str)
        ),
    };
    Assessment {
        health,
        text,
        reasons: why,
    }
}

/// Icono de la bandeja: el casco con un punto del color del estado en la esquina inferior derecha.
fn tray_icon(base: &Image<'_>, health: Health) -> Image<'static> {
    let (w, h) = (base.width(), base.height());
    let mut px = base.rgba().to_vec();
    let r = w as f32 * 0.17;
    let (cx, cy) = (w as f32 - r - 1.0, h as f32 - r - 1.0);
    let color: [u8; 4] = match health {
        Health::Ok => [0x34, 0xc7, 0x59, 255],
        Health::Degraded => [0xff, 0x9f, 0x0a, 255],
        Health::Down => [0xff, 0x45, 0x3a, 255],
        Health::Stopped => [0x8e, 0x8e, 0x93, 255],
    };
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let paint = if d <= r {
                Some(color)
            } else if d <= r + 1.5 {
                Some([0, 0, 0, 140])
            } else {
                None
            };
            if let Some(c) = paint {
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    Image::new_owned(px, w, h)
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Lo que responde cada red unida.
struct Source {
    label: String,
    health: Health,
    text: String,
}

/// Icono, tooltip y líneas del menú a partir de las redes que **responden** (`joined` = unidas). Una red
/// detenida a propósito no responde y no cuenta: no tiene que poner rojo el icono de las que sí están conectadas.
fn summarize(joined: usize, sources: &[Source]) -> (Health, String, Vec<(String, String)>) {
    match sources {
        [] => {
            let text = if joined == 0 {
                "Sin redes"
            } else {
                "Redes paradas o sin acceso al estado"
            };
            (
                Health::Stopped,
                text.into(),
                vec![("*".into(), text.into())],
            )
        }
        [one] => (
            one.health,
            one.text.clone(),
            vec![("*".into(), one.text.clone())],
        ),
        many => {
            let worst = many.iter().max_by_key(|s| s.health).expect("no vacío");
            (
                worst.health,
                format!("{} redes · {}", many.len(), worst.text),
                many.iter()
                    .map(|s| (s.label.clone(), format!("{}: {}", s.label, s.text)))
                    .collect(),
            )
        }
    }
}

/// Redes unidas y su estado persistido, actualizado por el CLI.
fn joined_networks() -> Vec<Value> {
    let Ok(Value::Array(list)) =
        networks::list().and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    else {
        return Vec::new();
    };
    list.into_iter()
        .filter(|n| n["network_id"].as_str().is_some_and(networks::valid_uuid))
        .collect()
}

fn tray_menu(
    app: &AppHandle,
    lines: &[(String, String)],
    actions: &[(String, String)],
) -> tauri::Result<Menu<Wry>> {
    let items: Vec<MenuItem<Wry>> = lines
        .iter()
        .enumerate()
        .map(|(i, (_, text))| {
            MenuItem::with_id(app, format!("state{i}"), text, false, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let actions: Vec<MenuItem<Wry>> = actions
        .iter()
        .map(|(id, text)| MenuItem::with_id(app, id, text, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let open = MenuItem::with_id(app, "open", "Abrir Atenea One", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let mut all: Vec<&dyn IsMenuItem<Wry>> =
        items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    all.extend(actions.iter().map(|i| i as &dyn IsMenuItem<Wry>));
    all.extend([&open as &dyn IsMenuItem<Wry>, &quit]);
    Menu::with_items(app, &all)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            network_list,
            network_legacy,
            network_inspect,
            network_apply,
            device_ping,
            open_panel,
            notifications_get,
            notifications_set
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let base = Image::from_bytes(TRAY_PNG)?;
            let menu = tray_menu(app.handle(), &[("*".into(), "Comprobando…".into())], &[])?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon(&base, Health::Stopped))
                .tooltip("Atenea One")
                .menu(&menu)
                .on_menu_event(|app, ev| match ev.id.as_ref() {
                    "open" => show_window(app),
                    "quit" => app.exit(0),
                    id => {
                        let id = id.to_string();
                        std::thread::spawn(move || {
                            if let Some((op, network)) = id.split_once(':') {
                                let result = match op {
                                    "up" | "down" => {
                                        networks::apply(op, Some(network), None, None).map(|_| ())
                                    }
                                    "copy" => tray_actions::copy(network),
                                    _ => return,
                                };
                                if let Err(error) = result {
                                    eprintln!("acción de bandeja: {error}");
                                }
                            }
                        });
                    }
                })
                .build(app)?;
            if std::env::args().any(|a| a == "--open") {
                show_window(app.handle());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut nets: Vec<Value> = Vec::new();
                let mut previous_menu = Vec::new();
                let mut notifications = std::collections::HashMap::<String, notify::State>::new();
                let mut tick = 0u32;
                loop {
                    // La lista de redes se consulta con el CLI: cada 5 lecturas (~10 s), no en cada una.
                    if tick.is_multiple_of(5) {
                        nets = joined_networks();
                    }
                    tick = tick.wrapping_add(1);
                    let mut sources: Vec<Source> = Vec::new();
                    let mut actions = Vec::new();
                    let clipboard = tray_actions::clipboard_program().is_some();
                    let enabled = notifications_path(&handle)
                        .and_then(|p| notify::get(&p))
                        .unwrap_or(false);
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |t| t.as_secs() as i64);
                    notifications
                        .retain(|id, _| nets.iter().any(|n| n["network_id"].as_str() == Some(id)));
                    for net in &nets {
                        let id = net["network_id"].as_str().unwrap_or("");
                        let name = net["name"].as_str().unwrap_or("Red");
                        let value: Option<Value> = network_socket(id)
                            .ok()
                            .and_then(|p| ask(&p).ok())
                            .and_then(|s| serde_json::from_str(&s).ok());
                        if let Some(value) = &value {
                            let assessment = assess(value);
                            sources.push(Source {
                                label: name.into(),
                                health: assessment.health,
                                text: assessment.text,
                            });
                        }
                        actions.extend(tray_actions::entries(net, value.as_ref(), clipboard));
                        for message in notifications.entry(id.into()).or_default().sample(
                            net["state"].as_str().unwrap_or("desconocida"),
                            value.as_ref(),
                            now,
                        ) {
                            if enabled {
                                notify::deliver(&format!("{name}: {message}"));
                            }
                        }
                    }
                    let (health, text, lines) = summarize(nets.len(), &sources);
                    let key: Vec<_> = lines.iter().chain(&actions).cloned().collect();
                    if key != previous_menu {
                        // No cerrar un menú abierto si no han cambiado sus entradas o etiquetas.
                        if let Ok(menu) = tray_menu(&handle, &lines, &actions) {
                            let _ = tray.set_menu(Some(menu));
                            previous_menu = key;
                        }
                    }
                    let _ = tray.set_icon(Some(tray_icon(&base, health)));
                    let _ = tray.set_tooltip(Some(&text));
                    std::thread::sleep(POLL);
                }
            });
            Ok(())
        })
        // Cerrar la ventana la oculta; la aplicación sigue en la bandeja hasta «Salir».
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error al ejecutar Atenea One");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(lease: bool, path: &str) -> Value {
        json!({"config": {"lease_active": lease, "last_error": null}, "sync": {"last_error": null},
               "coordinator": {"last_error": null}, "peers": [{"path": path}]})
    }

    #[test]
    fn health_follows_lease_errors_and_paths() {
        assert_eq!(assess(&status(true, "direct")).health, Health::Ok);
        assert_eq!(assess(&status(true, "relay-udp")).health, Health::Degraded);
        assert_eq!(assess(&status(true, "none")).health, Health::Ok);
        assert_eq!(assess(&status(true, "probing")).health, Health::Ok);
        assert_eq!(assess(&status(true, "relay-tls")).health, Health::Degraded);
        // Un lease perdido hace tiempo es una caída real.
        let long_ago = Some(1_800_000_000 - 1000);
        let a = assess_at(&status(false, "direct"), 1_800_000_000, long_ago);
        assert_eq!(
            (a.health, a.text.as_str(), a.level()),
            (Health::Down, "Sin conexión: sin lease activo", "down")
        );
        assert_eq!(a.reasons, ["sin lease activo"]);
        let mut s = status(true, "direct");
        s["sync"]["last_error"] = json!("acceso denegado");
        let a = assess(&s);
        assert_eq!(
            (a.health, a.text.as_str()),
            (
                Health::Degraded,
                "Degradado: control plane: acceso denegado"
            )
        );
    }

    #[test]
    fn certificate_warning_boundaries() {
        let now = 1_800_000_000;
        for (seconds, expected) in [
            (14 * 86400 + 1, Health::Ok),
            (14 * 86400, Health::Degraded),
            (3 * 86400, Health::Degraded),
            (1, Health::Degraded),
            (0, Health::Down),
            (-1, Health::Down),
        ] {
            let mut s = status(true, "direct");
            s["server"] = json!({"issuer_cert_not_after_unix": now + seconds});
            let assessment = assess_at(&s, now, None);
            assert_eq!(assessment.health, expected);
            if seconds <= 0 {
                assert_eq!(assessment.reasons, ["El certificado de la red ha caducado"]);
            } else if seconds <= 14 * 86400 {
                assert_eq!(
                    assessment.reasons,
                    [format!(
                        "El certificado de la red caduca en {} días",
                        (seconds + 86399) / 86400
                    )]
                );
            } else {
                assert!(assessment.reasons.is_empty());
            }
        }
        for server in [
            Value::Null,
            json!({}),
            json!({"issuer_cert_not_after_unix": null}),
        ] {
            let mut s = status(true, "direct");
            s["server"] = server;
            assert_eq!(assess_at(&s, now, None).health, Health::Ok);
        }
        assert_eq!(
            assess_at(&status(true, "direct"), now, None).health,
            Health::Ok
        );
    }

    #[test]
    fn lost_lease_is_reconnecting_only_during_grace() {
        let now = 1_800_000_000;
        // Tras despertar, el agente caduca el lease y la configuración antes de renovarlos.
        let mut s = status(false, "none");
        s["config"]["last_error"] = json!("expired");
        s["sync"]["last_error"] = json!("error de red");
        for since in [now, now - (LEASE_GRACE_SECS - 1)] {
            let a = assess_at(&s, now, Some(since));
            assert_eq!(
                (a.health, a.text.as_str()),
                (Health::Degraded, "Reconectando…")
            );
            assert!(
                !a.reasons
                    .iter()
                    .any(|r| r.contains("expired") || r == "sin lease activo")
            );
        }
        let a = assess_at(&s, now, Some(now - LEASE_GRACE_SECS));
        assert_eq!(a.health, Health::Down);
        assert_eq!(a.text, "Sin conexión: sin lease activo");
        // Un error de configuración distinto de la caducidad sigue siendo una caída inmediata.
        let mut s = status(false, "none");
        s["config"]["last_error"] = json!("firma inválida");
        assert_eq!(assess_at(&s, now, Some(now)).health, Health::Down);
        // Con el lease activo no hay gracia que valga.
        assert_eq!(
            assess_at(&status(true, "direct"), now, Some(now - 1000)).health,
            Health::Ok
        );
    }

    #[test]
    fn agent_errors_and_garbage_are_rejected() {
        assert!(checked(r#"{"error":"no autorizado"}"#.into()).is_err());
        assert!(checked(String::new()).is_err());
        assert!(checked(r#"{"peers":[]}"#.into()).is_ok());
    }

    fn src(label: &str, health: Health, text: &str) -> Source {
        Source {
            label: label.into(),
            health,
            text: text.into(),
        }
    }

    #[test]
    fn tray_summary_counts_only_sources_that_answer() {
        // Sin redes unidas: gris y «Sin redes».
        let (h, text, lines) = summarize(0, &[]);
        assert_eq!((h, text.as_str()), (Health::Stopped, "Sin redes"));
        assert_eq!(lines, [("*".to_string(), "Sin redes".to_string())]);

        // Redes unidas pero ninguna responde (paradas a propósito): también gris, sin rojo.
        let (h, text, _) = summarize(2, &[]);
        assert_eq!(
            (h, text.as_str()),
            (Health::Stopped, "Redes paradas o sin acceso al estado")
        );

        // Una red detenida no pone rojo el icono de la que sí responde.
        let (h, _, lines) = summarize(2, &[src("casa", Health::Ok, "Conectado")]);
        assert_eq!((h, lines.len()), (Health::Ok, 1));

        // Una sola fuente: el mismo texto de siempre, sin prefijo.
        let (h, text, lines) = summarize(1, &[src("casa", Health::Ok, "Conectado")]);
        assert_eq!((h, text.as_str()), (Health::Ok, "Conectado"));
        assert_eq!(lines, [("*".to_string(), "Conectado".to_string())]);

        // Varias: gana la peor, una línea por red con su nombre.
        let (h, text, lines) = summarize(
            2,
            &[
                src("casa", Health::Ok, "Conectado"),
                src("trabajo", Health::Degraded, "Degradado: por relay"),
            ],
        );
        assert_eq!(
            (h, text.as_str()),
            (Health::Degraded, "2 redes · Degradado: por relay")
        );
        assert_eq!(
            lines[1],
            (
                "trabajo".to_string(),
                "trabajo: Degradado: por relay".to_string()
            )
        );
    }
}
