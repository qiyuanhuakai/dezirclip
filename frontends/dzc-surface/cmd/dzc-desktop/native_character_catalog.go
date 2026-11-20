package main

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"regexp"
	"slices"
	"sort"
	"strings"
	"sync"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"golang.org/x/text/unicode/norm"
)

//go:embed emoji-data.json
var emojiCatalogJSON []byte

type characterCatalog struct {
	Emoji   []characterGroup
	Symbols []characterGroup
}

type characterGroup struct {
	ID      string
	Entries []catalogCharacter
	choices []uikit.CharacterChoice
}

type catalogCharacter struct {
	uikit.CharacterChoice
	Keywords     []string
	Aliases      []string
	glyph        string
	names        []string
	aliases      []string
	search       string
	englishWords []string
	chineseNames []string
}

var (
	presentationSelectors = strings.NewReplacer("\ufe0e", "", "\ufe0f", "")
	nameSeparators        = strings.NewReplacer("-", " ", "_", " ", "–", " ", "—", " ")
	englishWords          = regexp.MustCompile("[a-z]+")
	englishTypo           = regexp.MustCompile("^[a-z]{4,}$")
	chineseTypo           = regexp.MustCompile("^(?:\\p{Han}){4,}$")
	chineseName           = regexp.MustCompile("^(?:\\p{Han}){3,}$")
	shortcode             = regexp.MustCompile(`^:([^:]+):$`)
)

func normalizeCharacterSearch(text string) string {
	return presentationSelectors.Replace(strings.ToLower(norm.NFKC.String(text)))
}

func characterNameText(text string) string {
	return strings.Join(strings.Fields(nameSeparators.Replace(text)), " ")
}

var emojiCatalog = sync.OnceValue(func() characterCatalog {
	var catalog characterCatalog
	if err := json.Unmarshal(emojiCatalogJSON, &catalog); err != nil {
		panic(err)
	}
	for _, groups := range [][]characterGroup{catalog.Emoji, catalog.Symbols} {
		for groupIndex := range groups {
			group := &groups[groupIndex]
			group.choices = make([]uikit.CharacterChoice, len(group.Entries))
			for i := range group.Entries {
				entry := &group.Entries[i]
				entry.glyph = normalizeCharacterSearch(entry.Value)
				entry.names = make([]string, 0, 1+len(entry.Keywords))
				for _, label := range append([]string{entry.Name}, entry.Keywords...) {
					entry.names = append(entry.names, characterNameText(normalizeCharacterSearch(label)))
				}
				entry.aliases = make([]string, 0, len(entry.Aliases))
				for _, label := range entry.Aliases {
					entry.aliases = append(entry.aliases, characterNameText(normalizeCharacterSearch(label)))
				}
				names := strings.Join(entry.names, " ") + " " + strings.Join(entry.aliases, " ")
				entry.englishWords = englishWords.FindAllString(names, -1)
				for _, name := range slices.Concat(entry.names, entry.aliases) {
					if chineseName.MatchString(name) {
						entry.chineseNames = append(entry.chineseNames, name)
					}
				}
				fields := []string{entry.glyph, names}
				for _, value := range entry.Value {
					fields = append(fields, fmt.Sprintf("u+%04x", value))
				}
				entry.search = strings.Join(fields, " ")
				group.choices[i] = entry.CharacterChoice
			}
		}
	}
	return catalog
})

// A typo may insert, delete, replace or transpose one character.
func oneEditApart(a, b string) bool {
	left, right := []rune(a), []rune(b)
	if len(left) > len(right) {
		left, right = right, left
	}
	if len(right)-len(left) > 1 {
		return false
	}
	i := 0
	for i < len(left) && left[i] == right[i] {
		i++
	}
	if len(left) != len(right) {
		return slices.Equal(left[i:], right[i+1:])
	}
	if i == len(left) || slices.Equal(left[i+1:], right[i+1:]) {
		return true
	}
	return i+1 < len(left) && left[i] == right[i+1] && left[i+1] == right[i] &&
		slices.Equal(left[i+2:], right[i+2:])
}

type characterVocabulary uint8

const (
	englishVocabulary characterVocabulary = iota + 1
	chineseVocabulary
)

func searchCharacters(tab, query string) []characterGroup {
	catalog := emojiCatalog()
	groups := catalog.Emoji
	if tab == "symbols" {
		groups = catalog.Symbols
	}
	literal := strings.TrimSpace(normalizeCharacterSearch(query))
	if literal == "" {
		return groups
	}
	phrase := characterNameText(shortcode.ReplaceAllString(literal, "$1"))
	if phrase == "" {
		phrase = literal
	}
	terms := strings.Fields(phrase)
	typos := make(map[string]characterVocabulary, len(terms))
	for _, term := range terms {
		switch {
		case englishTypo.MatchString(term):
			typos[term] = englishVocabulary
		case chineseTypo.MatchString(term):
			typos[term] = chineseVocabulary
		}
	}
	result := matchCharacters(groups, literal, phrase, terms, nil)
	if len(result) == 0 && len(typos) > 0 {
		return matchCharacters(groups, literal, phrase, terms, typos)
	}
	return result
}

func matchCharacters(groups []characterGroup, literal, phrase string, terms []string, typos map[string]characterVocabulary) []characterGroup {
	type match struct {
		entry catalogCharacter
		rank  int
	}
	type groupMatch struct {
		group characterGroup
		rank  int
	}
	var ranked []groupMatch
	for _, group := range groups {
		var matches []match
		for _, entry := range group.Entries {
			rank := characterMatchRank(entry, literal, phrase, terms, typos)
			if rank >= 0 {
				matches = append(matches, match{entry, rank})
			}
		}
		if len(matches) == 0 {
			continue
		}
		sort.SliceStable(matches, func(i, j int) bool { return matches[i].rank < matches[j].rank })
		filtered := characterGroup{ID: group.ID}
		for _, match := range matches {
			filtered.Entries = append(filtered.Entries, match.entry)
			filtered.choices = append(filtered.choices, match.entry.CharacterChoice)
		}
		ranked = append(ranked, groupMatch{filtered, matches[0].rank})
	}
	sort.SliceStable(ranked, func(i, j int) bool { return ranked[i].rank < ranked[j].rank })
	var result []characterGroup
	for _, match := range ranked {
		result = append(result, match.group)
	}
	return result
}

func characterMatchRank(entry catalogCharacter, literal, phrase string, terms []string, typos map[string]characterVocabulary) int {
	if literal == entry.glyph {
		return 0
	}
	if slices.Contains(entry.aliases, phrase) {
		return 1
	}
	if slices.Contains(entry.names, phrase) {
		return 2
	}
	missing := ""
	for _, term := range terms {
		if strings.Contains(entry.search, term) {
			continue
		}
		if typos[term] == 0 || missing != "" {
			return -1
		}
		missing = term
	}
	if missing == "" {
		return 3
	}
	words := entry.englishWords
	if typos[missing] == chineseVocabulary {
		words = entry.chineseNames
	}
	for _, word := range words {
		if oneEditApart(missing, word) {
			return 4
		}
	}
	return -1
}
