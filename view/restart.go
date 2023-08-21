/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package view

import "fyne.io/fyne/v2/widget"

type Restart struct {
	Handler func()
}

func (r *Restart) CanvasObject() (ui *widget.Button) {
	return widget.NewButton("Restart", r.Handler)
}
