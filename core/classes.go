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

type classes struct {
	X []float64
	m []float64
}

func (b *classes) optimize(iterations int) {
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

func (b *classes) class(x float64) (k int) {
	lo := math.Abs(x - b.m[0])
	hi := math.Abs(x - b.m[1])
	if lo < hi {
		return 0
	} else {
		return 1
	}
}

func (b *classes) extra(x float64) (k int) {
	hi := math.Abs(x - b.m[1]*1)
	ex := math.Abs(x - b.m[1]*3)
	if hi < ex {
		return b.class(x)
	} else {
		return 2
	}
}
