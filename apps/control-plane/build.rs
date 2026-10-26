// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
fn main() {
    // SQLx incrusta migraciones: añadir un fichero debe invalidar también compilaciones incrementales.
    println!("cargo:rerun-if-changed=migrations");
}
