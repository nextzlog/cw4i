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
	M []float64
}

func (m *classes) train(K, epochs int) {
	min := min64(m.X)
	ptp := max64(m.X) - min
	for k := 0; k < K; k++ {
		t := float64(k) / float64(K)
		m.M = append(m.M, min+t*ptp)
	}
	for i := 0; i < epochs; i++ {
		sum1 := make([]float64, K)
		sumX := make([]float64, K)
		for _, x := range m.X {
			k := m.class(x)
			sum1[k] += 1
			sumX[k] += x
		}
		for k, x := range sumX {
			m.M[k] = x / sum1[k]
		}
	}
	sort.Float64s(m.M)
}

func (m *classes) class(x float64) int {
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

func (m *classes) extra(x float64) int {
	hi := math.Abs(x - max64(m.M)*1)
	ex := math.Abs(x - max64(m.M)*3)
	if hi < ex {
		return m.class(x)
	} else {
		return len(m.M)
	}
}

func (m *classes) computeBIC() float64 {
	sum1 := make([]float64, len(m.M))
	sumD := make([]float64, len(m.M))
	like := 0.0
	X := float64(len(m.X))
	M := float64(len(m.M))
	for _, x := range m.X {
		k := m.class(x)
		d := x - m.M[k]
		sum1[k] += 1
		sumD[k] += d * d
	}
	for k, _ := range m.M {
		sumD[k] /= sum1[k]
	}
	for _, x := range m.X {
		k := m.class(x)
		n := gaussian(x, m.M[k], sumD[k])
		like += math.Log(n)
	}
	return M*math.Log(X) - 2*like
}

func gaussian(x, m, s float64) float64 {
	d := x - m
	e := math.Exp(-0.5 * d * d / s)
	r := math.Sqrt(2 * math.Pi * s)
	return e / r
}
