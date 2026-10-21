package protocol

import (
    "bytes"
    _ "embed"
    "encoding/json"
    "errors"
    "reflect"
    "strings"
    "sync"
    "unicode/utf8"

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
    if len(raw) == 0 || len(raw) > MaxFrameBytes || !utf8.Valid(raw) || !validUnicodeEscapes(raw) { return ErrInvalidMessage }
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
    destination := reflect.TypeOf(result)
    if destination == nil || destination.Kind() != reflect.Pointer || reflect.ValueOf(result).IsNil() { return ErrInvalidMessage }
    value, err := jsonschema.UnmarshalJSON(bytes.NewReader(raw))
    if err != nil { return ErrInvalidMessage }
    exact, err := json.Marshal(exactFields(value, destination.Elem()))
    if err != nil { return ErrInvalidMessage }
    if err = json.Unmarshal(exact, result); err != nil { return ErrInvalidMessage }
    return nil
}

// encoding/json otherwise accepts case-insensitive field aliases. Project only
// the exact tags in the generated DTO, recursively, before typed decoding. This
// mirrors serde's unknown-field behavior without maintaining a second schema.
func exactFields(value any, destination reflect.Type) any {
    for destination.Kind() == reflect.Pointer { destination = destination.Elem() }
    if destination == reflect.TypeOf(json.RawMessage{}) { return value }
    switch destination.Kind() {
    case reflect.Struct:
        object, ok := value.(map[string]any)
        if !ok { return value }
        exact := make(map[string]any)
        for index := 0; index < destination.NumField(); index++ {
            field := destination.Field(index)
            tag := strings.Split(field.Tag.Get("json"), ",")[0]
            if tag == "" || tag == "-" { continue }
            if child, exists := object[tag]; exists { exact[tag] = exactFields(child, field.Type) }
        }
        return exact
    case reflect.Slice, reflect.Array:
        array, ok := value.([]any)
        if !ok { return value }
        exact := make([]any, len(array))
        for index, child := range array { exact[index] = exactFields(child, destination.Elem()) }
        return exact
    default:
        return value
    }
}

func validUnicodeEscapes(raw []byte) bool {
    // Syntax is checked by the JSON parser too; this scan rejects the lone
    // surrogate escapes that encoding/json would silently replace with U+FFFD.
    inside := false
    for index := 0; index < len(raw); index++ {
        if raw[index] == '"' { inside = !inside; continue }
        if !inside || raw[index] != '\\' { continue }
        index++
        if index >= len(raw) { return false }
        if raw[index] != 'u' { continue }
        code, ok := hexCode(raw, index + 1)
        if !ok { return false }
        index += 4
        if code >= 0xdc00 && code <= 0xdfff { return false }
        if code >= 0xd800 && code <= 0xdbff {
            if index + 6 >= len(raw) || raw[index + 1] != '\\' || raw[index + 2] != 'u' { return false }
            low, ok := hexCode(raw, index + 3)
            if !ok || low < 0xdc00 || low > 0xdfff { return false }
            index += 6
        }
    }
    return true
}

func hexCode(raw []byte, start int) (uint16, bool) {
    if start + 4 > len(raw) { return 0, false }
    var code uint16
    for _, value := range raw[start:start + 4] {
        code <<= 4
        switch {
        case value >= '0' && value <= '9': code += uint16(value - '0')
        case value >= 'a' && value <= 'f': code += uint16(value - 'a' + 10)
        case value >= 'A' && value <= 'F': code += uint16(value - 'A' + 10)
        default: return 0, false
        }
    }
    return code, true
}

func Encode(name string, value any) ([]byte, error) {
    raw, err := json.Marshal(value)
    if err != nil { return nil, ErrInvalidMessage }
    if err = Validate(name, raw); err != nil { return nil, err }
    return raw, nil
}
