// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"
)

// CLIPath es el binario del CLI que instala el paquete. ATENEA_CLI lo sustituye (desarrollo y pruebas).
func CLIPath() string {
	if p := os.Getenv("ATENEA_CLI"); p != "" {
		return p
	}
	if runtime.GOOS == "darwin" {
		return "/Library/Atenea/bin/atenea"
	}
	return "/usr/bin/atenea"
}

// RunDir es el directorio de ejecución de los agentes: ATENEA_RUN_DIR o el del sistema.
func RunDir() string {
	if p := os.Getenv("ATENEA_RUN_DIR"); p != "" {
		return p
	}
	if runtime.GOOS == "darwin" {
		return "/var/run/atenea"
	}
	return "/run/atenea"
}

// ClientInfo es lo único que la interfaz sabe del cliente instalado (ADR-021): el sistema, si la aplicación puede
// VER redes en él (Supported), si además puede unir y gestionarlas (CanJoin) y si el cliente está. En Windows
// «instalado» es que la tubería de estado del agente responde. Sin rutas, versiones ni texto del sistema.
type ClientInfo struct {
	OS        string `json:"os"`
	Supported bool   `json:"supported"`
	CanJoin   bool   `json:"can_join"`
	Installed bool   `json:"installed"`
}

// Client comprueba si el CLI está en su ruta fija (en Windows, si responde la tubería del agente).
func Client() ClientInfo { return clientFor(runtime.GOOS, CLIPath()) }

func clientFor(goos, cli string) ClientInfo {
	unix := goos == "darwin" || goos == "linux"
	info := ClientInfo{OS: goos, Supported: unix || goos == "windows", CanJoin: unix}
	switch {
	case unix:
		st, err := os.Stat(cli)
		info.Installed = err == nil && st.Mode().IsRegular() && st.Mode().Perm()&0o111 != 0
	case goos == "windows":
		info.Installed = windowsAgentResponds()
	}
	return info
}

// NetworkSocket es el socket de estado de una red unida: <run>/<id8>/status.sock, el mismo que usa el CLI.
func NetworkSocket(id string) (string, error) {
	if runtime.GOOS == "windows" {
		if id != WindowsNetworkID {
			return "", errors.New("identificador de red no válido")
		}
		return WindowsStatusPipe, nil
	}
	if !ValidUUID(id) {
		return "", errors.New("identificador de red no válido")
	}
	return filepath.Join(RunDir(), ShortID(id), "status.sock"), nil
}

// List devuelve `atenea network list --json` (sin privilegios). Sin CLI es una lista vacía. En Windows es la red
// única del equipo leída de la tubería de estado (windowsList). Rechaza cualquier salida que no sea un array JSON
// antes de entregarla.
func List() (string, error) {
	if runtime.GOOS == "windows" {
		return windowsList(Ask), nil
	}
	cli := CLIPath()
	if _, err := os.Stat(cli); err != nil {
		return "[]", nil
	}
	listCache.mu.Lock()
	if listCache.cli == cli && time.Since(listCache.at) < listTTL {
		text := listCache.text
		listCache.mu.Unlock()
		return text, nil
	}
	gen := listCache.gen
	listCache.mu.Unlock()

	text, err := Run(cli, "network", "list", "--json")
	if err != nil {
		return "", err // los errores no se cachean
	}
	var v any
	if json.Unmarshal([]byte(text), &v) == nil {
		if _, ok := v.([]any); ok {
			listCache.mu.Lock()
			if listCache.gen == gen { // una invalidación durante la lectura descarta este resultado
				listCache.cli, listCache.text, listCache.at = cli, text, time.Now()
			}
			listCache.mu.Unlock()
			return text, nil
		}
	}
	return "", errors.New("`atenea network list` devolvió una respuesta inesperada")
}

// listTTL es lo que se reutiliza la última lista: la ventana (5 s), la bandeja y DevicePing/OpenPanel la piden a
// la vez y no necesitan un proceso cada uno.
const listTTL = 3 * time.Second

var listCache struct {
	mu   sync.Mutex
	cli  string
	text string
	at   time.Time
	gen  uint64
}

// InvalidateList descarta la lista guardada: se llama al aplicar un cambio de redes para no mostrar estado viejo.
func InvalidateList() {
	listCache.mu.Lock()
	listCache.at = time.Time{}
	listCache.gen++
	listCache.mu.Unlock()
}

// Legacy devuelve `atenea network legacy --json`: {"present": bool}. Sin CLI (o en Windows) no hay instalación
// heredada que importar.
func Legacy() (string, error) {
	cli := CLIPath()
	if runtime.GOOS == "windows" {
		return `{"present":false}`, nil
	}
	if _, err := os.Stat(cli); err != nil {
		return `{"present":false}`, nil
	}
	text, err := Run(cli, "network", "legacy", "--json")
	if err != nil {
		return "", err
	}
	var v map[string]any
	if json.Unmarshal([]byte(text), &v) == nil {
		if _, ok := v["present"].(bool); ok {
			return text, nil
		}
	}
	return "", errors.New("`atenea network legacy` devolvió una respuesta inesperada")
}

// Inspect decodifica un código sin privilegios, sin disco y sin red (`atenea network inspect`).
func Inspect(raw string) (string, error) {
	code, err := NormalizeCode(raw)
	if err != nil {
		return "", err
	}
	text, err := Run(CLIPath(), "network", "inspect", code, "--json")
	if err != nil {
		return "", err
	}
	var v map[string]any
	if json.Unmarshal([]byte(text), &v) == nil && v != nil {
		return text, nil
	}
	return "", errors.New("`atenea network inspect` devolvió una respuesta inesperada")
}

// Elevate ejecuta la orden con el diálogo de autorización del sistema. Es una variable para que las pruebas la
// sustituyan: ninguna prueba abre un diálogo real.
var Elevate = func(script, prompt string) (string, error) {
	if runtime.GOOS == "windows" {
		return "", errors.New("Unirse a redes todavía no está soportado en Windows")
	}
	program, args, err := ElevationCommand(runtime.GOOS, script, prompt)
	if err != nil {
		return "", err
	}
	return RunElevated(program, args...)
}

// safePromptName admite el nombre de la red en el texto del diálogo solo si no puede romper su cita: el texto del diálogo
// no puede contener `"` ni `\` (ElevationCommand los rechaza) y el nombre de un código admite más caracteres (hasta 48
// bytes sin controles ni marcas bidi). Fuera de este alfabeto se usa el texto genérico.
func safePromptName(name string) bool {
	if name == "" || len(name) > 48 || name[0] == ' ' || name[len(name)-1] == ' ' {
		return false
	}
	for i := range len(name) {
		c := name[i]
		if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '.' || c == '_' || c == ' ' || c == '-') {
			return false
		}
	}
	return true
}

// prompt es el texto del diálogo de autorización. Solo `join` nombra la red (la del código que el usuario acaba de
// confirmar); cambia lo que lee el usuario, no la orden que se ejecuta.
func prompt(op Op, network string) string {
	switch op {
	case OpJoin:
		if safePromptName(network) {
			return "Atenea One quiere unir este equipo a la red «" + network + "»."
		}
		return "Atenea One quiere unir este equipo a una red."
	case OpUp:
		return "Atenea One quiere conectar este equipo a una red."
	case OpDown:
		return "Atenea One quiere desconectar este equipo de una red."
	case OpLeave:
		return "Atenea One quiere sacar este equipo de una red y borrar sus claves."
	}
	return "Atenea One quiere importar la instalación heredada como una red."
}

// Apply ejecuta una operación con el diálogo de autorización del sistema. En `join` el id de la red sale del
// propio código (lo decodifica `inspect`), nunca del llamante: el `up` posterior no puede apuntar a otra red.
func Apply(op, network, code, name string) (string, error) {
	o, err := ParseOp(op)
	if err != nil {
		return "", err
	}
	var joinName string // nombre de la red que dice el código, solo para el texto del diálogo (no entra en la orden)
	var gid uint32
	if o == OpJoin {
		if code == "" {
			return "", errors.New("falta el código")
		}
		text, err := Inspect(code)
		if err != nil {
			return "", err
		}
		var info map[string]any
		if err := json.Unmarshal([]byte(text), &info); err != nil {
			return "", err
		}
		id, _ := info["network_id"].(string)
		if !ValidUUID(id) {
			return "", errors.New("el código no trae un id de red válido")
		}
		network = id
		gid = uint32(os.Getgid())
		joinName, _ = info["name"].(string)
	}
	script, err := Script(CLIPath(), o, network, code, name, gid)
	if err != nil {
		return "", err
	}
	defer InvalidateList() // aunque falle o se cancele, el estado pudo cambiar a medias
	return Elevate(script, prompt(o, joinName))
}

// JoinedNetworks son las redes unidas y su estado persistido, actualizado por el CLI. Ante cualquier fallo, vacío.
func JoinedNetworks() []map[string]any {
	text, err := List()
	if err != nil {
		return nil
	}
	var list []any
	if json.Unmarshal([]byte(text), &list) != nil {
		return nil
	}
	var out []map[string]any
	for _, item := range list {
		if n, ok := item.(map[string]any); ok {
			if id, _ := n["network_id"].(string); ValidUUID(id) {
				out = append(out, n)
			}
		}
	}
	return out
}

// JoinedNetwork busca una red unida por su id.
func JoinedNetwork(network string) (map[string]any, error) {
	if !ValidUUID(network) {
		return nil, errors.New("identificador de red no válido")
	}
	text, err := List()
	if err != nil {
		return nil, err
	}
	var list []any
	if err := json.Unmarshal([]byte(text), &list); err != nil {
		return nil, err
	}
	for _, item := range list {
		if n, ok := item.(map[string]any); ok {
			if id, _ := n["network_id"].(string); id == network {
				return n, nil
			}
		}
	}
	return nil, errors.New("la red no está unida")
}
