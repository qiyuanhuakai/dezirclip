// Package searchtext provides text matching for native controls.
package searchtext

import "slices"

// WithinOneEdit accepts an exact match, one insertion, deletion or replacement,
// or one transposition of adjacent characters.
func WithinOneEdit(a, b string) bool {
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
