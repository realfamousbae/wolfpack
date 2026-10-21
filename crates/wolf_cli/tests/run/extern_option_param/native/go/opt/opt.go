// Package opt takes Option parameters as pointers.
package opt

// Point is a record.
type Point struct {
	X float64
	Y float64
}

// Counter is opaque.
type Counter struct {
	n int64
}

// NewCounter makes a counter.
func NewCounter() *Counter {
	return &Counter{n: 7}
}

// IntOr is n or d.
func IntOr(n *int64, d int64) int64 {
	if n == nil {
		return d
	}
	return *n
}

// FloatOr is x or -1.
func FloatOr(x *float64) float64 {
	if x == nil {
		return -1
	}
	return *x
}

// TextOr is s or "none".
func TextOr(s *string) string {
	if s == nil {
		return "none"
	}
	return *s
}

// Count is the length of l, or -1.
func Count(l *[]string) int64 {
	if l == nil {
		return -1
	}
	return int64(len(*l))
}

// Total adds the values of m, or is -1.
func Total(m *map[string]int64) int64 {
	if m == nil {
		return -1
	}
	var t int64
	for _, v := range *m {
		t += v
	}
	return t
}

// XOf is the x of p, or -1.
func XOf(p *Point) float64 {
	if p == nil {
		return -1
	}
	return p.X
}

// Ticks reads a counter, or is -1.
func Ticks(c **Counter) int64 {
	if c == nil {
		return -1
	}
	return (*c).n
}
