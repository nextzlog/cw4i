/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package audio

import (
	"bytes"
	"encoding/binary"
	"github.com/gen2brain/malgo"
	"github.com/nextzlog/CW4I/core/morse"
)

const INTERVAL_MS = 200

type Capture struct {
	context *Context
	devInfo malgo.DeviceInfo
	capture *malgo.Device
	Decoder morse.Decoder
	handler func([]morse.Message)
}

func (c *Capture) Name() string {
	return c.devInfo.Name()
}

func (c *Capture) Config() (cfg malgo.DeviceConfig) {
	cfg = malgo.DefaultDeviceConfig(malgo.Capture)
	cfg.PeriodSizeInMilliseconds = INTERVAL_MS
	cfg.Capture.Format = malgo.FormatS32
	cfg.Capture.Channels = 1
	cfg.Capture.DeviceID = c.devInfo.ID.Pointer()
	return
}

func (c *Capture) Listen(handler func([]morse.Message)) {
	ctx := c.context.ctx
	dcb := malgo.DeviceCallbacks{Data: c.onSignalEvent}
	c.capture, _ = malgo.InitDevice(ctx.Context, c.Config(), dcb)
	c.Decoder = morse.DefaultDecoder(int(c.capture.SampleRate()))
	c.handler = handler
	c.capture.Start()
}

func (c *Capture) Finish() {
	c.capture.Uninit()
}

func (c *Capture) onSignalEvent(out, in []byte, frames uint32) {
	ratio := malgo.SampleSizeInBytes(c.capture.CaptureFormat())
	buffer := bytes.NewReader(in)
	signal := make([]int32, len(in)/ratio)
	floats := make([]float64, len(signal))
	binary.Read(buffer, binary.LittleEndian, signal)
	for n, v := range signal {
		floats[n] = float64(v)
	}
	if c.handler != nil {
		c.handler(c.Decoder.Read(floats))
	}
}
