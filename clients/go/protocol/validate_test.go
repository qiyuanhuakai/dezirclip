package protocol

import (
    "encoding/json"
    "encoding/hex"
    "os"
    "reflect"
    "testing"
)

func TestSharedRustGoFixtures(t *testing.T) {
    raw, err := os.ReadFile("../../../protocol/fixtures/messages.json")
    if err != nil { t.Fatal(err) }
    var fixtures []struct { Name string; Schema string; Value json.RawMessage; Valid bool; Raw string; RawHex string `json:"raw_hex"`; Expected json.RawMessage }
    if err = json.Unmarshal(raw, &fixtures); err != nil { t.Fatal(err) }
    for _, fixture := range fixtures {
        t.Run(fixture.Name, func(t *testing.T) {
            if fixture.Raw != "" || fixture.RawHex != "" {
                raw := []byte(fixture.Raw)
                if fixture.RawHex != "" { var err error; raw, err = hex.DecodeString(fixture.RawHex); if err != nil { t.Fatal(err) } }
                if valid := Validate(fixture.Schema, raw) == nil; valid != fixture.Valid { t.Fatalf("raw valid=%v expected=%v",valid,fixture.Valid) }
                return
            }
            if valid := Validate(fixture.Schema, fixture.Value) == nil; valid != fixture.Valid { t.Fatalf("valid=%v expected %v", valid, fixture.Valid) }
            if fixture.Expected != nil {
                var target any
                switch fixture.Schema {
                case "PrivacySetParams": target = &PrivacySetParams{}
                case "HelloParams": target = &HelloParams{}
                case "WireError": target = &WireError{}
                case "HistoryResult": target = &HistoryResult{}
                default: t.Fatal("unsupported typed fixture")
                }
                if err := Decode(fixture.Schema, fixture.Value, target); err != nil { t.Fatal(err) }
                raw, err := json.Marshal(target)
                if err != nil { t.Fatal(err) }
                var actual, expected any
                if err = json.Unmarshal(raw, &actual); err != nil { t.Fatal(err) }
                if err = json.Unmarshal(fixture.Expected, &expected); err != nil { t.Fatal(err) }
                if !reflect.DeepEqual(actual, expected) { t.Fatalf("typed decode differs: %s expected %s",raw,fixture.Expected) }
            }
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
