# Copias y recuperación — fase 9

## Alcance y objetivos

ADR-009 propone RPO ≤ 24 h y RTO ≤ 2 h, no una garantía de HA. La BD y el emisor son instancias únicas; los leases fallan cerrados al expirar (300 s + 30 s de tolerancia). Recuperar dentro de dos horas no evita el corte de túneles. La recuperación no permite rebajar seriales/versiones que ya hayan visto los agentes: tras PITR se debe elevar el contador de emisión por encima del máximo conocido antes de reabrir el servicio. No cambie sus anclas ni elimine el estado antirrollback para ocultar el problema.

Se usa pgBackRest en lugar de implementar un gestor propio de `pg_basebackup` + WAL: repositorio cifrado AES-256-CBC, completas, incrementales, checksums y recuperación temporal ya integrados. Una completa al arrancar y cada cambio de día UTC; incremental cada cambio de hora UTC; WAL continuo con `archive_timeout=60s`. Retención: siete completas diarias y catorce diferenciales (si se lanzan manualmente); las incrementales dependen de su completa y expiran con ella. WAL desde la completa más antigua retenida. `check` cada minuto y antes de considerar válida una copia; ejecutar también `verify` y revisar su salida y código de retorno. Un fallo del temporizador termina el proceso: la política de reinicio de Compose lo arranca de nuevo y hace una completa. Esto no sustituye una alerta externa.

El volumen `pgbackrest-repo` **sigue en el mismo nodo**: protege frente a pérdida de `pgdata`, no frente a pérdida del nodo. Antes de producción el operador debe mantener copias cifradas fuera del nodo, verificar su transferencia y ensayar recuperarlas. No hay credenciales ni destino remoto inventados en este repositorio.

## Preparación

1. Construir en Docker: `docker build -t atenea/postgres-backup:local -f deploy/backup/Dockerfile .`. Para releases usar `ATENEA_BACKUP_IMAGE` con el digest de la imagen construida, no una etiqueta mutable.
2. En la estación del operador generar un secreto aleatorio para `pgbackrest-cipher` y custodiar una copia offline independiente del volumen y del nodo. Montarlo mediante `ATENEA_SECRETS_DIR`; debe ser legible por el usuario `postgres` del contenedor, no por usuarios ajenos. No escribirlo en Compose, Git, argumentos o capturas. Compose local no garantiza que `uid/mode` cambien los permisos de un bind mount: ajustar propietario/permisos reales.
3. Definir `ATENEA_BACKUP_IMAGE` con la imagen construida y fijada por digest para producción (en el laboratorio se permite la etiqueta local explícita). Aplicar ambos ficheros: `docker compose -f deploy/compose/prod/compose.yaml -f deploy/compose/prod/backup.yaml config --quiet`, luego `up -d`. El overlay conserva `pgdata`, añade `pgbackrest-repo` y socket Unix compartido. PostgreSQL archiva WAL y el servicio `backup` usa el socket local con usuario postgres.
4. `docker compose -f deploy/compose/prod/compose.yaml -f deploy/compose/prod/backup.yaml exec -u postgres postgres backup-entrypoint pgbackrest --stanza=atenea check`. Revisar `info --output=json`, fecha de la última completa/incremental y errores del archivador (`pg_stat_archiver`). Verificación completa: `exec -u postgres postgres backup-entrypoint backup-verify`. **pgBackRest 2.59.3 puede devolver 0 con checksum inválido**: el wrapper examina también `status: error/invalid` y falla; se ensayó este caso. Se ejecuta tras cada completa diaria.
5. Copia de emergencia antes de una migración: mismo prefijo, `exec -u postgres postgres backup-entrypoint pgbackrest --stanza=atenea --type=full backup`; registrar digest del control plane, versión de PostgreSQL y checksums de `_sqlx_migrations` junto a la copia.

## Claves: respaldo offline a varios destinatarios

Preparar un directorio privado con `issuer.key`, `relay-signing.hex`, `oidc-client-secret` y, **solo si existe en el despliegue**, `application.key`. Son el emisor online, la semilla del coordinator que autoriza tokens relay, el secreto OIDC y la clave de aplicación. El control plane actual no requiere una clave de aplicación independiente: el script permite omitirla; no crear un secreto ficticio ni reutilizar otra clave para rellenarlo. Mantener también certificado de emisor, certificados TLS y configuración no secreta que permita identificar las versiones y rutas; el certificado de emisor está en la BD.

Ejecutar los scripts dentro de la imagen (age no se instala en el Mac):

```sh
docker run --rm --entrypoint sh \
  -v "$PWD":/src:ro -v "$DIRECTORIO_PRIVADO":/keys \
  atenea/postgres-backup:local \
  /src/deploy/backup/keys-backup.sh /keys /keys/copias.age \
  "$DESTINATARIO_AGE_1" "$DESTINATARIO_AGE_2"
```

El script tiene una lista cerrada de nombres; no archiva recursivamente ni incorpora la raíz aunque exista al lado. La identidad privada age nunca se guarda en el nodo ni junto a la copia. Dos custodios independientes pueden descifrar: múltiples destinatarios son OR, no un esquema de umbral. Guardar los archivos cifrados offline y comprobar periódicamente ambos destinatarios. La clave del repositorio pgBackRest requiere custodia offline propia; no forma parte de este archivo de claves online.

**Raíz:** nunca reside ni se respalda en ningún nodo. ADR-005 ya decide para laboratorio fichero age/scrypt y dos copias cifradas offline en la estación de administración; la elección del token hardware antes del piloto sigue pendiente del responsable. Este runbook no elige hardware ni traslada esa raíz a la BD o al backup del nodo. Si se pierde o compromete la raíz, no existe recuperación automática: nueva raíz y reincorporación manual de cada dispositivo con nuevo trust anchor.

## Desastre de nodo

1. Aislar el nodo anterior, conservar evidencias si hay sospecha de compromiso; no arrancar dos emisores que compitan. Si fue compromiso, restaurar la misma clave no lo remedia: emitir un certificado nuevo de serial mayor desde la raíz offline.
2. Aprovisionar un nodo limpio con el mismo release/digests, PostgreSQL mayor 17, configuración sintética sustituida por la propia del operador y secretos restaurados por ficheros. Mantener el control plane detenido y la entrada pública cerrada.
3. Recuperar el repositorio cifrado **desde fuera del nodo** y su secreto desde la custodia offline. No inicializar una BD nueva sobre los datos que se pretende restaurar.
4. Seguir pérdida de BD y pérdida de emisor. Restaurar configuración del coordinator/relay y secretos OIDC; coordinator reconstruye presencia por anuncios, no tiene estado persistente que recuperar.
   Conservar los `relay_id` persistidos y su coincidencia entre cada relay y su entrada del coordinator; son configuración pública, no claves regenerables al arrancar. Si se cambia deliberadamente un ID, actualizar ambos lados y emitir concesiones nuevas: los tokens de la audiencia anterior no sirven.
5. Reabrir solo tras verificar BD, claves y un dispositivo conocido. Comprobar DNS, certificados, firewall, TCP/UDP 443 y STUN desde una red externa; no se han ensayado aquí.

## Pérdida de base de datos / DELETE accidental

1. Parar `control-plane` y `backup`, cerrar escrituras y anotar hora UTC del desastre. No borrar el repositorio. Conservar el volumen dañado para análisis; crear un `pgdata` nuevo vacío, propietario postgres. **No ejecutar `down -v`**: también destruiría el repositorio.
2. Parar `postgres`. Ejecutar un contenedor efímero de la misma imagen, usuario postgres, con `pgdata:/var/lib/postgresql/data`, `pgbackrest-repo:/var/lib/pgbackrest` y el secreto en `/run/secrets/pgbackrest_cipher`. Orden: `pgbackrest --stanza=atenea --type=default restore` para último WAL disponible.
3. Para DELETE accidental, elegir una hora UTC anterior al COMMIT destructivo y posterior a la copia elegida: `pgbackrest --stanza=atenea --type=time --target='AAAA-MM-DD HH:MM:SS+00' --target-action=promote restore`. No inventar la hora: usar auditoría/logs independientes. Elegir `--set=ETIQUETA` si la última copia es posterior al objetivo. Revisar timeline y evitar mezclar ramas; el ensayo recrea volúmenes vacíos en cada recuperación.
4. Arrancar PostgreSQL; esperar fin de recuperación/promoción, revisar logs. Comparar `_sqlx_migrations` y checksums con el release (actual: **0001–0005**). `atenea-control-plane --migrate` detecta checksum diferente o migración desconocida: **detenerse**, no borrar el historial ni forzar migraciones. El negativo actual añade de verdad una columna y registra una migración **6** que el binario 0001–0005 desconoce. Restaurar el binario compatible y ensayar upgrade aparte.
5. Verificar conteos de redes, dispositivos, rutas aprobadas y auditoría; comprobar constraints, `pg_amcheck -U postgres -d atenea --install-missing`. `pg_amcheck` requiere rol administrativo y comprueba corrupción física, no sustituye reglas de negocio.
6. Arrancar control plane con rol limitado. Medir RTO hasta `/healthz` **y** una configuración nueva servida a un dispositivo ya enrolado y aceptada por su agente. Medir RPO contra los COMMIT anteriores al desastre, no contra `pg_isready`. Reanudar archivado en la nueva timeline y hacer una completa antes de reabrir escrituras.
7. Sin WAL posterior a copia solo se recupera el final consistente de la última copia (`--set=ETIQUETA --type=immediate --target-action=promote`). No anunciar RPO cero; cuantificar las transacciones posteriores perdidas. Los WAL imprescindibles para hacer consistente una copia online deben conservarse: sin ellos la copia no puede recuperarse.

## Pérdida de emisor

1. Detener el control plane; custodiar copia cifrada e identidad age en estación aislada. Restaurar a un directorio **nuevo** con `keys-restore.sh COPIA.age IDENTITY DESTINO_NUEVO`, dentro de la imagen y con ambos ficheros montados de solo lectura. Rechaza destinos existentes y archivos con rutas/symlinks/nombres inesperados; ficheros 0600, directorio 0700.
2. Reponer `issuer.key` por canal seguro como fichero del usuario del servicio. Si se sospecha compromiso, usar procedimiento de rotación ADR-005, no reutilización. Reponer secretos del coordinator/OIDC/aplicación solo en sus servicios respectivos.
3. Mantener el certificado de emisor original y comprobar fecha, ámbito y serial. Emitir con `atenea issue --issuer-key ... --issuer-cert ... --spec ... --out ...`; el agente real debe aceptar la configuración con su trust anchor existente y estado antirrollback preservado. Después comprobar también emisión por control plane.
4. Desmontar/destruir material temporal y la identidad age del entorno de restauración. No guardar raíz ni identidad age en el volumen de aplicación.

## Simulacro reproducible

`tests/backup/run.sh` usa Docker y Python estándar del anfitrión exclusivamente como orquestador, sin instalar herramientas. Construye `deploy/backup/Dockerfile`; reutiliza binarios compilados del laboratorio en `atenea-lab-target` **sin modificarlo** y la imagen `atenea-lab`. Requiere haber preparado el laboratorio E2E existente. Cada ciclo usa red y volúmenes con prefijo aleatorio propio; la limpieza solo elimina esos recursos. Las claves, contraseñas y destinatarios se generan en runtime; la raíz efímera se elimina inmediatamente después de certificar al emisor y nunca se archiva.

El esquema es el real, aplicado por `atenea-control-plane --migrate` (arnés actual: **0001–0005**; mediciones históricas siguientes: 0001–0003); login Dex real, creación de organización/red, enrolamiento y aprobación por CLI/API. SQL añade ruta aprobada y marcadores de COMMIT para poder medir pérdidas. La prueba destruye el volumen físico, no restaura sobre una BD viva. El RTO incluye contenedores, recuperación PostgreSQL, arranque control plane, `/healthz` y lector de agente con una configuración servida al dispositivo ya enrolado; excluye aprovisionar un VPS o descargar copias remotas. El PITR excluye deliberadamente dos transacciones (DELETE y marcador posterior), no pérdida accidental de WAL. Sin archivo WAL se pierden tres transacciones de prueba posteriores a la incremental (dos marcadores y DELETE); el intervalo se mide entre timestamps SQL de los últimos marcadores confirmado/restaurado, no estima la latencia de COMMIT.

## Mediciones reales (2026-10-25)

Hardware: **Mac17,4, arm64, 24 GiB RAM, 10 CPU lógicas**, Docker Desktop (servidor Docker 29.7.2 arm64). PostgreSQL 17.11, pgBackRest 2.59.3, age 1.2.1; binarios del laboratorio, checkout base `b31de5621b0a3676b1ef137361fc01152c0455e4` con los cambios de fase 9 sin commit. PostgreSQL corrió sin root, con raíz de solo lectura y capacidades eliminadas. Datos pequeños: una red, un dispositivo enrolado/aprobado, una ruta aprobada, auditoría real y tres marcadores. **No es representativo de un VPS ni de una BD de producción.**

| Escenario / métrica | Ciclo 1 | Ciclo 2 | Media | Peor caso |
| --- | ---: | ---: | ---: | ---: |
| Último WAL: RTO hasta healthz + dispositivo sirviendo configuración aceptada | 3.293 s | 2.918 s | 3.106 s | 3.293 s |
| Último WAL: RPO, transacciones de prueba perdidas / intervalo perdido | 0 / 0 s | 0 / 0 s | 0 / 0 s | 0 / 0 s |
| PITR antes del DELETE: RTO | 3.079 s | 3.097 s | 3.088 s | 3.097 s |
| PITR: transacciones posteriores excluidas a propósito | 2 | 2 | 2 | 2 |
| Sin archivo WAL posterior: RPO | 3 tx / 1.455 s | 3 tx / 1.630 s | 3 tx / 1.543 s | 3 tx / 1.630 s |
| Recuperar emisor + `atenea issue` + aceptación real del agente | 0.383 s | 0.351 s | 0.367 s | 0.383 s |

Los dos ciclos locales **cumplen** la propuesta RPO ≤ 24 h / RTO ≤ 2 h para esta pérdida de volumen. Se fuerza `pg_switch_wal` y se espera `check` antes del desastre: RPO cero es de este ensayo con WAL archivado, **no una promesa** ante caída arbitraria con WAL pendiente. Sin WAL se elimina realmente el directorio de segmentos del repositorio; `archive-copy=y` conserva dentro de la copia los WAL imprescindibles para su consistencia. No se confunde una copia online sin ningún WAL con una copia recuperable.

Salida íntegra de la ejecución final de `tests/backup/run.sh` (sin transcribir secretos):

```text
Fecha UTC: 2026-10-25T10:18:31.353835+00:00
Commit: b31de5621b0a3676b1ef137361fc01152c0455e4
Docker: 29.7.2 / arm64
postgres (PostgreSQL) 17.11 (Debian 17.11-1.pgdg13+2)
pgBackRest 2.59.3
1.2.1
{"ciclo": 1, "latest_rto_s": 3.293, "latest_rpo_tx": 0, "latest_rpo_s": 0, "pitr_rto_s": 3.079, "pitr_excluidas_tx": 2, "sin_wal_perdidas_tx": 3, "sin_wal_rpo_s": 1.455, "clave_rto_s": 0.383}
OK: completa + incremental + WAL; pérdida volumen; latest; PITR antes DELETE; healthz + configuración aceptada por agente; pg_amcheck; constraints; pgBackRest verify detecta corrupción; age corrupto; sin clave; esquema distinto; archivo WAL eliminado; emisor restaurado y atenea issue aceptado
{"ciclo": 2, "latest_rto_s": 2.918, "latest_rpo_tx": 0, "latest_rpo_s": 0, "pitr_rto_s": 3.097, "pitr_excluidas_tx": 2, "sin_wal_perdidas_tx": 3, "sin_wal_rpo_s": 1.63, "clave_rto_s": 0.351}
OK: completa + incremental + WAL; pérdida volumen; latest; PITR antes DELETE; healthz + configuración aceptada por agente; pg_amcheck; constraints; pgBackRest verify detecta corrupción; age corrupto; sin clave; esquema distinto; archivo WAL eliminado; emisor restaurado y atenea issue aceptado
latest_rto_s: media=3.106s peor=3.293s
pitr_rto_s: media=3.088s peor=3.097s
clave_rto_s: media=0.367s peor=0.383s
BACKUP: DOS CICLOS OK
```

Validación real del overlay (código de salida 0, sin salida):

```sh
env PANEL_HOST=panel.example.test RELAY_HOST=relay.example.test CADDY_TLS=internal \
  CONTROL_PLANE_IMAGE=atenea/control-plane:local COORDINATOR_IMAGE=atenea/coordinator:local \
  RELAY_IMAGE=atenea/relay:local ATENEA_RUNTIME_DIR=/tmp/atenea-backup-synthetic \
  ATENEA_SECRETS_DIR=/tmp/atenea-backup-synthetic/secrets \
  ATENEA_BACKUP_IMAGE=atenea-backup-lab:latest \
  docker compose -f deploy/compose/prod/compose.yaml -f deploy/compose/prod/backup.yaml config --quiet
```

Esta validación comprueba el modelo Compose, no existencia de los secretos sintéticos ni arranque de todo el stack. El temporizador entrega la planificación diaria/horaria; no se ha esperado un día de reloj ni un ciclo completo de retención de siete días.

## SIN COMPROBAR

Restauración en VPS real, copias/transferencias fuera del nodo, pérdida simultánea de nodo y repositorio, conectividad/red del proveedor, capacidad con datos de producción, todos los destinatarios/custodios reales, token hardware raíz, protección física de medios offline. Docker en Mac arm64 **no representa un VPS**. Un RTO local no incluye provisión, DNS, descarga remota ni aprobación humana. No se ha publicado ni desplegado nada en infraestructura real.
