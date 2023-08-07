/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package view

import (
	"encoding/binary"
	"fyne.io/fyne/v2/widget"
	"github.com/gen2brain/malgo"
)

type Capture struct {
	Squelch float64
	Initial string
	Context malgo.Context
	Capture *malgo.Device
	Restart func(int)
	Handler func([]float64)
}

func (c *Capture) Start(deviceInfo malgo.DeviceInfo) {
	cfg := malgo.DefaultDeviceConfig(malgo.Capture)
	cfg.PeriodSizeInMilliseconds = 200
	cfg.Capture.Format = malgo.FormatS32
	cfg.Capture.DeviceID = deviceInfo.ID.Pointer()
	cfg.Capture.Channels = 1
	endian := binary.LittleEndian
	dcb := malgo.DeviceCallbacks{
		Data: func(out, in []byte, size uint32) {
			signal := make([]float64, size)
			for n := 0; n < len(in); n += 4 {
				v := endian.Uint32(in[n : n+4])
				signal[n/4] = float64(int32(v))
			}
			c.Handler(signal)
		},
	}
	c.Capture, _ = malgo.InitDevice(c.Context, cfg, dcb)
	c.Restart(int(c.Capture.SampleRate()))
	c.Capture.Start()
	return
}

func (c *Capture) OnDeviceSelected(selection string) {
	devices, _ := c.Context.Devices(malgo.Capture)
	if c.Capture != nil {
		c.Capture.Uninit()
	}
	for _, dev := range devices {
		if dev.Name() == selection {
			c.Start(dev)
		}
	}
	return
}

func (c *Capture) CanvasObject() (ui *widget.Select) {
	sel := widget.NewSelect(nil, c.OnDeviceSelected)
	devices, err := c.Context.Devices(malgo.Capture)
	for _, dev := range devices {
		sel.Options = append(sel.Options, dev.Name())
	}
	if err != nil {
		sel.Disable()
	} else if c.Initial == "" {
		sel.SetSelectedIndex(0)
	} else {
		sel.SetSelected(c.Initial)
	}
	return sel
}
