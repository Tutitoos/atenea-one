// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package backend

import (
	"encoding/json"
	"errors"
	"fmt"
	"runtime"
	"slices"
	"strings"
)

// grantablePermissions son los que un dispositivo puede dar: `write:grants` solo lo concede un administrador de la red.
var grantablePermissions = []string{
	"read:devices", "read:policies", "write:join-codes", "write:approve-devices", "write:policies", "write:revoke-devices",
}

// AdminParams son los argumentos de una operación de administración delegada (ADR-026, fase 6). Cada operación usa
// solo los suyos y los valida con un alfabeto cerrado antes de componer la orden elevada.
type AdminParams struct {
	TTL         int    `json:"ttl"`
	Fingerprint string `json:"fingerprint"`
	Device      string `json:"device"`
	Policy      string `json:"policy"`
	From        string `json:"from"`
	To          string `json:"to"`
	Proto       string `json:"proto"`
	Ports       string `json:"ports"`
	// Conceder permisos (ADR-027): lista cerrada de permisos y alcance; `Device` es el destinatario.
	Permissions []string `json:"permissions"`
	Scope       string   `json:"scope"`
	Devices     []string `json:"devices"`
}

// AdminOp es la lista cerrada de escrituras delegadas. La lectura (concesión, inventario, políticas) no pasa por aquí:
// la hace el agente y la aplicación la lee de su estado local.
type AdminOp int

const (
	AdminJoinCode AdminOp = iota
	AdminApprove
	AdminRevoke
	AdminPolicyAdd
	AdminPolicyRemove
	AdminGrant
	AdminUngrant
)

// ParseAdminOp acepta exactamente las siete operaciones permitidas.
func ParseAdminOp(s string) (AdminOp, error) {
	switch s {
	case "join-code":
		return AdminJoinCode, nil
	case "approve":
		return AdminApprove, nil
	case "revoke":
		return AdminRevoke, nil
	case "policy-add":
		return AdminPolicyAdd, nil
	case "policy-remove":
		return AdminPolicyRemove, nil
	case "grant":
		return AdminGrant, nil
	case "ungrant":
		return AdminUngrant, nil
	}
	return 0, errors.New("operación no permitida")
}

// ValidFingerprint: huella completa, 8 grupos de 4 hexadecimales en minúsculas separados por guiones.
func ValidFingerprint(s string) bool {
	groups := strings.Split(s, "-")
	if len(groups) != 8 {
		return false
	}
	for _, g := range groups {
		if len(g) != 4 {
			return false
		}
		for i := range len(g) {
			c := g[i]
			if !(c >= '0' && c <= '9') && !(c >= 'a' && c <= 'f') {
				return false
			}
		}
	}
	return true
}

func validPort(s string) (int, bool) {
	if s == "" || len(s) > 5 {
		return 0, false
	}
	n := 0
	for i := range len(s) {
		if s[i] < '0' || s[i] > '9' {
			return 0, false
		}
		n = n*10 + int(s[i]-'0')
	}
	return n, n >= 1 && n <= 65535
}

// validPorts: «22» o «8000-8100», de menor a mayor; vacío = todos.
func validPorts(s string) bool {
	if s == "" {
		return true
	}
	lo, hi, isRange := strings.Cut(s, "-")
	min, ok := validPort(lo)
	if !ok {
		return false
	}
	if !isRange {
		return true
	}
	max, ok := validPort(hi)
	return ok && min <= max
}

// AdminScript es la orden de shell elevada de cada operación: una sola llamada al CLI con argumentos fijos y
// validados (identificadores, huella, protocolo, puertos). Nada libre llega a la orden.
func AdminScript(cli string, op AdminOp, network string, p AdminParams) (string, error) {
	if err := validCLI(cli); err != nil {
		return "", err
	}
	if !ValidUUID(network) {
		return "", errors.New("identificador de red no válido")
	}
	id := func(v, what string) (string, error) {
		if ValidUUID(v) {
			return v, nil
		}
		return "", fmt.Errorf("%s no válido", what)
	}
	switch op {
	case AdminJoinCode:
		if p.TTL < 60 || p.TTL > 3600 {
			return "", errors.New("la caducidad debe estar entre 60 y 3600 segundos")
		}
		return fmt.Sprintf("%s network join-code --network %s --ttl %d --json", cli, network, p.TTL), nil
	case AdminApprove:
		if !ValidFingerprint(p.Fingerprint) {
			return "", errors.New("se exige la huella completa")
		}
		return fmt.Sprintf("%s network approve %s --network %s", cli, p.Fingerprint, network), nil
	case AdminRevoke:
		d, err := id(p.Device, "dispositivo")
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network revoke %s --network %s --yes", cli, d, network), nil
	case AdminPolicyAdd:
		from, err := id(p.From, "origen")
		if err != nil {
			return "", err
		}
		to, err := id(p.To, "destino")
		if err != nil {
			return "", err
		}
		switch p.Proto {
		case "any", "icmp", "icmpv6":
			if p.Ports != "" {
				return "", errors.New("solo TCP y UDP admiten puertos")
			}
		case "tcp", "udp":
			if !validPorts(p.Ports) {
				return "", errors.New("puertos no válidos")
			}
		default:
			return "", errors.New("protocolo no válido")
		}
		script := fmt.Sprintf("%s network policy add --network %s --from %s --to %s --proto %s", cli, network, from, to, p.Proto)
		if p.Ports != "" {
			script += " --ports " + p.Ports
		}
		return script, nil
	case AdminPolicyRemove:
		pol, err := id(p.Policy, "identificador de política")
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network policy remove %s --network %s --yes", cli, pol, network), nil
	case AdminGrant:
		d, err := id(p.Device, "dispositivo")
		if err != nil {
			return "", err
		}
		if len(p.Permissions) == 0 {
			return "", errors.New("indica al menos un permiso")
		}
		for i, perm := range p.Permissions {
			if !slices.Contains(grantablePermissions, perm) || slices.Contains(p.Permissions[:i], perm) {
				return "", errors.New("permiso no válido")
			}
		}
		script := fmt.Sprintf("%s network grant %s --network %s --permission %s --scope %s", cli, d, network, strings.Join(p.Permissions, ","), p.Scope)
		switch p.Scope {
		case "enrolled", "network":
			if len(p.Devices) != 0 {
				return "", errors.New("solo el alcance de una lista admite dispositivos")
			}
		case "devices":
			if len(p.Devices) == 0 || len(p.Devices) > 64 {
				return "", errors.New("el alcance de una lista pide de 1 a 64 dispositivos")
			}
			for i, dev := range p.Devices {
				if _, err := id(dev, "dispositivo del alcance"); err != nil || slices.Contains(p.Devices[:i], dev) {
					return "", errors.New("dispositivo del alcance no válido")
				}
			}
			script += " --devices " + strings.Join(p.Devices, ",")
		default:
			return "", errors.New("alcance no válido")
		}
		return script, nil
	case AdminUngrant:
		d, err := id(p.Device, "dispositivo")
		if err != nil {
			return "", err
		}
		return fmt.Sprintf("%s network ungrant %s --network %s --yes", cli, d, network), nil
	}
	return "", errors.New("operación no permitida")
}

func adminPrompt(op AdminOp) string {
	switch op {
	case AdminJoinCode:
		return "Atenea One quiere generar un código de unión para una red."
	case AdminApprove:
		return "Atenea One quiere aprobar un dispositivo en una red."
	case AdminRevoke:
		return "Atenea One quiere revocar un dispositivo de una red."
	case AdminPolicyAdd:
		return "Atenea One quiere crear una política de acceso en una red."
	case AdminGrant:
		return "Atenea One quiere cambiar los permisos de un dispositivo en una red."
	case AdminUngrant:
		return "Atenea One quiere retirar los permisos de un dispositivo en una red."
	}
	return "Atenea One quiere borrar una política de acceso de una red."
}

// ApplyAdmin ejecuta una escritura delegada con el diálogo de autorización del sistema. Cancelarlo no cambia nada.
// En Windows no está soportada todavía: el canal de resultado de UAC no devuelve el código de unión y no se ha
// probado ahí. Devuelve la salida del CLI; para `join-code` solo el código y su caducidad.
func ApplyAdmin(op, network string, p AdminParams) (string, error) {
	o, err := ParseAdminOp(op)
	if err != nil {
		return "", err
	}
	if runtime.GOOS == "windows" {
		return "", errors.New("la administración delegada todavía no está soportada en Windows")
	}
	cli, err := elevatedCLI()
	if err != nil {
		return "", err
	}
	script, err := AdminScript(cli, o, network, p)
	if err != nil {
		return "", err
	}
	out, err := Elevate(script, adminPrompt(o))
	if err != nil {
		return "", err
	}
	if o != AdminJoinCode {
		return strings.TrimSpace(out), nil
	}
	var reply struct {
		Code      string `json:"code"`
		ExpiresAt string `json:"expires_at"`
	}
	if json.Unmarshal([]byte(out), &reply) != nil || reply.Code == "" {
		return "", errors.New("el CLI no devolvió un código de unión")
	}
	b, err := json.Marshal(reply)
	return string(b), err
}
