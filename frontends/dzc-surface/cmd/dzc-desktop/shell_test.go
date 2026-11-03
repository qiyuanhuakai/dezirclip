package main

import (
	"encoding/json/v2"
	"slices"
	"testing"
)

func TestBackupDialogFilter(t *testing.T) {
	var dialog fileDialog
	if err := json.Unmarshal([]byte(`{"filters":[{"name":"Backup","extensions":["json","dzc"]}]}`), &dialog); err != nil {
		t.Fatal(err)
	}
	filters := dialog.nativeFilters()
	if len(filters) != 1 || filters[0].Name != "Backup" || !slices.Equal(filters[0].Extensions, []string{"json", "dzc"}) {
		t.Fatalf("unexpected native file filters: %+v", filters)
	}
}
