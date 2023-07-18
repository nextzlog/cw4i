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

type Decoder struct {
	Iter int
	Hold int
	Miss int
	Bias int
	Gain float64
	Mute float64
	STFT *stft.STFT
	Spec [][]float64
	wave []float64
	prev []Message
	time int
}

func (d *Decoder) decode(signal []float64) (result Message) {
	key := make([]float64, len(signal))
	max := max64(signal)
	for idx, val := range signal {
		key[idx] = val * math.Min(d.Gain, max/val)
	}
	gmm1 := &classes{X: key}
	gmm2 := &classes{X: key}
	gmm1.train(1, d.Iter)
	gmm2.train(2, d.Iter)
	like1 := gmm1.computeBIC()
	like2 := gmm2.computeBIC()
	if like1 > like2 {
		seq := gmm2.segments(0)
		tones := make([]float64, 0)
		for _, s := range seq {
			if s.down {
				tones = append(tones, s.span)
			}
		}
		gmm3 := &classes{X: tones}
		gmm3.train(2, d.Iter)
		result.Code = gmm3.code(seq)
		result.Time = d.time
	}
	result.Data = signal
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

func (d *Decoder) scan(signal []float64) (result []Message) {
	spec, _ := gossp.SplitSpectrogram(d.STFT.STFT(signal))
	for _, f := range d.search(spec) {
		wave := make([]float64, len(spec))
		for t, sp := range spec {
			wave[t] = sp[f]
		}
		next := d.decode(wave)
		next.Freq = f
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
				next = d.decode(data)
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
		Miss: 2,
		Bias: 10,
		Gain: 2,
		Mute: 0.1,
		Hold: SamplingRateInHz * 5,
		STFT: stft.New(SamplingRateInHz/100, 2048),
	}
}
