package main

import (
	"errors"

	"golang.org/x/sys/windows"
)

func nativeEventTransportFailure(err error) bool {
	return errors.Is(err, windows.ERROR_BROKEN_PIPE) || errors.Is(err, windows.ERROR_NO_DATA) || errors.Is(err, windows.ERROR_PIPE_NOT_CONNECTED)
}
