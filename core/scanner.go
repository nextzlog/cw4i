/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import "github.com/thoas/go-funk"

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
	align := &Classes{X: source.AGC(s.Gain)}
	align.Train(2, s.Iter)
	clips := align.Segments(0)
	if clips == nil {
		return source
	}
	since := funk.Head(clips).(Segment).Prev
	until := funk.Last(clips).(Segment).Time
	level := &Classes{X: align.X[since : until+1]}
	level.Train(2, s.Iter)
	steps := level.Segments(0)
	tones := make([]float64, 0)
	for _, s := range steps {
		if s.Down {
			tones = append(tones, s.Span)
		}
	}
	speed := &Classes{X: tones}
	speed.Train(2, s.Iter)
	source.Code = speed.Code(steps)
	source.Text = CodeToText(source.Code)
	source.Tone = max64(level.M)
	source.Mute = min64(level.M)
	return source
}
