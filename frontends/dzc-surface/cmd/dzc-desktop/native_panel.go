package main

import "errors"

// Restore both the saved preference and live registration after a failed change.
func changeNativePanel(previous, enabled bool, save func(bool) error, refresh func() error) error {
	if err := save(enabled); err != nil {
		return err
	}
	if err := refresh(); err != nil {
		return errors.Join(err, save(previous), refresh())
	}
	return nil
}
