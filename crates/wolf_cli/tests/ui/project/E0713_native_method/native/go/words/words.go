// Package words keeps words.
package words

import "strings"

// Index holds words.
type Index struct {
	items []string
}

// Add adds a word.
func (i *Index) Add(word string) {
	i.items = append(i.items, word)
}

// Len is the number of words.
func (i *Index) Len() int {
	return len(i.items)
}

// Has says whether a word is there.
func (i *Index) Has(word string) bool {
	for _, w := range i.items {
		if w == word {
			return true
		}
	}
	return false
}

// Joined joins the words.
func (i *Index) Joined(sep string) string {
	return strings.Join(i.items, sep)
}

// NewIndex makes an index.
func NewIndex() *Index {
	return &Index{}
}
