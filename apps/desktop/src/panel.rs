// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! El panel usa la raíz y parámetros de consulta, no rutas por dispositivo.

pub fn url(base: &str, network: &str, device: Option<&str>) -> Result<tauri::Url, String> {
    if !crate::networks::valid_uuid(network)
        || device.is_some_and(|id| !crate::networks::valid_uuid(id))
    {
        return Err("identificador no válido".into());
    }
    let mut url = tauri::Url::parse(base).map_err(|_| "URL del servidor no válida")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("el panel necesita una URL HTTPS sin credenciales".into());
    }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    url.query_pairs_mut()
        .append_pair("net", network)
        .append_pair("view", "Dispositivos");
    Ok(url)
}

pub fn open(url: &tauri::Url) -> Result<(), String> {
    if cfg!(windows) {
        return Err("no soportado en Windows".into());
    }
    let program = if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else {
        "xdg-open"
    };
    let result = std::process::Command::new(program)
        .arg(url.as_str())
        .status()
        .map_err(|e| e.to_string())?;
    if result.success() {
        Ok(())
    } else {
        Err("no se pudo abrir el panel".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "00000000-0000-0000-0000-000000000001";
    #[test]
    fn builds_https_root_and_validates_ids() {
        let built = url("https://panel.example.test/api?old=x#old", ID, Some(ID)).unwrap();
        assert_eq!(
            built.as_str(),
            format!("https://panel.example.test/?net={ID}&view=Dispositivos")
        );
        for base in [
            "http://panel.example.test",
            "javascript:alert(1)",
            "https://user:pass@panel.example.test",
        ] {
            assert!(url_for_test(base).is_err());
        }
        assert!(url("https://panel.example.test", "bad", None).is_err());
        assert!(url("https://panel.example.test", ID, Some("bad")).is_err());
    }
    fn url_for_test(base: &str) -> Result<tauri::Url, String> {
        url(base, ID, None)
    }
}
