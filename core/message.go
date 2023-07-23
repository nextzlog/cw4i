/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

import "math"

type Message struct {
	Data []float64
	Code string
	Freq int
	Time int
	Miss int
	Tone float64
	Mute float64
}

func (m Message) AGC(gain float64) []float64 {
	seq := make([]float64, len(m.Data))
	max := max64(m.Data)
	for n, v := range m.Data {
		seq[n] = v * math.Min(gain, max/v)
	}
	return seq
}

func (m Message) Merge(next Message) Message {
	return Message{
		Data: append(m.Data, next.Data...),
		Freq: m.Freq,
		Time: m.Time,
	}
}
