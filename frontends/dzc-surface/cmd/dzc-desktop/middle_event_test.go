package main

import "testing"

func TestMiddleButtonWaitsForItsOwnRelease(t *testing.T) {
	for _, event := range []byte{4, 5, 4 | 128, 5 | 128} {
		for detail := byte(1); detail <= 5; detail++ {
			expected := event&127 == 5 && detail == 2
			if middleButtonReleased(event, detail) != expected {
				t.Fatalf("event=%d detail=%d", event, detail)
			}
		}
	}
}
