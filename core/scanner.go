/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

type Scanner struct {
	Iter int
	Gain float64
}

func DefaultScanner() Scanner {
	return Scanner{
		Iter: 5,
		Gain: 2,
	}
}

func (s *Scanner) Scan(source Message) Message {
	level := &Classes{X: source.AGC(s.Gain)}
	level.Train(2, s.Iter)
	steps := level.Segments(0)
	var spans []float64
	for _, s := range steps {
		if s.Class {
			spans = append(spans, s.Width)
		}
	}
	speed := &Classes{X: spans}
	speed.Train(2, s.Iter)
	source.Body = steps
	source.Code = speed.Code(steps)
	source.Text = CodeToText(source.Code)
	return source
}
