/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

import (
	"math"
	"sort"
)

type means struct {
	X []float64
	m []float64
}

func (b *means) optimize(iterations int) {
	b.m = append(b.m, min64(b.X))
	b.m = append(b.m, max64(b.X))
	for i := 0; i < iterations; i++ {
		newN := make([]float64, len(b.m))
		newM := make([]float64, len(b.m))
		for _, x := range b.X {
			k := b.class(x)
			newN[k] += 1
			newM[k] += x
		}
		for k, m := range newM {
			b.m[k] = m / newN[k]
		}
	}
	sort.Float64s(b.m)
}

func (b *means) class(x float64) (k int) {
	lo := math.Abs(x - b.m[0])
	hi := math.Abs(x - b.m[1])
	if lo < hi {
		return 0
	} else {
		return 1
	}
}

func (b *means) extra(x float64) (k int) {
	hi := math.Abs(x - b.m[1]*1)
	ex := math.Abs(x - b.m[1]*3)
	if hi < ex {
		return b.class(x)
	} else {
		return 2
	}
}

func (b *means) steps() (result []*step) {
	pre := 0
	for n, x := range b.X {
		k := b.class(x)
		if pre != k {
			result = append(result, &step{
				time: n,
				down: k == 0,
			})
		}
		pre = k
	}
	return
}

type step struct {
	time int
	down bool
	span float64
}

func (s *step) tone(class int) string {
	switch class {
	case 0:
		return "."
	case 1:
		return "_"
	default:
		return "_"
	}
}

func (s *step) mute(class int) string {
	switch class {
	case 0:
		return ""
	case 1:
		return " "
	default:
		return " ; "
	}
}

func min64(x []float64) (min float64) {
	for n, v := range x {
		if n == 0 || v < min {
			min = v
		}
	}
	return
}

func max64(x []float64) (max float64) {
	for n, v := range x {
		if n == 0 || v > max {
			max = v
		}
	}
	return
}

func sum64(x []float64) (sum float64) {
	for _, v := range x {
		sum += v
	}
	return
}

func top64(x []float64) (index []int) {
	top := 0.0
	pos := 0
	for n, v := range x {
		if v > top {
			top = v
			pos = n
		} else if v < top {
			index = append(index, pos)
		}
	}
	return
}
