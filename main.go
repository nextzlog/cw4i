/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package main

import (
	"bytes"
	"encoding/binary"
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"
	"github.com/gen2brain/malgo"
	"github.com/nextzlog/CW4I/core"
	"net/url"
)

var cfg malgo.ContextConfig

func main() {
	app := app.New()
	app.Settings().SetTheme(theme.DarkTheme())
	ctx, _ := malgo.InitContext(nil, cfg, nil)
	win := app.NewWindow("CW4ISR Morse Decoder")
	history := new(History)
	capture := Capture{
		Context: ctx.Context,
		Handler: history.Add,
	}
	sel := capture.Component()
	his := history.Component()
	btm := LinkToZLOG()
	out := container.NewBorder(sel, btm, nil, nil, his)
	win.Resize(fyne.NewSize(640, 480))
	win.SetContent(out)
	win.ShowAndRun()
	ctx.Uninit()
	return
}

type Capture struct {
	Context malgo.Context
	Capture *malgo.Device
	Handler func([]core.Message)
}

func (c *Capture) Run(dev malgo.DeviceInfo) (err error) {
	cfg := malgo.DefaultDeviceConfig(malgo.Capture)
	cfg.PeriodSizeInMilliseconds = 200
	cfg.Capture.Format = malgo.FormatS32
	cfg.Capture.DeviceID = dev.ID.Pointer()
	cfg.Capture.Channels = 1
	var de core.Decoder
	cb := malgo.DeviceCallbacks{
		Data: func(out, in []byte, size uint32) {
			c.Handler(de.Read(c.s32(in, size)))
		},
	}
	c.Capture, _ = malgo.InitDevice(c.Context, cfg, cb)
	de = core.DefaultDecoder(int(c.Capture.SampleRate()))
	c.Capture.Start()
	return
}

func (c *Capture) s32(in []byte, size uint32) []float64 {
	buffer := bytes.NewReader(in)
	signal := make([]float64, int(size))
	int32s := make([]int32, len(signal))
	binary.Read(buffer, binary.LittleEndian, int32s)
	for n, v := range int32s {
		signal[n] = float64(v)
	}
	return signal
}

func (c *Capture) Component() (view *widget.Select) {
	devices, _ := c.Context.Devices(malgo.Capture)
	view = widget.NewSelect(nil, func(name string) {
		if c.Capture != nil {
			c.Capture.Uninit()
			c.Capture = nil
		}
		for _, dev := range devices {
			if dev.Name() == name {
				c.Run(dev)
			}
		}
	})
	for _, dev := range devices {
		view.Options = append(view.Options, dev.Name())
	}
	view.SetSelectedIndex(0)
	return
}

type History struct {
	Items []core.Message
	views []*widget.List
}

func (h *History) Component() (view *widget.List) {
	view = widget.NewList(
		func() int {
			return len(h.Items)
		},
		func() fyne.CanvasObject {
			return widget.NewLabel("")
		},
		func(id int, obj fyne.CanvasObject) {
			item := h.Items[len(h.Items)-id-1]
			label := obj.(*widget.Label)
			label.SetText(item.Text)
		},
	)
	h.views = append(h.views, view)
	return
}

func (h *History) Add(list []core.Message) {
	for _, next := range list {
		lonely := true
		for n, prev := range h.Items {
			time := next.Time == prev.Time
			freq := next.Freq == prev.Freq
			if time && freq {
				h.Items[n] = next
				lonely = false
				break
			}
		}
		if lonely {
			h.Items = append(h.Items, next)
		}
	}
	for _, view := range h.views {
		view.Refresh()
	}
}

func LinkToZLOG() (view *widget.Hyperlink) {
	ref, _ := url.Parse("https://use.zlog.org/downloads")
	view = widget.NewHyperlink("Download zLog here", ref)
	return
}
