/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package view

import "fyne.io/fyne/v2/widget"

const (
	SQL_MIN = 1
	SQL_MAX = 100
)

type Squelch struct {
	Handler func(float64)
	Initial float64
}

func (s *Squelch) CanvasObject() (ui *widget.Slider) {
	sql := widget.NewSlider(SQL_MIN, SQL_MAX)
	sql.OnChanged = s.Handler
	sql.SetValue(s.Initial)
	return sql
}
