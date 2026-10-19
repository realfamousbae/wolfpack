// Package slug makes slugs for URLs: lower case letters and digits, with one dash between words.
package slug

import "strings"

func wordByte(c byte) bool {
	return ('a' <= c && c <= 'z') || ('A' <= c && c <= 'Z') || ('0' <= c && c <= '9')
}

// Slugify returns the slug of a title.
func Slugify(title string) string {
	var b strings.Builder
	dash := false
	for i := 0; i < len(title); i++ {
		c := title[i]
		if !wordByte(c) {
			dash = true
			continue
		}
		if dash && b.Len() > 0 {
			b.WriteByte('-')
		}
		dash = false
		if 'A' <= c && c <= 'Z' {
			c += 'a' - 'A'
		}
		b.WriteByte(c)
	}
	return b.String()
}

// CountWords returns the number of runs of letters and digits in a title.
func CountWords(title string) int {
	n := 0
	in := false
	for i := 0; i < len(title); i++ {
		w := wordByte(title[i])
		if w && !in {
			n++
		}
		in = w
	}
	return n
}
