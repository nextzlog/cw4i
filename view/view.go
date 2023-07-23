/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package view

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"
	"github.com/nextzlog/CW4I/core/audio"
	"github.com/nextzlog/CW4I/core/morse"
	"net/url"
)

const (
	NAME = "CW4ISR Morse Decoder"
	HREF = "https://use.zlog.org"
	LINK = "Download Latest zLog"
)

var (
	ctx audio.Context
	dev *audio.Capture
	win fyne.Window
	his *widget.List
)

var (
	items []morse.Message
	table map[string]*audio.Capture
	names []string
)

func device(name string) {
	if dev != nil {
		dev.Finish()
	}
	dev = table[name]
	dev.Listen(func(list []morse.Message) {
		items = dev.Decoder.List.Display
		his.Refresh()
	})
}

func List() *widget.List {
	return widget.NewList(
		func() int {
			return len(items)
		},
		func() fyne.CanvasObject {
			return widget.NewLabel("")
		},
		func(id int, obj fyne.CanvasObject) {
			item := items[len(items)-id-1]
			label := obj.(*widget.Label)
			label.SetText(morse.CodeToText(item.Code))
		},
	)
}

func Show() (err error) {
	if ctx, err = audio.NewContext(); err == nil {
		names, table = ctx.Devices()
		app := app.New()
		app.Settings().SetTheme(theme.DarkTheme())
		his = List()
		win = app.NewWindow(NAME)
		ref, _ := url.Parse(HREF)
		btm := widget.NewHyperlink(LINK, ref)
		sel := widget.NewSelect(names, device)
		out := container.NewBorder(sel, btm, nil, nil, his)
		win.Resize(fyne.NewSize(640, 480))
		sel.SetSelectedIndex(0)
		win.SetContent(out)
		win.ShowAndRun()
		ctx.Finish()
	}
	return
}
