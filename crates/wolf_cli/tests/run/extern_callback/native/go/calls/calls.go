// Package calls calls wolfpack functions.
package calls

import "errors"

// Each calls f for every item.
func Each(items []string, f func(string)) {
	for _, s := range items {
		f(s)
	}
}

// Twice applies f two times.
func Twice(n int64, f func(int64) int64) int64 {
	return f(f(n))
}

// Mapped maps f over xs.
func Mapped(xs []int64, f func(int64) string) []string {
	out := make([]string, len(xs))
	for i, x := range xs {
		out[i] = f(x)
	}
	return out
}

// Count counts the items for which f holds.
func Count(items []string, f func(string) bool) (int, error) {
	if len(items) == 0 {
		return 0, errors.New("nothing to count")
	}
	n := 0
	for _, s := range items {
		if f(s) {
			n++
		}
	}
	return n, nil
}
