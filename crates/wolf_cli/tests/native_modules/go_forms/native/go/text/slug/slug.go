// Package slug holds the Go functions of the native module native.text.slug.
package slug

import (
	"errors"
	"strconv"
	"strings"
)

// Counter is opaque to wolfpack: its field is not exported.
type Counter struct {
	n uint32
}

// Slugify makes a slug of a title.
func Slugify(title string) string {
	return strings.ReplaceAll(strings.ToLower(title), " ", "-")
}

// Small adds integers of three Go types.
func Small(n uint8, m int32, k int) uint64 {
	return uint64(n) + uint64(m) + uint64(k)
}

// Halve halves a float32.
func Halve(x float32) float32 {
	return x / 2
}

// ParseURL is named in the Go way.
func ParseURL(s string) string {
	return s + "rl"
}

// SumSmall adds a slice of uint32.
func SumSmall(v []uint32) int64 {
	var s int64
	for _, x := range v {
		s += int64(x)
	}
	return s
}

// SumSlice adds a slice of uint16.
func SumSlice(v []uint16) uint32 {
	var s uint32
	for _, x := range v {
		s += uint32(x)
	}
	return s
}

// Words upper-cases words.
func Words(v []string) []string {
	out := make([]string, len(v))
	for i, s := range v {
		out[i] = strings.ToUpper(s)
	}
	return out
}

// Keys narrows the values of a map.
func Keys(m map[string]int32) map[string]uint8 {
	out := make(map[string]uint8, len(m))
	for k, v := range m {
		out[k] = uint8(v)
	}
	return out
}

// Total adds the values of a map.
func Total(m map[string]uint64) uint64 {
	var s uint64
	for _, v := range m {
		s += v
	}
	return s
}

// Parse parses a small number.
func Parse(s string) (uint16, error) {
	n, err := strconv.ParseUint(s, 10, 16)
	if err != nil {
		return 0, errors.New("not a number: " + s)
	}
	return uint16(n), nil
}

// First is the first string, if any.
func First(v []string) (string, bool) {
	if len(v) == 0 {
		return "", false
	}
	return v[0], true
}

// Ratio is 1/n as a float32, if n is not zero.
func Ratio(n int16) (float32, bool) {
	if n == 0 {
		return 0, false
	}
	return 1 / float32(n), true
}

// Check fails for negative numbers.
func Check(n int) error {
	if n < 0 {
		return errors.New("negative")
	}
	return nil
}

// NewCounter makes a counter.
func NewCounter() *Counter {
	return &Counter{n: 7}
}

// Count reads a counter.
func Count(c *Counter) uint32 {
	return c.n
}

// Each counts the items for which f is true.
func Each(items []string, f func(string) bool) int {
	n := 0
	for _, s := range items {
		if f(s) {
			n++
		}
	}
	return n
}

// Size is a record: its fields are exported.
type Size struct {
	W    uint32
	H    float32
	Tags []string
	Note string
	ID   int8
}

// Frame holds a Size.
type Frame struct {
	Size Size
	Name string
}

// Area is the area of a size.
func Area(s Size) uint64 {
	return uint64(float32(s.W) * s.H)
}

// Grow makes a size wider.
func Grow(s Size, by uint32) Size {
	s.W += by
	s.Note = strconv.Itoa(len(s.Tags)) + " tags"
	return s
}

// Frames lists frames.
func Frames(n int) []Frame {
	out := []Frame{}
	for i := 0; i < n; i++ {
		out = append(out, Frame{Size: Size{W: uint32(i), H: 0.5, Tags: []string{}}, Name: "f" + strconv.Itoa(i)})
	}
	return out
}

// FindFrame finds a frame by name.
func FindFrame(name string) (Frame, bool) {
	for _, f := range Frames(3) {
		if f.Name == name {
			return f, true
		}
	}
	return Frame{}, false
}

// ParseSize parses "WxH".
func ParseSize(s string) (Size, error) {
	w, h, ok := strings.Cut(s, "x")
	if !ok {
		return Size{}, errors.New("no x")
	}
	wn, err := strconv.ParseUint(w, 10, 32)
	if err != nil {
		return Size{}, errors.New("bad width")
	}
	hn, err := strconv.ParseFloat(h, 32)
	if err != nil {
		return Size{}, errors.New("bad height")
	}
	return Size{W: uint32(wn), H: float32(hn), Tags: []string{s}, ID: 1}, nil
}
