// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Ensayo de ADR-017 fase 1, SOLO para el runner elevado y desechable de CI: dos adaptadores Wintun
//! simultáneos con GUID propio (el UUID de cada red) y sin tocar nada más. Exige `ATENEA_WINTUN_DLL`.
#[cfg(windows)]
fn main() {
    use atenea_tunnel::Tun;
    use std::process::Command;
    assert_eq!(
        std::env::var("GITHUB_ACTIONS").as_deref(),
        Ok("true"),
        "solo en el runner de CI"
    );
    let a: [u8; 16] = *b"\x6c\xfa\xcd\x61\x00\x00\x40\x00\x80\x00\x00\x00\x00\x00\x00\x0a";
    let b: [u8; 16] = *b"\x45\xfe\x40\xd6\x00\x00\x40\x00\x80\x00\x00\x00\x00\x00\x00\x0b";
    let first = Tun::create_with_guid("atn_aaaaaaa1", a).expect("primer adaptador");
    let second = Tun::create_with_guid("atn_aaaaaaa2", b).expect("segundo adaptador");
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg("Get-NetAdapter -IncludeHidden | Where-Object { $_.Name -like 'atn_aaaaaaa*' } | Sort-Object Name | ForEach-Object { \"$($_.Name) $($_.InterfaceGuid)\" }")
        .output()
        .expect("powershell");
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    println!("{text}");
    assert!(
        text.contains("atn_aaaaaaa1 {6cfacd61-0000-4000-8000-00000000000a}"),
        "{text}"
    );
    assert!(
        text.contains("atn_aaaaaaa2 {45fe40d6-0000-4000-8000-00000000000b}"),
        "{text}"
    );
    drop(first);
    drop(second);
    println!("DOS ADAPTADORES: OK");
}
#[cfg(not(windows))]
fn main() {}
