<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Cuentas propias y redes compartidas

[ADR-028](../architecture/ADR-028-cuentas-propias-y-redes-por-cuenta.md). Esta guía describe cómo se usan las cuentas con correo y contraseña, cómo se crea una red con una raíz propia y cómo se invita a otras cuentas. El contrato de las rutas está en [control-plane-v1 §26 y §27](../api/control-plane-v1.md).

## Lo que guarda el servidor y lo que no

Los datos de las cuentas viven en el servidor, en la misma base PostgreSQL que el resto y, por tanto, en sus copias: usuarios, el hash argon2id de la contraseña, los hashes de los códigos de recuperación, organizaciones, redes, miembros e invitaciones. **La clave privada raíz de cada red no se guarda nunca**: la genera y la custodia su dueño.

## Operador

- **Abrir el registro.** Está cerrado por defecto. Se abre con `ATENEA_CP_REGISTRATION_OPEN=true` y se cierra quitándolo. Actualizar el control plane no lo abre.
- **Límites** (variables `ATENEA_CP_*`): `REGISTER_PER_MINUTE` (5 por IP, IPv6 por /64), `REGISTER_GLOBAL_PER_MINUTE` (30), `LOGIN_PER_MINUTE` (20, compartido con el inicio OIDC), `PERSONAL_MAX_NETWORKS` (3), `PERSONAL_MAX_DEVICES` (64 no revocados por red) y `MAX_OPEN_INVITATIONS` (10 por red). Los topes de redes y dispositivos solo afectan a las organizaciones personales; las demás no los tienen.
- **GitHub y otros proveedores.** Siguen entrando por OIDC. Añadir GitHub es configuración de Dex en el despliegue (un conector `github`), no del control plane. No está probado en el piloto.
- **Sin correo.** El servidor no envía correos: no hay verificación ni recuperación por correo. Quien pierde la contraseña y los códigos pierde la cuenta; el operador puede restablecerla desde la base, con auditoría.
- **Una cuenta nueva ve la red de otra solo por invitación.** El enlace da acceso a una red, no a la organización de quien invita.

## Crear una red con una raíz propia

El panel guía los pasos (Organización → Crear red):

1. En el equipo del usuario: `atenea root-init --out raiz-offline`. Imprime la clave pública (64 caracteres) y guarda la privada en `raiz-offline`.
2. Esa clave pública va en el campo «Ancla pública raíz». Solo ella viaja al servidor.
3. Tras crear la red, «DNS y certificado» muestra la clave pública del emisor del servidor y el comando con el identificador de la red ya relleno:

   ```sh
   atenea issuer-cert --root raiz-offline --issuer-pub <clave del emisor> \
     --network-id <id sin guiones> --serial 1 --valid-days 30 --out issuer.cert
   ```

   El fichero `issuer.cert` (178 bytes) se sube desde el mismo diálogo. Hasta entonces la red no emite configuraciones.
4. El certificado caduca (de 1 a 90 días). Hay que firmar otro con un `--serial` mayor antes de que caduque; el panel muestra cuánto falta. Los pasos están en [renovación del certificado de emisor](renovacion-certificado-emisor.md).

**Si se pierde la raíz no hay recuperación** (ADR-005): al caducar el certificado la red deja de emitir configuraciones y hay que reincorporar cada dispositivo con una raíz nueva. El panel lo avisa al crear la red.

`atenea issuer-cert` admite `--issuer-key <fichero>` (quien tiene la semilla del emisor) o `--issuer-pub <hex>` (la clave pública que muestra el panel); se pasa uno solo.

## Invitar a otra cuenta

Solo el propietario de la organización de la red. En la sección **Compartir**:

- **Solo ver:** ve la red, sus dispositivos, políticas, rutas, DNS y auditoría. No cambia nada.
- **Ver, generar códigos y aprobar (`enroller`):** además crea códigos de unión y aprueba dispositivos **escribiendo la huella completa** que muestra el propio dispositivo. No crea políticas, no revoca, no cambia DNS ni rutas, no ve el código de red y no invita.

El panel entrega un enlace de un solo uso que caduca a los 7 días y se muestra una vez; el propietario lo envía por el canal que prefiera. Quien lo abre inicia sesión (o crea una cuenta: la invitación se conserva) y confirma con «Aceptar invitación». Las redes aparecen en «Compartidas conmigo».

El invitado puede abandonar la red desde «Compartidas conmigo». El propietario puede anular un enlace sin usar, cambiar el rol de un miembro o quitarle el acceso. **Quitar a un miembro no revoca los dispositivos que aprobó**: la auditoría (`device.approve`, con el autor) dice quién aprobó cada uno y el propietario decide. Aprobar no da acceso por sí solo: las políticas siguen siendo deny por defecto y solo un administrador las crea.

## Retroceso

Las migraciones `0015` y `0016` son aditivas. Con el binario anterior (0.1.34) las cuentas locales no pueden iniciar sesión (no conoce las rutas `/auth/local`), los miembros por invitación no ven nada y las organizaciones personales quedan como organizaciones normales. No se pierde ninguna red ni dispositivo.

## Sin comprobar

- Con un navegador real y un servidor local de pruebas se comprobaron: registro con códigos de recuperación, creación de red con raíz propia, firma del certificado con `--issuer-pub`, invitación a un `enroller`, aceptación tras registrarse, y que el invitado crea códigos y no ve el código de red ni «Compartir».
- No se probó con Dex, con GitHub, con dos navegadores distintos, ni en un servidor de producción.
- La aprobación como `enroller` con la huella está probada contra PostgreSQL, no en el navegador.
- El tope de 64 dispositivos y el de 3 redes tienen prueba automática; el de 10 invitaciones también. La carga de registros abiertos no se midió.
