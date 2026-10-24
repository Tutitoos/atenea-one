# AGENTS.md

Monorepo de Atenea One (VPN P2P self-hosted). Un historial Git, Cargo Workspace + pnpm Workspace.

## Reglas

- Crea un directorio o crate solo cuando tenga una responsabilidad implementada. Sin proyectos vacíos, sin `common/` ni `packages/config/` sin consumidores concretos.
- Añade miembros a `Cargo.toml` y `pnpm-workspace.yaml` al crearlos.
- Repositorio público: nunca commitear secretos, claves, `.env`, inventarios, IPs o alias de servidores reales, volcados, logs ni capturas reales. Solo ejemplos sintéticos.
- Sin WireGuard como protocolo ni servicios externos de coordinación como dependencia operativa.
- No inventar primitivas criptográficas. Decisiones de protocolo/seguridad: ADR en `docs/architecture/` antes del código.
- Publicar código, publicar artefactos, desplegar e instalar son entregas separadas; nada se despliega sin autorización explícita.

## Comandos

- `scripts/setup.sh`: prepara el entorno.
- `scripts/check.sh`: formato, clippy, tests y comprobaciones web; es el mismo que ejecuta CI.
