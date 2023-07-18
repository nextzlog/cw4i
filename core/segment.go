/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

type Segment struct {
	time int
	down bool
	span float64
}

func (m *classes) segments(first int) (result []Segment) {
	start := 0
	for n, x := range m.X {
		k := m.class(x)
		if first != k {
			result = append(result, Segment{
				time: n,
				down: k == 0,
				span: float64(n - start),
			})
			start = n
		}
		first = k
	}
	return result[1:]
}

func (m *classes) code(segments []Segment) (code string) {
	for _, s := range segments {
		if s.down {
			switch m.class(s.span) {
			case 0:
				code += "."
			case 1:
				code += "_"
			}
		} else {
			switch m.extra(s.span) {
			case 0:
				code += ""
			case 1:
				code += " "
			default:
				code += " ; "
			}
		}
	}
	return
}
