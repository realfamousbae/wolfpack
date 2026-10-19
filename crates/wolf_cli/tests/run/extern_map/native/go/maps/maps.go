// Package maps passes maps across the extern boundary.
package maps

// Total adds the values.
func Total(m map[string]int64) int64 {
	var sum int64
	for _, v := range m {
		sum += v
	}
	return sum
}

// Counts counts each word.
func Counts(words []string) map[string]int {
	out := map[string]int{}
	for _, w := range words {
		out[w]++
	}
	return out
}

// Halves doubles the keys and halves the values.
func Halves(m map[int64]float64) map[int64]float64 {
	out := map[int64]float64{}
	for k, v := range m {
		out[k*2] = v / 2
	}
	return out
}

// Split puts even numbers under true and odd ones under false.
func Split(ns []int64) map[bool][]int64 {
	out := map[bool][]int64{true: {}, false: {}}
	for _, n := range ns {
		out[n%2 == 0] = append(out[n%2 == 0], n)
	}
	return out
}

// Letters counts each letter.
func Letters(s string) map[string]int32 {
	out := map[string]int32{}
	for _, c := range s {
		out[string(c)]++
	}
	return out
}

// Nest returns the map as it is.
func Nest(m map[string]map[string]int64) map[string]map[string]int64 {
	return m
}

// Width counts the keys of all maps.
func Width(ms []map[string]int64) int {
	n := 0
	for _, m := range ms {
		n += len(m)
	}
	return n
}
