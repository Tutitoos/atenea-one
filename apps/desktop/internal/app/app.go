// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// Package app reúne la lógica de las doce operaciones que la aplicación expone al frontend y el sondeo de la
// bandeja (ADR-019). No depende de Wails: el servicio vinculado de `main` solo delega aquí, de modo que todo lo
// que decide algo se puede probar sin ventana.
package app

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/Tutitoos/atenea-one/apps/desktop/internal/autostart"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/backend"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/firstrun"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/health"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/notify"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/notifyprefs"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/panel"
	"github.com/Tutitoos/atenea-one/apps/desktop/internal/ping"
)

// App son las operaciones de la aplicación y las dependencias del entorno que las pruebas sustituyen.
type App struct {
	// Ask pregunta `status` al socket de un agente (respuesta ya validada).
	Ask func(socket string) (string, error)
	// ConfigDir es la carpeta de configuración de la aplicación (preferencias).
	ConfigDir func() (string, error)
	HomeDir   func() (string, error)
	// Exe es la ruta del ejecutable propio, para el fichero de arranque con la sesión.
	Exe    func() (string, error)
	Getenv func(string) string
	// Now es el reloj de los avisos.
	Now func() time.Time
	// Deliver muestra un aviso del sistema.
	Deliver func(text string) error
	// Log recibe los errores que no tienen a quién devolverse (avisos del sistema).
	Log func(message string)

	prefMu sync.Mutex
	pref   *bool // preferencia de avisos en memoria (nil = aún sin leer)
}

// New devuelve la aplicación con el entorno real. La carpeta de configuración es la misma que usaba la versión
// de Tauri (`one.atenea.desktop` dentro de la carpeta de configuración del usuario), para conservar las preferencias.
func New() *App {
	return &App{
		Ask: func(socket string) (string, error) {
			return backend.Ask(socket, backend.MaxStatusBytes, 2*time.Second)
		},
		ConfigDir: func() (string, error) {
			dir, err := os.UserConfigDir()
			if err != nil {
				return "", err
			}
			return filepath.Join(dir, "one.atenea.desktop"), nil
		},
		HomeDir: os.UserHomeDir,
		Exe:     os.Executable,
		Getenv:  os.Getenv,
		Now:     time.Now,
		Deliver: notify.Deliver,
		Log:     func(message string) { fmt.Fprintln(os.Stderr, message) },
	}
}

// decode lee el JSON del agente conservando los números tal cual llegan (contadores de bytes sin pasar por
// float64) para reenviarlo al frontend sin alterarlo.
func decode(text string) (any, error) {
	dec := json.NewDecoder(bytes.NewReader([]byte(text)))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, err
	}
	return v, nil
}

// readStatus pregunta al agente de una red unida y devuelve su estado como objeto (vacío si no lo es).
func (a *App) readStatus(network string) (any, map[string]any, error) {
	socket, err := backend.NetworkSocket(network)
	if err != nil {
		return nil, nil, err
	}
	text, err := a.Ask(socket)
	if err != nil {
		return nil, nil, err
	}
	value, err := decode(text)
	if err != nil {
		return nil, nil, err
	}
	obj, _ := value.(map[string]any)
	return value, obj, nil
}

// Status es el estado de una red unida con `atenea network join` más su salud, que se calcula aquí una sola vez
// y comparten la bandeja y la ventana: el frontend no repite el criterio. No hay instancia «predeterminada».
func (a *App) Status(network string) (string, error) {
	value, obj, err := a.readStatus(network)
	if err != nil {
		return "", err
	}
	assessment := health.Assess(obj)
	reasons := assessment.Reasons
	if reasons == nil {
		reasons = []string{}
	}
	out, err := json.Marshal(map[string]any{
		"status": value,
		"health": map[string]any{"level": assessment.Level(), "text": assessment.Text, "reasons": reasons},
	})
	return string(out), err
}

// NetworkList son las redes unidas (`atenea network list --json`, sin privilegios).
func (a *App) NetworkList() (string, error) { return backend.List() }

// NetworkLegacy informa de si queda una instalación heredada pendiente de importar.
func (a *App) NetworkLegacy() (string, error) { return backend.Legacy() }

// ClientStatus es el sistema, si la aplicación puede gestionar redes en él y si el CLI está instalado (ADR-021). Solo
// lectura: no ejecuta el CLI ni abre ningún socket, y no devuelve rutas ni versiones.
func (a *App) ClientStatus() backend.ClientInfo { return backend.Client() }

// NetworkInspect decodifica un código pegado para la confirmación previa; no toca disco ni red.
func (a *App) NetworkInspect(code string) (string, error) { return backend.Inspect(code) }

// NetworkApply ejecuta unirse, conectar, desconectar, salir o importar la instalación heredada. El sistema
// operativo pide la autorización en cada operación.
func (a *App) NetworkApply(op, network, code, name string) (string, error) {
	return backend.Apply(op, network, code, name)
}

// DevicePing prueba la conexión con un par. La dirección debe ser la de un par del estado de ESA red unida: se
// comprueba antes de lanzar nada, y la red debe estar unida antes de preguntar a su agente.
func (a *App) DevicePing(network, address string) (string, error) {
	if _, err := backend.JoinedNetwork(network); err != nil {
		return "", err
	}
	_, obj, err := a.readStatus(network)
	if err != nil {
		return "", err
	}
	ip, err := ping.Validate(address, obj)
	if err != nil {
		return "", err
	}
	return ping.Run(ip)
}

// OpenPanel abre el panel web del servidor de una red unida en el navegador del sistema.
func (a *App) OpenPanel(network, deviceID string) error {
	joined, err := backend.JoinedNetwork(network)
	if err != nil {
		return err
	}
	base, ok := joined["url"].(string)
	if !ok {
		return errors.New("falta la URL del servidor")
	}
	url, err := panel.URL(base, network, deviceID)
	if err != nil {
		return err
	}
	return panel.Open(url)
}

// ClaimFirstRun dice si esta es la primera vez que se abre la aplicación para este usuario y lo anota (ADR-021): true
// solo una vez. Un fallo al leer la carpeta de configuración o escribir el fichero se registra y cuenta como «no es la
// primera», para no abrir la ventana en cada arranque.
func (a *App) ClaimFirstRun() bool {
	dir, err := a.ConfigDir()
	if err != nil {
		a.Log("primera ejecución: " + err.Error())
		return false
	}
	first, err := firstrun.Claim(dir)
	if err != nil {
		a.Log("primera ejecución: " + err.Error())
	}
	return first
}

func (a *App) notificationsPath() (string, error) {
	dir, err := a.ConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "notifications.json"), nil
}

// NotificationsGet lee la preferencia de avisos del sistema (activada por defecto).
func (a *App) NotificationsGet() (bool, error) { return a.notificationsEnabled() }

// NotificationsSet guarda la preferencia de avisos del sistema.
func (a *App) NotificationsSet(on bool) error {
	path, err := a.notificationsPath()
	if err != nil {
		return err
	}
	if err := notifyprefs.Set(path, on); err != nil {
		return err
	}
	a.prefMu.Lock()
	a.pref = &on
	a.prefMu.Unlock()
	return nil
}

// notificationsEnabled lee la preferencia del disco una sola vez y la recuerda: la bandeja la pide en cada lectura.
// Solo NotificationsSet la cambia. Un error de lectura no se recuerda.
func (a *App) notificationsEnabled() (bool, error) {
	a.prefMu.Lock()
	defer a.prefMu.Unlock()
	if a.pref != nil {
		return *a.pref, nil
	}
	path, err := a.notificationsPath()
	if err != nil {
		return false, err
	}
	on, err := notifyprefs.Get(path)
	if err != nil {
		return on, err
	}
	a.pref = &on
	return on, nil
}

// AutostartGet dice si la aplicación se abre al iniciar sesión; `nil` si el sistema no lo admite.
func (a *App) AutostartGet() (*bool, error) {
	home, err := a.HomeDir()
	if err != nil {
		return nil, err
	}
	return autostart.Get(home, a.Getenv("XDG_CONFIG_HOME"))
}

// AutostartSet activa o desactiva la apertura al iniciar sesión (un fichero de arranque del propio usuario).
func (a *App) AutostartSet(on bool) error {
	home, err := a.HomeDir()
	if err != nil {
		return err
	}
	exe, err := a.Exe()
	if err != nil {
		return err
	}
	return autostart.Set(home, a.Getenv("XDG_CONFIG_HOME"), exe, on)
}

// TrayState es lo que recuerda el sondeo de la bandeja entre lecturas.
type TrayState struct {
	nets   []map[string]any
	tick   int
	notify map[string]*notify.State
}

// Tray es lo que pinta la bandeja tras una lectura.
type Tray struct {
	Health health.Health
	Text   string
}

// Poll lee el estado de las redes unidas y devuelve el icono y el texto (descripción) de la bandeja. La lista de
// redes se consulta con el CLI cada 5 lecturas (~10 s), no en cada una. Una red que no responde (parada a propósito o
// sin acceso al estado) no cuenta para la salud: no tiene que poner rojo el icono de las que sí están conectadas, pero
// sí tiene su detector de avisos. Los avisos del sistema se deciden siempre (para que el estado no se desfase) y solo
// se entregan si la preferencia está activada.
func (a *App) Poll(state *TrayState) Tray {
	if state.tick%5 == 0 {
		state.nets = backend.JoinedNetworks()
	}
	state.tick++
	enabled, _ := a.notificationsEnabled()
	now := a.Now().Unix()
	if state.notify == nil {
		state.notify = map[string]*notify.State{}
	}
	live := map[string]bool{}
	var sources []health.Source
	type reading struct {
		obj map[string]any
		err error
	}
	reads := make([]reading, len(state.nets)) // un hueco por red: cada goroutine escribe solo el suyo
	var wg sync.WaitGroup
	for i, net := range state.nets {
		id, _ := net["network_id"].(string)
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, reads[i].obj, reads[i].err = a.readStatus(id)
		}()
	}
	wg.Wait()
	for i, net := range state.nets {
		id, _ := net["network_id"].(string)
		name, ok := net["name"].(string)
		if !ok {
			name = "Red"
		}
		live[id] = true
		obj := reads[i].obj
		if reads[i].err != nil || obj == nil {
			obj = nil
		} else {
			assessment := health.Assess(obj)
			sources = append(sources, health.Source{Health: assessment.Health, Text: assessment.Text})
		}
		if state.notify[id] == nil {
			state.notify[id] = &notify.State{}
		}
		networkState, _ := net["state"].(string)
		if networkState == "" {
			networkState = "desconocida"
		}
		for _, message := range state.notify[id].Sample(networkState, obj, now) {
			if !enabled {
				continue
			}
			if err := a.Deliver(name + ": " + message); err != nil {
				a.Log("aviso del sistema: " + err.Error())
			}
		}
	}
	for id := range state.notify { // una red que ya no está unida no conserva su historial
		if !live[id] {
			delete(state.notify, id)
		}
	}
	h, text := health.Summarize(len(state.nets), sources)
	return Tray{Health: h, Text: text}
}
