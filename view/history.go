/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package view

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/widget"
	"github.com/nextzlog/cw4i/core"
)

type History struct {
	core.History
}

func (h *History) canvas() fyne.CanvasObject {
	return widget.NewLabel("")
}

func (h *History) update(id int, obj fyne.CanvasObject) {
	item := h.Items[len(h.Items)-id-1]
	label := obj.(*widget.Label)
	label.SetText(item.Text)
}

func (h *History) CanvasObject() (ui fyne.CanvasObject) {
	list := widget.NewList(h.Length, h.canvas, h.update)
	h.Added = func() {
		list.Refresh()
	}
	return list
}
