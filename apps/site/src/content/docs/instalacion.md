---
title: Instalación
description: Cómo instalar el cliente de Atenea One en Linux, macOS y Windows, verificar la descarga y enrolar un dispositivo.
---

Atenea One es un producto en evaluación, sin auditoría independiente. **Instala primero en una máquina de laboratorio**, no en un servidor con cargas de producción.

## Plataformas

| Sistema | Alcance | Paquete |
| --- | --- | --- |
| Linux (Debian 13, Ubuntu 24.04 o posterior) | Cliente y router de subred | `.deb` (amd64 y arm64) |
| macOS 11 o posterior | Solo cliente | `.pkg` con cliente y aplicación |
| Windows 10/11 x64 | Solo cliente | Instalador `.exe` único |

Los clientes de macOS y Windows consumen rutas aprobadas y DNS dividido. El router que sirve subredes es siempre Linux. Los paquetes de macOS y Windows se publican **sin firma de desarrollador ni notarización**.

## Verificar la descarga

Comprueba el paquete **antes** de instalarlo. `SHA256SUMS` por sí solo no prueba procedencia: quien sustituya el paquete en la publicación sustituye también ese fichero. La comprobación válida es la del manifiesto firmado (Ed25519), con los scripts de `scripts/release/` del repositorio, en un directorio que contenga solo lo descargado:

```sh
bash scripts/release/verify.sh --published DESCARGAS /ruta/confiable/release.pub COMMIT_DEL_TAG VERSION
```

La clave pública, el commit del tag y la versión deben obtenerse por un canal independiente de la publicación: una clave que viaja junto al instalador no es confianza. Requiere Python 3.11 o posterior y OpenSSL 3 con Ed25519.

## Linux

```sh
sudo apt install ./atenea-cli-*-linux-*.deb
```

El paquete instala el agente y la CLI como servicio systemd endurecido. Crea solo el grupo `atenea`; no genera claves ni configuración.

## macOS

Descarga `atenea-one-<versión>-macos-<arq>.pkg` de la [última release](https://github.com/Tutitoos/atenea-one/releases/latest), verifícalo y ejecútalo con doble clic o desde la terminal:

```sh
sudo installer -pkg atenea-one-*-macos-*.pkg -target /
```

Instala el cliente en `/Library/Atenea/bin` y `Atenea One.app` en `/Applications`, con una sola autorización. Sin firma de Apple ni notarización: Gatekeeper avisará.

## Windows

Descarga `atenea-one-<versión>-windows-amd64.exe` de la [última release](https://github.com/Tutitoos/atenea-one/releases/latest) y ejecútalo con doble clic (UAC) o, en silencio, desde una consola elevada:

```powershell
.\atenea-one-<versión>-windows-amd64.exe /S
```

Instala por equipo el cliente, el agente y la aplicación en `%ProgramFiles%\Atenea`. Sin firma de código: SmartScreen puede avisar. No hay `install.ps1` ni ZIP; el instalador es el único camino.

## Enrolar un dispositivo

1. En el panel, un administrador crea la red y un token de un solo uso, y entrega el objeto `join` por un canal de confianza. Comprueba su `root_anchor` fuera de banda.
2. En el dispositivo, genera las claves y enrola (usa un dominio propio en lugar del de ejemplo):

   ```sh
   sudo atenea device-init --dir /var/lib/atenea/device
   sudo atenea enroll --url https://control.example.test --token "$TOKEN" --dir /var/lib/atenea/device
   ```

3. El administrador compara la huella del dispositivo por otro canal y lo aprueba en el panel.
4. Genera la configuración del agente y arranca el servicio:

   ```sh
   sudo atenea agent-config --join /root/join.json --dir /var/lib/atenea/device --out /etc/atenea/agent.toml
   sudo systemctl enable --now atenea-agent.service
   sudo atenea status --socket /run/atenea/control.sock
   ```

El token se recibe por argumento y puede verse en la lista de procesos: hazlo desde una máquina de administración de confianza. Borra el `join` temporal cuando ya no haga falta.

Un servicio `active` no implica tráfico autorizado: sin configuración firmada vigente, el tráfico queda denegado.

## Desinstalar

`apt remove` y `apt purge` **no borran** claves, configuración, estado ni el grupo `atenea`, para evitar una pérdida irreversible. Antes de eliminarlos, revoca el dispositivo en el panel y confirma que tienes copia de lo que necesites.
