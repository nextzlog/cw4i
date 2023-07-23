/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package morse

import "math"

const (
	rise = 0.1
	fall = 0.1
)

type Encoder struct {
	buff []float64
	Freq float64
	WPMs float64
	Rate float64
}

func (e *Encoder) unit() float64 {
	return e.Rate * 60.0 / (e.WPMs * 50.0)
}

func (e *Encoder) size(code string) (cnt int) {
	for _, ch := range code {
		switch ch {
		case ' ':
			cnt += int(e.unit() * 3)
		case ';':
			cnt += int(e.unit() * 1)
		case '_':
			cnt += int(e.unit() * 4)
		case '.':
			cnt += int(e.unit() * 2)
		}
	}
	return cnt
}

func (e *Encoder) Tone(code string) []float64 {
	idx := 0
	e.buff = make([]float64, e.size(code))
	for _, ch := range code {
		switch ch {
		case ' ':
			idx = e.mute(3, idx)
		case ';':
			idx = e.mute(1, idx)
		case '_':
			idx = e.beep(3, idx)
			idx = e.mute(1, idx)
		case '.':
			idx = e.beep(1, idx)
			idx = e.mute(1, idx)
		}
	}
	return e.buff
}

func (e *Encoder) beep(time, idx int) int {
	step := 2 * math.Pi * e.Freq / e.Rate
	t1 := e.unit() * float64(rise)
	t2 := e.unit() * float64(fall)
	t3 := e.unit() * float64(time)
	for t := 0.0; t < t3; t += 1 {
		amp, r := 1.0, t3-t
		if t < t1 {
			amp *= t / t1
		} else if r < t2 {
			amp *= r / t2
		}
		e.buff[idx] = amp * math.Sin(t*step)
		idx += 1
	}
	return idx
}

func (e *Encoder) mute(time, idx int) int {
	time = int(e.unit() * float64(time))
	for t := 0; t < time; t++ {
		e.buff[idx] = 0
		idx += 1
	}
	return idx
}
