package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"os"
)

var elevatedInstance string

type elevationProfile struct {
	Dezirclip *string `json:"dezirclip"`
	Tiez      *string `json:"tiez"`
}

func encodeElevationProfile() (string, error) {
	profile := elevationProfile{}
	if value, exists := os.LookupEnv("DEZIRCLIP_DB_PATH"); exists {
		profile.Dezirclip = &value
	}
	if value, exists := os.LookupEnv("TIEZ_DB_PATH"); exists {
		profile.Tiez = &value
	}
	raw, err := json.Marshal(profile)
	return base64.RawURLEncoding.EncodeToString(raw), err
}

func applyElevationProfile(encoded string) error {
	raw, err := base64.RawURLEncoding.DecodeString(encoded)
	if err != nil {
		return err
	}
	var profile elevationProfile
	if err := json.Unmarshal(raw, &profile); err != nil {
		return err
	}
	for key, value := range map[string]*string{"DEZIRCLIP_DB_PATH": profile.Dezirclip, "TIEZ_DB_PATH": profile.Tiez} {
		if value == nil {
			err = os.Unsetenv(key)
		} else {
			err = os.Setenv(key, *value)
		}
		if err != nil {
			return err
		}
	}
	return nil
}

func (s *DesktopShell) RestartAsAdmin(ctx context.Context) error {
	// UAC is interactive. Do not time out and report cancellation while Windows
	// still holds an authorized elevation request that may later complete.
	return s.resident.send(ctx, residentRequest{Operation: "restart-admin"}, 0)
}
