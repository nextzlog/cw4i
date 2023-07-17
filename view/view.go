/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
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
	"image/color"
	"net/url"
	"unsafe"
)

const (
	INTERVAL_MS = 200
	MAX_HISTORY = 100
	VOL_MAX_VAL = 100
)

const (
	NAME = "CW4ISR Morse Decoder"
	HREF = "https://use.zlog.org"
	LINK = "Download Latest zLog"
)

const GRAPH_WIDTH = 500

var (
	ctx *malgo.AllocatedContext
	dev *Device
)

var (
	items []core.Message
	alive []core.Message
	table map[string]*Device
	names []string
	graph [][]float64
	level float64
)

var (
	his *widget.List
	osc *canvas.Image
	spa *canvas.Raster
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
	alive = dev.decoder.Read(readSignedInt(in))
	for _, m := range alive {
		miss := true
		for n, p := range items {
			freq := m.Freq == p.Freq
			time := m.Time == p.Time
			if freq && time {
				items[n] = m
				miss = false
			}
		}
		if miss {
			items = append(items, m)
		}
	}
	if len(items) > MAX_HISTORY {
		items = items[len(items)-MAX_HISTORY:]
	}
	graph = append(graph, dev.decoder.Spec...)
	if len(graph) > GRAPH_WIDTH {
		graph = graph[len(graph)-GRAPH_WIDTH:]
	}
	level = 0.0
	for _, row := range graph {
		for _, v := range row {
			if v > level {
				level = v
			}
		}
	}
	his.Refresh()
	spa.Refresh()
	oscilloscope()
}

func readSignedInt(signal []byte) (result []float64) {
	for _, b := range funk.Chunk(signal, 4).([][]byte) {
		v := binary.LittleEndian.Uint32(b)
		result = append(result, float64(int32(v)))
	}
	return
}

func spectrogram(x, y, w, h int) (pixel color.Color) {
	x = x * GRAPH_WIDTH / w
	y = (h - y) * 100 / h
	value := 0.0
	width := GRAPH_WIDTH - len(graph)
	if x > width {
		value = graph[x-width][y] / level
	}
	return color.RGBA{
		R: uint8(255 * value),
		G: uint8(255 * value),
		B: 0,
		A: 255,
	}
}

func oscilloscope() {
	graph := chart.Chart{
		Width:  int(osc.Size().Width),
		Height: int(osc.Size().Height),
	}
	graph.XAxis.Style.Hidden = true
	graph.YAxis.Style.Hidden = true
	for _, item := range alive {
		x := float64(len(item.Data))
		g := chart.ContinuousSeries{
			XValues: chart.LinearRange(1, x),
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
	his.Refresh()
}

func volume(vol float64) {
	dev.decoder.Mute = vol / VOL_MAX_VAL
}

func Show() {
	ctx, _ = malgo.InitContext(nil, malgo.ContextConfig{}, nil)
	app := app.New()
	app.Settings().SetTheme(theme.DarkTheme())
	win := app.NewWindow(NAME)
	ref, _ := url.Parse(HREF)
	btm := widget.NewHyperlink(LINK, ref)
	osc = &canvas.Image{}
	spa = canvas.NewRasterWithPixels(spectrogram)
	his = widget.NewList(length, create, update)
	table, names = DeviceList()
	sel := widget.NewSelect(names, restart)
	vol := widget.NewSlider(0, VOL_MAX_VAL)
	btn := widget.NewButton("clear", clear)
	vol.OnChanged = volume
	sel.SetSelectedIndex(0)
	vol.SetValue(0.3 * VOL_MAX_VAL)
	mon := container.NewGridWithRows(2, osc, spa)
	vsp := container.NewVSplit(his, mon)
	bar := container.NewBorder(nil, nil, sel, btn, vol)
	out := container.NewBorder(bar, btm, nil, nil, vsp)
	win.SetContent(out)
	win.Resize(fyne.NewSize(640, 480))
	win.ShowAndRun()
}
