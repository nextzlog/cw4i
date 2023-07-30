/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import "sort"

type Decoder struct {
	Tone float64
	Mute float64
	Scan Scanner
	Moni Monitor
	List History
}

func DefaultDecoder(Rate int) Decoder {
	return Decoder{
		Tone: 10,
		Mute: 0.1,
		Scan: DefaultScanner(),
		Moni: DefaultMonitor(Rate),
		List: DefaultHistory(),
	}
}

func (d *Decoder) Next(signal []float64) (result []Message) {
	all := d.Moni.Next(signal)
	pow := make([]float64, len(all))
	set := make(map[int]bool)
	var seq []int
	for idx, msg := range all {
		pow[idx] = pow64(msg.Data)
	}
	min := d.Mute * sum64(pow)
	for _, idx := range top64(pow) {
		if pow[idx] > min {
			set[idx] = true
		}
	}
	for _, msg := range d.List.Present {
		set[msg.Freq] = true
	}
	for idx, _ := range set {
		seq = append(seq, idx)
	}
	sort.Ints(seq)
	for _, idx := range seq {
		next := d.Scan.Scan(all[idx])
		result = append(result, next)
	}
	return
}

func (d *Decoder) Read(signal []float64) (result []Message) {
	for _, next := range d.Next(signal) {
		for _, prev := range d.List.Present {
			if next.Freq == prev.Freq {
				next = prev.Merge(next)
				next = d.Scan.Scan(next)
				if next.Code == prev.Code {
					next.Miss = prev.Miss + 1
				}
			}
		}
		result = append(result, next)
	}
	d.List.Push(result)
	return
}
