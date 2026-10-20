package protocol

import (
    "encoding/json"
    "os"
    "reflect"
    "testing"
)

func TestSharedRustGoFixtures(t *testing.T) {
    raw, err := os.ReadFile("../../../protocol/fixtures/messages.json")
    if err != nil { t.Fatal(err) }
    var fixtures []struct { Name string; Schema string; Value json.RawMessage; Valid bool }
    if err = json.Unmarshal(raw, &fixtures); err != nil { t.Fatal(err) }
    for _, fixture := range fixtures {
        t.Run(fixture.Name, func(t *testing.T) {
            if valid := Validate(fixture.Schema, fixture.Value) == nil; valid != fixture.Valid { t.Fatalf("valid=%v expected %v", valid, fixture.Valid) }
            if fixture.Valid && fixture.Schema == "Request" {
                var request Request
                if err := Decode("Request", fixture.Value, &request); err != nil { t.Fatal(err) }
                roundtrip, err := Encode("Request", request)
                if err != nil { t.Fatal(err) }
                var before, after any
                if err = json.Unmarshal(fixture.Value, &before); err != nil { t.Fatal(err) }
                if err = json.Unmarshal(roundtrip, &after); err != nil { t.Fatal(err) }
                if !reflect.DeepEqual(before, after) { t.Fatalf("roundtrip mismatch") }
            }
        })
    }
}

func TestDecoderRejectsOversizeAndTrailingJson(t *testing.T) {
    if Validate("EmptyParams", make([]byte, MaxFrameBytes + 1)) == nil { t.Fatal("oversize accepted") }
    if Validate("EmptyParams", []byte("{} {}")) == nil { t.Fatal("trailing JSON accepted") }
}
