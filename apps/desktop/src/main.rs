// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Aplicación de escritorio: icono en la bandeja y una ventana con el estado del agente.
//!
//! Solo lee el socket de estado (`status`); no envía órdenes de control. La única orden que
//! el frontend puede invocar es `status`, y no hay permisos de plugins (ver `capabilities/`).
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use serde_json::Value;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");
const POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Health {
    Ok,
    Degraded,
    Down,
    Stopped,
}

fn socket_path() -> String {
    std::env::var("ATENEA_STATUS_SOCKET").unwrap_or_else(|_| {
        if cfg!(windows) {
            r"\\.\pipe\atenea-status".into()
        } else if cfg!(target_os = "macos") {
            "/var/run/atenea/status.sock".into()
        } else {
            "/run/atenea/status.sock".into()
        }
    })
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

#[tauri::command]
async fn status() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| ask(&socket_path()))
        .await
        .map_err(|e| e.to_string())?
}

/// Estado global y una línea de resumen, con el mismo criterio que el panel y la app de menús.
fn assess(s: &Value) -> (Health, String) {
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
        Health::Ok if peers.is_empty() => "Conectado (sin pares)".to_string(),
        Health::Ok => "Conectado".to_string(),
        Health::Degraded => format!(
            "Degradado: {}",
            why.first()
                .map_or("algún par por relay o sin camino", String::as_str)
        ),
        _ => format!(
            "Sin conexión: {}",
            why.first().map_or("sin detalle", String::as_str)
        ),
    };
    (health, text)
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![status])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let base = Image::from_bytes(TRAY_PNG)?;
            let state = MenuItem::with_id(app, "state", "Comprobando…", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Abrir Atenea One", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&state, &open, &quit])?;
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
            std::thread::spawn(move || {
                loop {
                    let (health, text) = match ask(&socket_path()) {
                        Ok(json) => serde_json::from_str(&json)
                            .map_or((Health::Stopped, "Estado ilegible".to_string()), |v| {
                                assess(&v)
                            }),
                        Err(_) => (
                            Health::Stopped,
                            "Agente parado o sin acceso al estado".to_string(),
                        ),
                    };
                    let _ = tray.set_icon(Some(tray_icon(&base, health)));
                    let _ = tray.set_tooltip(Some(&text));
                    let _ = state.set_text(&text);
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
        assert_eq!(assess(&status(true, "direct")).0, Health::Ok);
        assert_eq!(assess(&status(true, "relay-udp")).0, Health::Degraded);
        assert_eq!(assess(&status(true, "none")).0, Health::Degraded);
        let (h, text) = assess(&status(false, "direct"));
        assert_eq!(
            (h, text.as_str()),
            (Health::Down, "Sin conexión: sin lease activo")
        );
        let mut s = status(true, "direct");
        s["sync"]["last_error"] = json!("acceso denegado");
        assert_eq!(
            assess(&s),
            (
                Health::Degraded,
                "Degradado: control plane: acceso denegado".into()
            )
        );
    }

    #[test]
    fn agent_errors_and_garbage_are_rejected() {
        assert!(checked(r#"{"error":"no autorizado"}"#.into()).is_err());
        assert!(checked(String::new()).is_err());
        assert!(checked(r#"{"peers":[]}"#.into()).is_ok());
    }
}
