/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package view

import (
	"encoding/binary"
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"
	"github.com/gen2brain/malgo"
	"github.com/nextzlog/CW4I/core"
	"github.com/thoas/go-funk"
	"github.com/wcharczuk/go-chart/v2"
	"net/url"
	"unsafe"
)

const INTERVAL_MS = 200

const (
	NAME = "CW4ISR Morse Decoder"
	HREF = "https://use.zlog.org"
	LINK = "Download Latest zLog"
)

var (
	ctx *malgo.AllocatedContext
	dev *Device
)

var (
	items []core.Message
	table map[string]*Device
	names []string
)

var (
	his *widget.List
	osc *canvas.Image
)

var dcb = malgo.DeviceCallbacks{
	Data: onSignalEvent,
}

type Device struct {
	pointer unsafe.Pointer
	capture *malgo.Device
	decoder core.Decoder
}

func (dev *Device) Config() (cfg malgo.DeviceConfig) {
	cfg = malgo.DefaultDeviceConfig(malgo.Capture)
	cfg.PeriodSizeInMilliseconds = INTERVAL_MS
	cfg.Capture.Format = malgo.FormatS32
	cfg.Capture.Channels = 1
	cfg.Capture.DeviceID = dev.pointer
	return
}

func (dev *Device) Listen() {
	dev.capture, _ = malgo.InitDevice(ctx.Context, dev.Config(), dcb)
	dev.decoder = core.DefaultDecoder(int(dev.capture.SampleRate()))
	dev.capture.Start()
}

func (dev *Device) Stop() {
	dev.capture.Uninit()
}

func DeviceList() (table map[string]*Device, names []string) {
	if ctx != nil {
		table = make(map[string]*Device)
		devs, _ := ctx.Devices(malgo.Capture)
		for _, dev := range devs {
			names = append(names, dev.Name())
			table[dev.Name()] = &Device{
				pointer: dev.ID.Pointer(),
			}
		}
	}
	return
}

func onSignalEvent(out, in []byte, frames uint32) {
	dev.decoder.Read(readSignedInt(in))
	items = dev.decoder.List.Display
	his.Refresh()
	oscilloscope()
}

func readSignedInt(signal []byte) (result []float64) {
	for _, b := range funk.Chunk(signal, 4).([][]byte) {
		v := binary.LittleEndian.Uint32(b)
		result = append(result, float64(int32(v)))
	}
	return
}

func oscilloscope() {
	graph := chart.Chart{
		Width:  int(osc.Size().Width),
		Height: int(osc.Size().Height),
	}
	graph.XAxis.Style.Hidden = true
	graph.YAxis.Style.Hidden = true
	for _, item := range dev.decoder.List.Present {
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
	text := core.CodeToText(items[len(items)-id-1].Code)
	obj.(*widget.Label).SetText(text)
}

func restart(name string) {
	if dev != nil {
		dev.Stop()
	}
	dev = table[name]
	dev.Listen()
}

func clear() {
	items = nil
	if dev != nil {
		dev.decoder.List.History = nil
	}
	his.Refresh()
}

func Show() {
	ctx, _ = malgo.InitContext(nil, malgo.ContextConfig{}, nil)
	app := app.New()
	app.Settings().SetTheme(theme.DarkTheme())
	win := app.NewWindow(NAME)
	ref, _ := url.Parse(HREF)
	btm := widget.NewHyperlink(LINK, ref)
	osc = &canvas.Image{}
	his = widget.NewList(length, create, update)
	table, names = DeviceList()
	sel := widget.NewSelect(names, restart)
	btn := widget.NewButton("clear", clear)
	sel.SetSelectedIndex(0)
	vsp := container.NewVSplit(his, osc)
	bar := container.NewBorder(nil, nil, nil, btn, sel)
	out := container.NewBorder(bar, btm, nil, nil, vsp)
	win.SetContent(out)
	win.Resize(fyne.NewSize(640, 480))
	win.ShowAndRun()
}
