/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package audio

import (
	"github.com/gen2brain/malgo"
)

type Context struct {
	ctx *malgo.AllocatedContext
}

func NewContext() (context Context, err error) {
	context.ctx, err = malgo.InitContext(nil, malgo.ContextConfig{}, nil)
	return
}

func (c *Context) Finish() {
	c.ctx.Uninit()
	c.ctx.Free()
}

func (c *Context) Devices() (list []string, table map[string]*Capture) {
	if all, err := c.ctx.Devices(malgo.Capture); err == nil {
		table = make(map[string]*Capture)
		for _, info := range all {
			list = append(list, info.Name())
			table[info.Name()] = &Capture{context: c, devInfo: info}
		}
	}
	return
}
