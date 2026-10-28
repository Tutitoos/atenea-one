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

use serde_json::Value;
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

/// Estado global y su resumen: lo comparten la bandeja y la ventana (el frontend no lo recalcula).
fn assess(s: &Value) -> Assessment {
    let mut health = Health::Ok;
    let mut why: Vec<String> = Vec::new();
    let mut worsen = |h: Health, text: Option<String>| {
        health = health.max(h);
        why.extend(text);
    };
    let cfg = &s["config"];
    if cfg["lease_active"].as_bool() != Some(true) {
        worsen(Health::Down, Some("sin lease activo".into()));
    }
    if let Some(e) = cfg["last_error"].as_str() {
        worsen(Health::Down, Some(format!("configuración: {e}")));
    }
    if let Some(e) = s["sync"]["last_error"].as_str() {
        worsen(Health::Degraded, Some(format!("control plane: {e}")));
    }
    if let Some(e) = s["coordinator"]["last_error"].as_str() {
        worsen(Health::Degraded, Some(format!("coordinator: {e}")));
    }
    let peers = s["peers"].as_array().map_or(&[][..], Vec::as_slice);
    if peers.iter().any(|p| p["path"].as_str() != Some("direct")) {
        worsen(Health::Degraded, None);
    }
    let text = match health {
        Health::Ok if peers.is_empty() => "Conectado (sin otros dispositivos)".to_string(),
        Health::Ok => "Conectado".to_string(),
        Health::Degraded => format!(
            "Degradado: {}",
            why.first()
                .map_or("algún dispositivo por relay o sin camino", String::as_str)
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

fn read_source(label: &str, path: &str) -> Option<Source> {
    let v: Value = serde_json::from_str(&ask(path).ok()?).ok()?;
    let a = assess(&v);
    Some(Source {
        label: label.into(),
        health: a.health,
        text: a.text,
    })
}

/// Redes unidas (id, nombre) para la bandeja.
fn joined_networks() -> Vec<(String, String)> {
    let Ok(Value::Array(list)) =
        networks::list().and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|n| {
            Some((
                n["network_id"].as_str()?.to_string(),
                n["name"].as_str()?.to_string(),
            ))
        })
        .filter(|(id, _)| networks::valid_uuid(id))
        .collect()
}

fn tray_menu(
    app: &AppHandle,
    lines: &[(String, String)],
) -> tauri::Result<(Menu<Wry>, Vec<MenuItem<Wry>>)> {
    let items: Vec<MenuItem<Wry>> = lines
        .iter()
        .enumerate()
        .map(|(i, (_, text))| {
            MenuItem::with_id(app, format!("state{i}"), text, false, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let open = MenuItem::with_id(app, "open", "Abrir Atenea One", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let mut all: Vec<&dyn IsMenuItem<Wry>> =
        items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    all.extend([&open as &dyn IsMenuItem<Wry>, &quit]);
    Ok((Menu::with_items(app, &all)?, items))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            network_list,
            network_legacy,
            network_inspect,
            network_apply
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let base = Image::from_bytes(TRAY_PNG)?;
            let (menu, mut items) =
                tray_menu(app.handle(), &[("*".into(), "Comprobando…".into())])?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon(&base, Health::Stopped))
                .tooltip("Atenea One")
                .menu(&menu)
                .on_menu_event(|app, ev| match ev.id.as_ref() {
                    "open" => show_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            if std::env::args().any(|a| a == "--open") {
                show_window(app.handle());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut nets: Vec<(String, String)> = Vec::new();
                let mut labels: Vec<String> = vec!["*".into()];
                let mut tick = 0u32;
                loop {
                    // La lista de redes se consulta con el CLI: cada 5 lecturas (~10 s), no en cada una.
                    if tick.is_multiple_of(5) {
                        nets = joined_networks();
                    }
                    tick = tick.wrapping_add(1);
                    let mut sources: Vec<Source> = Vec::new();
                    for (id, name) in &nets {
                        if let Some(s) = network_socket(id).ok().and_then(|p| read_source(name, &p))
                        {
                            sources.push(s);
                        }
                    }
                    let (health, text, lines) = summarize(nets.len(), &sources);
                    let keys: Vec<String> = lines.iter().map(|(k, _)| k.clone()).collect();
                    if keys != labels {
                        // Cambió el conjunto de redes que responden: se reconstruye el menú (no en cada lectura,
                        // para no cerrar un menú que el usuario tenga abierto).
                        if let Ok((menu, new_items)) = tray_menu(&handle, &lines) {
                            let _ = tray.set_menu(Some(menu));
                            items = new_items;
                            labels = keys;
                        }
                    }
                    for (item, (_, line)) in items.iter().zip(&lines) {
                        let _ = item.set_text(line);
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
        assert_eq!(assess(&status(true, "none")).health, Health::Degraded);
        let a = assess(&status(false, "direct"));
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
