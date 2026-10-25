# Instalación del cliente Windows

## Alcance y requisitos

Windows x64 o ARM64, **Windows PowerShell 5.1** con sus módulos nativos y una sesión elevada de Administrador. No se necesita instalar PowerShell 7 (`pwsh`). Solo **cliente**: consume rutas privadas y DNS dividido firmados; no anuncia ni sirve subredes, no instala NAT/RRAS, no cambia el forwarding, las rutas por defecto ni el DNS global. El router de subred sigue siendo Linux (ADR-007).

El responsable aceptó **Wintun como única excepción de controlador el 2026-10-25**. No se usa el protocolo WireGuard ni otro componente suyo. Atenea carga la API de Wintun sin modificar la DLL. Los binarios de Atenea y sus scripts se entregan **sin firma**: no confundirlo con el controlador Wintun, cuya distribución oficial sí está firmada. Faltan el certificado de firma de código de Atenea y la validación en máquinas reales del responsable; no se declara una entrega firmada ni toda la matriz de interoperabilidad verificada.

## Construir y registrar sin arrancar

En Windows, desde la raíz del repositorio:

```powershell
cargo build --release -p atenea-agent -p atenea-cli --bins
packaging/windows/build-zip.ps1
Expand-Archive target/windows-package/atenea-client-unsigned.zip -DestinationPath atenea-client
# Abrir PowerShell como Administrador y ejecutar desde la carpeta extraída:
.\atenea-client\install.ps1
```

El instalador no solicita desactivar UAC, Secure Boot ni las comprobaciones de firma de controladores. Si la política local bloquea scripts sin firmar, revisarlos y aplicar la política de ejecución aprobada por el administrador; no se altera la política de toda la máquina desde los scripts.

Se instalan `atenea.exe`, `atenea-agent.exe`, la DLL oficial y su licencia en `%ProgramFiles%\Atenea`; `%ProgramData%\Atenea` contiene configuración, claves y estado. El directorio de datos solo permite SYSTEM y Administradores. Los usuarios pueden leer/ejecutar binarios, nunca sustituirlos. No se añade PATH global ni una regla de firewall general.

`AteneaAgent` se registra como **LocalSystem**, arranque **Manual**, con:

```text
atenea-agent.exe run --service --config C:\ProgramData\Atenea\agent.toml
```

La ruta real se deriva de las variables del sistema: no se presupone la unidad C. No se arranca durante la instalación, incluso si existe una configuración anterior. No se sobrescribe una instalación registrada; para actualizar, parar y desinstalar primero, conservando los datos.

El servicio escribe desde el arranque en `%ProgramData%\Atenea\agent.log`, en modo anexado y sin secuencias ANSI. El fichero tiene propietario Administradores y DACL protegida que solo admite SYSTEM/Administradores; se puede leer mientras el servicio está activo. Si no puede abrirlo de forma privada, el arranque falla: no se cambia a una salida insegura. Conservar este registro antes de una retirada con `-Purge`, que también lo elimina.

### Procedencia y hash de Wintun

El ZIP de Atenea **no contiene una DLL de Wintun**. `get-wintun.ps1` descarga exclusivamente [Wintun 0.14.1 oficial](https://www.wintun.net/builds/wintun-0.14.1.zip), comprueba SHA256 **antes de extraer** y comprueba Authenticode en la DLL de la arquitectura nativa:

```text
07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51
```

Hash publicado en [wintun.net](https://www.wintun.net/) y obtenido independientemente descargando el ZIP oficial (2026-10-25). No hay URL alternativa ni aceptación de un hash distinto. El instalador conserva `WINTUN-LICENSE.txt` junto a la DLL. La licencia de binarios precompilados no es GPL: permite su uso mediante la API y redistribución junto al software que la usa, prohíbe modificar la DLL y retirar avisos. Véase el inventario de dependencias. La descarga requiere HTTPS saliente y falla si no puede validarse; no se versiona ni se incorpora un controlador de reemplazo.

## Enrolar, configurar y arrancar

El administrador de la red entrega un `join.json` auténtico (URL, ID de red y ancla pública) y un token de enrolamiento. No generar ni copiar la clave raíz o del emisor al cliente. En una sesión elevada, con `$Join` apuntando al fichero recibido, `$Url` a su URL y `$Token` al token efímero recibido:

```powershell
$Cli = Join-Path $env:ProgramFiles 'Atenea/atenea.exe'
$Data = Join-Path $env:ProgramData 'Atenea'
& $Cli device-init --dir "$Data/device"
& $Cli enroll --url $Url --token $Token --dir "$Data/device" --name $env:COMPUTERNAME
& $Cli agent-config --join $Join --dir "$Data/device" --out "$Data/agent.toml" `
  --interface atenea0 --state-dir "$Data/state" --config-file "$Data/config.bin" `
  --ipc-socket '\\.\pipe\atenea-control' --status-socket '\\.\pipe\atenea-status' --status-gid 545
Remove-Variable Token
Start-Service AteneaAgent
& $Cli status --socket '\\.\pipe\atenea-status'
```

Comprobar el código de salida de cada orden antes de continuar. El token se pasa a una herramienta de línea de órdenes: no pegarlo en registros, transcripciones compartidas ni repositorios. La primera configuración firmada se obtiene por sincronización; la autorización y el antirrollback son los mismos que en Linux. No usar `--advertise-route` en Windows. No exponer secretos mediante permisos heredados: el backend crea los ficheros privados con DACL protegida y rechaza lecturas con permisos ajenos. `credential:` de systemd no es un mecanismo de Windows.

`status_gid = 545` autoriza a **BUILTIN\Users** a consultar el estado; no es un GID Unix ni el SID de un grupo local personalizado. `544` limita a BUILTIN\Administrators; sin el campo solo SYSTEM/Administradores. La canalización de control acepta únicamente SYSTEM/Administradores; la de estado rechaza `stop`, `reload` y `cleanup` aunque el cliente pueda conectarse. Se rechazan clientes remotos. Las rutas de ambas canalizaciones deben ser distintas.

Con `545`, la lectura positiva de un usuario estándar es obligatoria: no se acepta cambiar la sonda para exigir rechazo de ambos canales. El estado concede y la CLI solicita `FILE_GENERIC_READ | FILE_WRITE_DATA` (`0x12008b`): permite leer datos y metadatos, sincronizar y enviar la palabra `status`, no modificar el agente ni crear instancias de la canalización. Se excluyen expresamente `GENERIC_WRITE`, `FILE_CREATE_PIPE_INSTANCE`, `WRITE_DAC` y `WRITE_OWNER`; el control conserva la DACL SYSTEM/Administradores y su comprobación por impersonación. El diagnóstico imprime el descriptor real de ambos pipes, incluidos owner y máscaras ACE.

El pipe de estado con `545` lleva una etiqueta obligatoria Medium/No-Write-Up para admitir al lector estándar de integridad media; no permite escritura de procesos Low/Untrusted. El canal de control y el estado privado no reciben esta etiqueta rebajada.

Wintun se busca junto al ejecutable; para una ejecución administrativa controlada también se admite `ATENEA_WINTUN_DLL` con una ruta absoluta protegida. No cargar DLLs de un directorio descargable o modificable por usuarios sin privilegios.

## Operación, limpieza y retirada

```powershell
$Cli = Join-Path $env:ProgramFiles 'Atenea/atenea.exe'
& $Cli reload --socket '\\.\pipe\atenea-control'
Stop-Service AteneaAgent
# Tras una terminación forzada, con el servicio detenido:
& "$env:ProgramFiles/Atenea/atenea-agent.exe" cleanup --config "$env:ProgramData/Atenea/agent.toml"
# Desinstalar desde la carpeta extraída o la copia instalada:
& "$env:ProgramFiles/Atenea/uninstall.ps1"
# Retirada completa e irreversible (en vez de la orden anterior):
& "$env:ProgramFiles/Atenea/uninstall.ps1" -Purge
```

Las direcciones, MTU y rutas se aplican exclusivamente al adaptador Wintun mediante los cmdlets nativos NetTCPIP; el DNS dividido usa `Add-DnsClientNrptRule`, no claves de registro escritas a mano. Nunca se captura el dominio raíz. La parada SCM espera la limpieza normal; `cleanup` recupera rutas y NRPT propios y retira el dispositivo Wintun huérfano identificado de forma estricta, sin retirar reglas ajenas. Un fallo de limpieza aborta la retirada para permitir su reparación. Por defecto la desinstalación conserva `%ProgramData%\Atenea` para no destruir la identidad ni el estado antirrollback accidentalmente; `-Purge` elimina también todos esos datos de forma **irreversible**. No borra otros adaptadores ni fuerza la retirada global del controlador compartido Wintun.

La parada SCM publica `StopPending` con checkpoint inicial 1 y una indicación de espera de 30 segundos; no es una terminación forzada ni progreso ficticio. Solo publica `Stopped` después de la limpieza y del cierre del runtime; un fallo devuelve código de servicio 1. El desinstalador solicita la parada y espera como máximo 30 segundos, limita `cleanup` a 30 segundos y `sc.exe` a 10 segundos. Si falla la parada, muestra la excepción y el estado/códigos de SCM y **no fuerza la muerte del servicio ni borra la instalación**. Revisar `agent.log` y los eventos de **Service Control Manager** en el registro System antes de reparar; «Cannot stop» por sí solo no identifica la causa. La descarga oficial de Wintun tiene un plazo de 60 segundos.

## Aceptación de CI y pendientes

El trabajo `windows` compila cliente y `atenea-testpeer`, ejecuta pruebas y clippy, construye el ZIP y ejecuta `tests/windows/run.ps1` en el runner elevado desechable de GitHub. Instala realmente la DLL descargada/verificada y el servicio; comprueba arranque manual hasta configurar, configuraciones firmadas v1 y v2, direcciones IPv4/IPv6, MTU 1280, rutas del adaptador, NRPT por dominio, ausencia de cambios de rutas por defecto/DNS global, DACL de las canalizaciones reales, rechazo de control por la canalización de estado, parada SCM, caída forzada, limpieza offline, reinicio y desinstalación. Los ecos ICMP IPv4/IPv6 atraviesan Wintun y Noise hacia el par Rust de usuario; no son respuestas simuladas de un backend de red.

La comprobación NRPT proyecta únicamente `Name`, `Namespace`, `NameServers` y `Comment`, y normaliza sus colecciones antes de comparar el servidor y el dominio exactos; no serializa el árbol interno CIM. Tras cada parada normal y tras `cleanup` de una terminación forzada exige ausencia de reglas NRPT, de las dos rutas de subred de prueba y del adaptador, también oculto, además de conservar rutas por defecto y DNS global.

La aceptación intenta crear un usuario estándar temporal y prueba su acceso real: control denegado, estado legible y órdenes mutantes rechazadas. Solo si el runner impide crear o iniciar esa cuenta se registra el motivo y se conserva la comprobación de las DACL de los objetos reales; un fallo de una prueba ya iniciada no se oculta.

Las sondas conservan el handle nativo del proceso mediante `System.Diagnostics.Process.Start`: Windows PowerShell puede devolver `ExitCode = $null` con `Start-Process -PassThru` y redirección ([incidencia PowerShell #5421](https://github.com/PowerShell/PowerShell/issues/5421)). Se exige un código real y, en caso de fallo, se incluyen stdout/stderr y código de la CLI; no se convierte un código nulo en un falso «acceso denegado». La ausencia temporal del pipe durante el arranque y las instancias ocupadas se sondean dentro del plazo de readiness sin tracebacks; un acceso denegado real sigue siendo un error.

El runner lanza los scripts hijos mediante el ejecutable nativo de Windows PowerShell 5.1, con `PSModulePath` limitado a sus módulos de sistema y sin `PSHOME` heredado de PowerShell 7: evita que `Get-Acl` intente cargar `Microsoft.PowerShell.Security` de otra edición. Cada orden del smoke tiene plazo (30 segundos por defecto) y cada petición de canalización se aísla en un proceso con límite de 5 segundos. Un watchdog exterior de 360 segundos cubre también bloqueos en llamadas nativas/ctypes; instalación y desinstalación tienen límites exteriores de 90 y 60 segundos. El presupuesto del runner, incluidos diagnósticos y retirada, permanece por debajo del paso de CI de 12 minutos.

Los bloques `finally` conservan el error original y muestran siempre la ruta de `agent.log`, el registro del par, estado/códigos SCM, adaptadores, direcciones, rutas, NRPT y eventos de servicio. Cada consulta nativa de diagnóstico tiene límite de 2 segundos y registra su timeout sin saltarse las restantes; cada lote tiene un límite exterior de 25 segundos. Los errores de limpieza se informan y convierten una aceptación aparentemente correcta en fallo. Antes de retirar el directorio temporal se termina y espera al par, se cierra su fichero de registro y se intenta parar el agente; solo en esta VM desechable se fuerza y espera su terminación si SCM falla. Esa recuperación de pruebas no es la política del desinstalador de producción.

Las pruebas portables de las ayudas de entorno, errores y límites se pueden ejecutar sin Windows: `python3 -m unittest discover -s tests/windows -p 'test_*.py'`. No sustituyen la aceptación elevada.

No ejecutar esa prueba en la máquina del responsable: requiere `GITHUB_ACTIONS=true` y elevación. La definición de CI no es un resultado observado: hasta ejecutarla en un push no se afirma que haya pasado. Pendientes: certificado de firma de código, instalación firmada y matriz Linux↔Windows↔macOS en máquinas reales, P2P/relay, suspensión/reanudación y cambios de Wi-Fi. El runner Windows no sustituye pruebas ARM64 ni dispositivos físicos.
