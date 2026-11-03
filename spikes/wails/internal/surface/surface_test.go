// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

package surface

import (
	"reflect"
	"testing"
)

type permitted struct{}

func (*permitted) Status() string { return "" }
func (*permitted) hidden() string { return "" }

type withExtra struct{ permitted }

func (*withExtra) Danger() string { return "" }

func TestExportedListsOnlyExportedMethodsSorted(t *testing.T) {
	if got := Exported(&permitted{}); !reflect.DeepEqual(got, []string{"Status"}) {
		t.Fatalf("Exported = %v", got)
	}
}

// El guardia detecta un método nuevo, aunque llegue por incrustación.
func TestAddingAnExportedMethodChangesTheSurface(t *testing.T) {
	want := []string{"Status"}
	if reflect.DeepEqual(Exported(&withExtra{}), want) {
		t.Fatal("un método añadido debe cambiar la superficie")
	}
	if got := Exported(&withExtra{}); !reflect.DeepEqual(got, []string{"Danger", "Status"}) {
		t.Fatalf("Exported = %v", got)
	}
}
