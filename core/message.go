/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import "math"

type Message struct {
	Data []float64
	Body []Segment
	Code string
	Text string
	Freq int
	Time int
	Miss int
}

func (m Message) Distinct(thre float64) bool {
	var tones []float64
	var mutes []float64
	for _, step := range m.Body {
		if step.Class {
			tones = append(tones, step.Level)
		} else {
			mutes = append(mutes, step.Level)
		}
	}
	tone := med64(tones)
	mute := med64(mutes)
	return tone > thre*mute
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
