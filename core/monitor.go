/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

import (
	"github.com/r9y9/gossp/stft"
	"math/cmplx"
)

type Monitor struct {
	Bass int
	Band int
	Rate int
	Time int
	STFT stft.STFT
	prev []float64
}

func DefaultMonitor(Rate int) Monitor {
	return Monitor{
		Bass: 100,
		Band: 2000,
		Rate: Rate,
		STFT: *stft.New(Rate/100, 2048),
	}
}

func (m *Monitor) Next(signal []float64) (result []Message) {
	bass := int(m.Bass * m.STFT.FrameLen / m.Rate)
	band := int(m.Band * m.STFT.FrameLen / m.Rate)
	stft := m.STFT.STFT(append(m.prev, signal...))
	size := len(signal) / m.STFT.FrameShift
	if len(stft) < size {
		size = len(stft)
	}
	for freq := 0; freq < band; freq++ {
		result = append(result, Message{
			Data: make([]float64, size),
			Freq: freq,
			Time: m.Time,
		})
	}
	for t, spec := range stft[len(stft)-size:] {
		for f, v := range spec[bass:band] {
			result[f].Data[t] = cmplx.Abs(v)
		}
	}
	m.prev = signal
	m.Time += size
	return
}
