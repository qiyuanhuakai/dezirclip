package main

import (
	"errors"
	"os"
)

func prepareElevation() (bool, error) {
	if len(os.Args) > 1 && (os.Args[1] == "--elevate-helper" || os.Args[1] == "--after-elevation") {
		return true, errors.New("WindowsElevationRequired")
	}
	return false, nil
}

func elevateDesktop(string, string) error { return errors.New("WindowsElevationRequired") }
