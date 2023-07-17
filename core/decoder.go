/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

import (
	"github.com/r9y9/gossp"
	"github.com/r9y9/gossp/stft"
	"math"
	"sort"
)

const (
	edge_step = 10
	edge_damp = 10
)

type Message struct {
	Data []float64
	Code string
	Freq int
	Time int
	Miss int
}

type Decoder struct {
	Iter int
	Bias int
	Hold int
	Miss int
	Gain float64
	Mute float64
	STFT *stft.STFT
	Spec [][]float64
	wave []float64
	prev []Message
	time int
}

func (d *Decoder) binary(signal []float64) (result []*step) {
	key := make([]float64, len(signal))
	max := max64(signal)
	for idx, val := range signal {
		key[idx] = val * math.Min(d.Gain, max/val)
	}
	gmm := means{X: key}
	gmm.optimize(d.Iter)
	result = gmm.steps()
	return
}

func (d *Decoder) detect(signal []float64) (result Message) {
	result.Data = make([]float64, len(signal))
	copy(result.Data, signal)
	steps := d.binary(signal)
	tones := make([]float64, 0)
	if len(steps) >= 1 {
		for idx, s := range steps[1:] {
			s.span = float64(s.time - steps[idx].time)
			if s.down {
				tones = append(tones, s.span)
			}
		}
	}
	if len(tones) >= 1 {
		gmm := &means{X: tones}
		gmm.optimize(d.Iter)
		for _, s := range steps[1:] {
			if s.down {
				result.Code += s.tone(gmm.class(s.span))
			} else {
				result.Code += s.mute(gmm.extra(s.span))
			}
		}
	}
	result.Time = d.time
	return
}

func (d *Decoder) search(series [][]float64) (result []int) {
	cut := d.STFT.FrameLen / 2
	pow := make([]float64, cut)
	for _, sp := range series {
		for n, v := range sp[:cut] {
			pow[n] += v * v
		}
	}
	bit := make(map[int]bool)
	lev := d.Mute * sum64(pow[d.Bias:])
	for _, freq := range top64(pow[:cut]) {
		if pow[freq] > lev && freq > d.Bias {
			bit[freq] = true
		}
	}
	for _, prev := range d.prev {
		bit[prev.Freq] = true
	}
	for freq := range bit {
		result = append(result, freq)
	}
	sort.Ints(result)
	return
}

func (d *Decoder) edge(signal []float64) (spec [][]float64) {
	spec, _ = gossp.SplitSpectrogram(d.STFT.STFT(signal))
	state := make([]float64, d.STFT.FrameLen)
	for n := 0; n < edge_step; n++ {
		for _, sp := range spec {
			copy(state, sp)
			for k, v := range sp[1 : len(sp)-1] {
				if state[k] > v || state[k+2] > v {
					sp[k+1] /= edge_damp
				}
			}
		}
	}
	return
}

func (d *Decoder) scan(signal []float64) (result []Message) {
	spec := d.edge(signal)
	wave := make([]float64, len(spec))
	for _, idx := range d.search(spec) {
		for t, s := range spec {
			wave[t] = s[idx]
		}
		next := d.detect(wave)
		next.Freq = idx
		result = append(result, next)
	}
	d.Spec = spec
	return
}

func (d *Decoder) Read(signal []float64) (result []Message) {
	shift := d.STFT.FrameShift
	if len(d.wave) > d.Hold {
		d.wave = d.wave[len(d.wave)-d.Hold:]
	}
	d.wave = append(d.wave, signal...)
	for _, next := range d.scan(d.wave) {
		for _, prev := range d.prev {
			if next.Freq == prev.Freq {
				drop := len(next.Data) - (len(signal) / shift)
				data := append(prev.Data, next.Data[drop:]...)
				next = d.detect(data)
				next.Freq = prev.Freq
				next.Time = prev.Time
				if next.Code == prev.Code {
					next.Miss = prev.Miss + 1
				}
			}
		}
		result = append(result, next)
	}
	d.time++
	d.prev = nil
	d.wave = signal
	for _, next := range result {
		if next.Miss <= d.Miss {
			d.prev = append(d.prev, next)
		}
	}
	return
}

func DefaultDecoder(SamplingRateInHz int) (decoder Decoder) {
	return Decoder{
		Iter: 5,
		Bias: 5,
		Miss: 2,
		Gain: 2,
		Mute: 0.2,
		Hold: SamplingRateInHz * 5,
		STFT: stft.New(SamplingRateInHz/100, 2048),
	}
}
