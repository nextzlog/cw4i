/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import "github.com/thoas/go-funk"

type Squelch struct {
	Squelch float64
	Density int
	Monitor Monitor
	Scanner Scanner
	History []Message
}

func DefaultSquelch(Rate int) Squelch {
	return Squelch{
		Squelch: 0.1,
		Density: 1,
		Monitor: DefaultMonitor(Rate),
		Scanner: DefaultScanner(),
	}
}

func (s *Squelch) Next(signal []float64) (result []Message) {
	data := s.Monitor.Next(signal)
	pows := make([]float64, len(data))
	for n, msg := range data {
		pows[n] = pow64(msg.Data)
	}
	var list []int
	th := s.Squelch * sum64(pows)
	for _, n := range top64(pows) {
		if pows[n] > th {
			list = s.Uniq(list, n)
		}
	}
	for _, msg := range s.History {
		list = append(list, msg.Freq)
	}
	list = funk.UniqInt(list)
	for _, n := range list {
		result = append(result, s.Scanner.Scan(data[n]))
	}
	return
}

func (s *Squelch) Uniq(list []int, freq int) (result []int) {
	for f := freq - s.Density; f <= freq+s.Density; f++ {
		for _, msg := range s.History {
			if msg.Freq == f {
				return append(list, f)
			}
		}
	}
	return append(list, freq)
}
