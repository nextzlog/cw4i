/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package view

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"
	"github.com/nextzlog/CW4I/core/audio"
	"github.com/nextzlog/CW4I/core/morse"
	"github.com/wcharczuk/go-chart/v2"
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
)

var (
	items []morse.Message
	table map[string]*audio.Capture
	names []string
)

var (
	his *widget.List
	osc *canvas.Image
)

func onDecodeEvent(messages []morse.Message) {
	items = dev.Decoder.List.Display
	his.Refresh()
	oscilloscope()
}

func oscilloscope() {
	graph := chart.Chart{
		Width:  int(osc.Size().Width),
		Height: int(osc.Size().Height),
	}
	graph.XAxis.Style.Hidden = true
	graph.YAxis.Style.Hidden = true
	for _, item := range dev.Decoder.List.Present {
		t := float64(item.Time)
		s := float64(len(item.Data))
		g := chart.ContinuousSeries{
			XValues: chart.LinearRange(t, t+s-1),
			YValues: item.Data,
		}
		graph.Series = append(graph.Series, g)
	}
	buffer := &chart.ImageWriter{}
	graph.Render(chart.PNG, buffer)
	image, _ := buffer.Image()
	osc.Image = image
	osc.Refresh()
}

func length() int {
	return len(items)
}

func create() fyne.CanvasObject {
	return widget.NewLabel("")
}

func update(id widget.ListItemID, obj fyne.CanvasObject) {
	text := morse.CodeToText(items[len(items)-id-1].Code)
	obj.(*widget.Label).SetText(text)
}

func restart(name string) {
	if dev != nil {
		dev.Finish()
	}
	dev = table[name]
	dev.Listen(onDecodeEvent)
}

func clear() {
	items = nil
	if dev != nil {
		dev.Decoder.List.History = nil
	}
	his.Refresh()
}

func Show() (err error) {
	if ctx, err = audio.NewContext(); err == nil {
		names, table = ctx.Devices()
		app := app.New()
		app.Settings().SetTheme(theme.DarkTheme())
		win := app.NewWindow(NAME)
		ref, _ := url.Parse(HREF)
		btm := widget.NewHyperlink(LINK, ref)
		osc = &canvas.Image{}
		his = widget.NewList(length, create, update)
		sel := widget.NewSelect(names, restart)
		btn := widget.NewButton("clear", clear)
		sel.SetSelectedIndex(0)
		vsp := container.NewVSplit(his, osc)
		bar := container.NewBorder(nil, nil, nil, btn, sel)
		out := container.NewBorder(bar, btm, nil, nil, vsp)
		win.SetContent(out)
		win.Resize(fyne.NewSize(640, 480))
		win.ShowAndRun()
		ctx.Finish()
	}
	return
}
