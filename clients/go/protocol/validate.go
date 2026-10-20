package protocol

import (
    "bytes"
    _ "embed"
    "encoding/json"
    "errors"
    "sync"

    "github.com/santhosh-tekuri/jsonschema/v6"
)

//go:embed schema.json
var schemaBytes []byte

var Version = ProtocolVersion{Major: 1, Minor: 0}
var ErrInvalidMessage = errors.New("invalid local protocol message")
var once sync.Once
var validators map[string]*jsonschema.Schema
var initializationError error

type offlineLoader struct{}
func (offlineLoader) Load(string) (any, error) { return nil, ErrInvalidMessage }

func initialize() {
    document, err := jsonschema.UnmarshalJSON(bytes.NewReader(schemaBytes))
    if err != nil { initializationError = err; return }
    compiler := jsonschema.NewCompiler()
    compiler.UseLoader(offlineLoader{})
    if err = compiler.AddResource("https://dezirclip.local/bundled", document); err != nil { initializationError = err; return }
    validators = make(map[string]*jsonschema.Schema)
    definitions := document.(map[string]any)["$defs"].(map[string]any)
    for name := range definitions {
        validator, err := compiler.Compile("https://dezirclip.local/bundled#/$defs/" + name)
        if err != nil { initializationError = err; return }
        validators[name] = validator
    }
}

func Validate(name string, raw []byte) error {
    if len(raw) == 0 || len(raw) > MaxFrameBytes { return ErrInvalidMessage }
    once.Do(initialize)
    if initializationError != nil { return ErrInvalidMessage }
    validator, found := validators[name]
    if !found { return ErrInvalidMessage }
    value, err := jsonschema.UnmarshalJSON(bytes.NewReader(raw))
    if err != nil || !withinDepth(value, MaxJSONDepth) || validator.Validate(value) != nil { return ErrInvalidMessage }
    return nil
}

func withinDepth(value any, remaining int) bool {
    if remaining == 0 { return false }
    switch typed := value.(type) {
    case []any:
        for _, child := range typed { if !withinDepth(child, remaining - 1) { return false } }
    case map[string]any:
        for _, child := range typed { if !withinDepth(child, remaining - 1) { return false } }
    }
    return true
}

func Decode(name string, raw []byte, result any) error {
    if err := Validate(name, raw); err != nil { return err }
    if err := json.Unmarshal(raw, result); err != nil { return ErrInvalidMessage }
    return nil
}

func Encode(name string, value any) ([]byte, error) {
    raw, err := json.Marshal(value)
    if err != nil { return nil, ErrInvalidMessage }
    if err = Validate(name, raw); err != nil { return nil, err }
    return raw, nil
}
