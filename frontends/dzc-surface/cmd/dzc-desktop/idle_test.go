package main

import "testing"

func TestAcceptedWakeInvalidatesQueuedDestructionBeforeHostRead(t *testing.T) {
	idle := hiddenWebView{generation: 1}
	oldGeneration := idle.generation
	queued := make(chan func(), 1)
	hostRead := make(chan struct{})
	releaseRead := make(chan struct{})
	finished := make(chan struct{})
	go func() {
		idle.acceptWake(func(fn func()) { queued <- fn })
		close(hostRead)
		<-releaseRead
		close(finished)
	}()
	accept := <-queued
	select {
	case <-hostRead:
		t.Fatal("Host read started before UI accepted wake")
	default:
	}
	accept()
	<-hostRead
	// Simulate the old timer callback becoming runnable while the Host stalls.
	if oldGeneration == idle.generation {
		t.Fatal("queued destroy can close the accepted window")
	}
	// A later explicit hide owns a fresh generation, independent of this read.
	idle.cancel()
	newHide := idle.generation
	close(releaseRead)
	<-finished
	if idle.generation != newHide {
		t.Fatal("completed wake invalidated a later hide")
	}
}
