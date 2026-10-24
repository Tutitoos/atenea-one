# Atenea One

Plataforma VPN P2P self-hosted: red privada creada desde un panel web, con dispositivos que se comunican cifrados directamente o mediante relays propios.

**Estado: fase 2 en curso (túnel Linux cifrado de laboratorio) sobre una base documentada (ADRs, contrato de configuración, modelo de amenazas).** No hay releases. No usar para proteger datos sensibles: el protocolo no tiene revisión criptográfica externa.

## Alcance

- Primera entrega (MVP-0): dos clientes Linux con IPv4/IPv6, túnel cifrado UDP, configuración firmada emitida a mano, ACL mínima y revocación por lease.
- Después: control plane y panel, coordinator con STUN propio, conexión directa, relay UDP y fallback TLS/TCP 443; luego subredes/DNS, Windows/macOS y L2.
- Sin WireGuard como protocolo y sin dependencia operativa de ZeroTier, Tailscale, Headscale ni servicios externos de coordinación.
- No se promete equivalencia completa con ZeroTier.

## Estructura

Monorepositorio: Cargo Workspace (Rust) y pnpm Workspace (TypeScript). Cada directorio se crea cuando tiene una responsabilidad implementada; ver `AGENTS.md`.

## Desarrollo

Requisitos: `rustup` (la versión de Rust la fija `rust-toolchain.toml`), Node y pnpm (versiones en `package.json`).

```sh
scripts/setup.sh   # comprueba herramientas e instala dependencias
scripts/check.sh   # formato, clippy, tests Rust y comprobaciones web (lo mismo que CI)
```

## Seguridad

Ver [SECURITY.md](SECURITY.md). Contribuir: [CONTRIBUTING.md](CONTRIBUTING.md).

## Licencia

[Apache-2.0](LICENSE).
