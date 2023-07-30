/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import "sort"

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

func med64(x []float64) (med float64) {
	xs := make([]float64, len(x))
	copy(xs, x)
	sort.Float64s(xs)
	return xs[len(xs)/2]
}

func sum64(x []float64) (sum float64) {
	for _, v := range x {
		sum += v
	}
	return
}

func pow64(x []float64) (pow float64) {
	for _, v := range x {
		pow += v * v
	}
	pow /= float64(len(x))
	return
}

func top64(x []float64) (index []int) {
	for k := 1; k < len(x)-1; k++ {
		v := x[k]
		p := x[k-1]
		n := x[k+1]
		if p < v && v > n {
			index = append(index, k)
		}
	}
	return
}
