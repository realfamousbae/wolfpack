// Package geo has records that cross the extern boundary.
package geo

import (
	"fmt"
	"math"
	"strings"
)

// Point is a point.
type Point struct {
	X float64
	Y float64
}

// Label is a label at a point.
type Label struct {
	Text    string
	MaxSize int64
	Tags    []string
	At      Point
}

// Distance is the distance of two points.
func Distance(a, b Point) float64 {
	return math.Hypot(b.X-a.X, b.Y-a.Y)
}

// Moved moves a point to the right.
func Moved(p Point, dx float64) Point {
	return Point{X: p.X + dx, Y: p.Y}
}

// Describe shows a label.
func Describe(l Label) string {
	return fmt.Sprintf("%s (%d) %s at %g,%g", l.Text, l.MaxSize, strings.Join(l.Tags, ","), l.At.X, l.At.Y)
}

var grownCalls int64

// Grown makes a label bigger.
func Grown(l Label) Label {
	grownCalls++
	return Label{Text: strings.ToUpper(l.Text), MaxSize: l.MaxSize * 2, Tags: append(append([]string{}, l.Tags...), "c"), At: l.At}
}

// Corners lists n points.
func Corners(n int64) []Point {
	out := []Point{}
	for i := int64(0); i < n; i++ {
		out = append(out, Point{X: float64(i), Y: float64(10 - i)})
	}
	return out
}

// Find finds a point by name.
func Find(name string) *Point {
	if name == "origin" {
		return &Point{}
	}
	return nil
}

// Shifted moves every point of a map to the right.
func Shifted(m map[string]Point) map[string]Point {
	out := map[string]Point{}
	for k, p := range m {
		out[k] = Point{X: p.X + 1, Y: p.Y}
	}
	return out
}

// Calls is how often Grown was called.
func Calls() int64 {
	return grownCalls
}
