# Atenea One

Plataforma VPN P2P self-hosted: red privada creada desde un panel web, con dispositivos que se comunican cifrados directamente o mediante relays propios.

**Estado: MVP-0 (fases 0–3) más control plane (fase 3B) y conexión directa tras NAT (fase 4), todo en laboratorio.** Túnel Linux cifrado entre agentes con configuración firmada, ACL, revocación por lease, control plane con OIDC, coordinator y STUN propios con hole punching. Aún sin relays (fase 5), panel (fase 6) ni release. No usar para proteger datos sensibles: el protocolo no tiene revisión criptográfica externa.

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

### Incorporar un dispositivo al control plane

En la estación de administración (la raíz nunca se copia al servidor), certifica la
clave de emisor existente para la red creada en el control plane y sube el certificado:

```sh
atenea issuer-cert --root ROOT.key --issuer-key ISSUER.key --network-id HEX \
  --serial 2 --valid-days 30 --out red.cert
```

El administrador entrega el objeto `join` por un canal de confianza: comprueba su
`root_anchor` fuera de banda. En el dispositivo:

```sh
atenea device-init --dir ./dispositivo
atenea enroll --url https://vpn.example.test --token TOKEN --dir ./dispositivo --name portatil
atenea agent-config --join join.json --dir ./dispositivo --out agent.toml
sudo atenea up --config agent.toml
```

`--join` acepta un fichero o el objeto JSON literal. El token no se guarda ni se
imprime (el argumento puede aparecer en el historial del shell/listado de procesos).
El administrador compara la huella del bundle antes de aprobar el dispositivo.
`agent-config` no sobrescribe y crea un fichero 0600; usa rutas absolutas bajo
`--dir`, con opciones para interfaz, escucha, estado, configuración firmada e IPC.
La sección opcional `[sync]` descarga al arrancar y cada 60 s (`interval_secs`,
mínimo 5); el agente verifica la firma antes de aplicar y mantiene el lease si la
descarga falla. Sin `[sync]` sigue funcionando la emisión manual a fichero.

## Seguridad

Ver [SECURITY.md](SECURITY.md). Contribuir: [CONTRIBUTING.md](CONTRIBUTING.md).

## Licencia

[Apache-2.0](LICENSE).
