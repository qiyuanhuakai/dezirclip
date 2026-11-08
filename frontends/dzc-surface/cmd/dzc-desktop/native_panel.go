package main

import "errors"

// Restore both the saved preference and live registration after a failed change.
func changeNativePanel[T bool | string](previous, next T, save func(T) error, refresh func() error) error {
	if err := save(next); err != nil {
		return err
	}
	if err := refresh(); err != nil {
		return errors.Join(err, save(previous), refresh())
	}
	return nil
}
