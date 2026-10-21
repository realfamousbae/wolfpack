package calc

import "strconv"

type Point struct {
	X     float64
	Count int32
}

func Scale(n int64, by float64) float64 {
	return float64(n) * by
}

func Mean(v []int) float64 {
	t := 0
	for _, x := range v {
		t += x
	}
	return float64(t) / float64(len(v))
}

func Bump(p Point) Point {
	return Point{X: p.X, Count: p.Count + 1}
}

func Label(n int64) string {
	return "#" + strconv.FormatInt(n, 10)
}
