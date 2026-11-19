package main

import (
	_ "embed"
	"encoding/json"
)

//go:embed native-main-messages.json
var nativeMainMessagesJSON []byte

type nativeLanguage string

var nativeMainMessages = func() map[nativeLanguage]map[string]string {
	var messages map[nativeLanguage]map[string]string
	if err := json.Unmarshal(nativeMainMessagesJSON, &messages); err != nil {
		panic(err)
	}
	return messages
}()

func mainLanguage(saved string) nativeLanguage {
	switch saved {
	case "en", "tw":
		return nativeLanguage(saved)
	default:
		return "zh"
	}
}

func (language nativeLanguage) text(key string, english ...string) string {
	if language == "en" && len(english) != 0 {
		return english[0]
	}
	if message := nativeMainMessages[language][key]; message != "" {
		return message
	}
	return nativeMainMessages["en"][key]
}

func (n *nativeMain) language() nativeLanguage { return mainLanguage(n.preferences["app.language"]) }

func (n *nativeMain) text(key string, english ...string) string {
	return n.language().text(key, english...)
}
