package geo

import "math"

type Point struct {
	X float64
	Z float64
}

type Tag struct {
	name string
}

func Norm(p Point) float64 {
	return math.Hypot(p.X, p.Z)
}

func Label() *Tag {
	return &Tag{name: "a"}
}
