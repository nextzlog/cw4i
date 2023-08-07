/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package view

import (
	"github.com/nextzlog/cw4i/core"
	"math"
)

type Decoder struct {
	Initial func(int) (core.Decoder, error)
	Decoder core.Decoder
	Squelch float64
	History
}

func (d *Decoder) Resquelch(level float64) {
	d.Squelch = math.Pow(10, level)
	d.Decoder.Squelch = d.Squelch
}

func (d *Decoder) Update(samplingRate int) {
	d.Decoder, _ = d.Initial(samplingRate)
	d.Decoder.Squelch = d.Squelch
}

func (d *Decoder) Decode(signal []float64) {
	d.Add(d.Decoder.Read(signal))
}
