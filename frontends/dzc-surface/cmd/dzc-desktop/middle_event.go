package main

func middleButtonReleased(event, detail byte) bool {
	return event&127 == 5 && detail == 2
}
