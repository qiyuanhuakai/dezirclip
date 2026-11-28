//go:build !windows

package runtime

func nativeEventTransportFailure(error) bool { return false }
