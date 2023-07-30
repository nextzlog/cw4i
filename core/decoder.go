/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

type Decoder struct {
	MaxMiss int
	Clarity float64
	Squelch Squelch
	Program func(Message) Message
}

func DefaultDecoder(Rate int) Decoder {
	return Decoder{
		MaxMiss: 2,
		Clarity: 10,
		Squelch: DefaultSquelch(Rate),
	}
}

func (d *Decoder) Next(signal []float64) (result []Message) {
	for _, next := range d.Squelch.Next(signal) {
		for _, prev := range d.Squelch.History {
			if prev.Freq == next.Freq {
				next = d.Squelch.Scanner.Scan(prev.Merge(next))
				if next.Code == prev.Code {
					next.Miss = prev.Miss + 1
				}
			}
		}
		if d.Program != nil {
			next = d.Program(next)
		}
		if next.Miss <= d.MaxMiss {
			result = append(result, next)
		}
	}
	d.Squelch.History = result
	return
}

func (d *Decoder) Read(signal []float64) (result []Message) {
	for _, next := range d.Next(signal) {
		if next.Distinct(d.Clarity) {
			result = append(result, next)
		}
	}
	return
}
