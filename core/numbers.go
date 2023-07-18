/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

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
