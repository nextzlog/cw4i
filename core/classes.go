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

type Classes struct {
	X []float64
	M []float64
}

func (m *Classes) Train(K, epochs int) {
	min := min64(m.X)
	ptp := max64(m.X) - min
	for k := 0; k < K; k++ {
		t := float64(k) / float64(K)
		m.M = append(m.M, min+t*ptp)
	}
	for i := 0; i < epochs; i++ {
		g := make([][]float64, K)
		for _, x := range m.X {
			k := m.Class(x)
			g[k] = append(g[k], x)
		}
		for k, x := range g {
			if len(x) > 0 {
				m.M[k] = med64(x)
			}
		}
	}
	sort.Float64s(m.M)
}

func (m *Classes) Class(x float64) int {
	score := math.Inf(0)
	class := 0
	for k, m := range m.M {
		d := math.Abs(x - m)
		if d < score {
			score = d
			class = k
		}
	}
	return class
}

func (m *Classes) Extra(x float64) int {
	hi := math.Abs(x - max64(m.M)*1)
	ex := math.Abs(x - max64(m.M)*3)
	if hi < ex {
		return m.Class(x)
	} else {
		return len(m.M)
	}
}
