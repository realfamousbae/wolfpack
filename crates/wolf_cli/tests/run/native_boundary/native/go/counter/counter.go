// Package counting lives in the directory counter: generated code imports it with a name.
package counting

import (
	"fmt"
	"strconv"
)

// Counter counts.
type Counter struct {
	count int64
}

// Start returns a counter at zero.
func Start() *Counter {
	return &Counter{}
}

// Bump returns a counter n higher.
func (c *Counter) Bump(n int64) *Counter {
	return &Counter{count: c.count + n}
}

// Value is the count.
func (c Counter) Value() int64 {
	return c.count
}

// Parse reads a counter from its count.
func Parse(s string) (*Counter, error) {
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil {
		return nil, fmt.Errorf("not a number: %s", s)
	}
	return &Counter{count: n}, nil
}
