/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import "github.com/thoas/go-funk"

type Decoder struct {
	Scatter int
	MaxMiss int
	Squelch float64
	Monitor Monitor
	Scanner Scanner
	History []Message
	Program func(Message) Message
}

func DefaultDecoder(Rate int) Decoder {
	return Decoder{
		Scatter: 1,
		MaxMiss: 2,
		Monitor: DefaultMonitor(Rate),
		Scanner: DefaultScanner(),
	}
}

func (d *Decoder) uniq(list []int, freq int) (result []int) {
	for _, m := range d.History {
		lower := m.Freq >= freq-d.Scatter
		upper := m.Freq <= freq+d.Scatter
		if lower && upper {
			return append(list, m.Freq)
		}
	}
	return append(list, freq)
}

func (d *Decoder) Next(signal []float64) (result []Message) {
	data := d.Monitor.Next(signal)
	pows := make([]float64, len(data))
	for n, msg := range data {
		pows[n] = pow64(msg.Data)
	}
	var list []int
	for _, n := range top64(pows) {
		if pows[n] > d.Squelch {
			list = d.uniq(list, n)
		}
	}
	for _, msg := range d.History {
		list = append(list, msg.Freq)
	}
	list = funk.UniqInt(list)
	for _, n := range list {
		result = append(result, d.Scanner.Scan(data[n]))
	}
	return
}

func (d *Decoder) Read(signal []float64) (result []Message) {
	for _, next := range d.Next(signal) {
		for _, prev := range d.History {
			if prev.Freq == next.Freq {
				next = d.Scanner.Scan(prev.Merge(next))
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
	d.History = result
	return
}
