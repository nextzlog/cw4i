/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package morse

type Segment struct {
	Time int
	Prev int
	Down bool
	Span float64
}

func (m *Classes) Segments(first int) (result []Segment) {
	start := 0
	for n, x := range m.X {
		k := m.Class(x)
		if first != k {
			result = append(result, Segment{
				Time: n,
				Prev: start,
				Down: k == 0,
				Span: float64(n - start),
			})
			start = n
		}
		first = k
	}
	if len(result) > 1 {
		return result[1:]
	} else {
		return nil
	}
}

func (m *Classes) Code(segments []Segment) (code string) {
	for _, s := range segments {
		if s.Down {
			switch m.Class(s.Span) {
			case 0:
				code += "."
			case 1:
				code += "_"
			}
		} else {
			switch m.Extra(s.Span) {
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
