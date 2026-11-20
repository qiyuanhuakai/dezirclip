//go:build !windows

package main

func (n *nativeMain) installShortcutInput() {}

func registerExtraShortcut(string, func()) (bool, error) { return false, nil }
func unregisterExtraShortcut(string) bool                { return false }
