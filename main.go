/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package main

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"github.com/dop251/goja"
	"github.com/gen2brain/malgo"
	"github.com/nextzlog/cw4i/core"
	"github.com/nextzlog/cw4i/util"
	"github.com/nextzlog/cw4i/view"
	"os"
)

const (
	DEV = "dev"
	SQL = "sql"
)

func main() {
	app := app.NewWithID("cw4i")
	cfg := malgo.ContextConfig{}
	app.Settings().SetTheme(theme.DarkTheme())
	ctx, _ := malgo.InitContext(nil, cfg, nil)
	win := app.NewWindow("CW4ISR Morse Decoder")
	decoder := view.Decoder{
		Initial: Script,
	}
	capture := view.Capture{
		Context: ctx.Context,
		Restart: decoder.Update,
		Handler: decoder.Decode,
		Initial: app.Preferences().String(DEV),
	}
	squelch := view.Squelch{
		Handler: decoder.Resquelch,
		Initial: app.Preferences().Float(SQL),
	}
	sel := capture.CanvasObject()
	his := decoder.CanvasObject()
	sql := squelch.CanvasObject()
	out := container.NewBorder(sel, sql, nil, nil, his)
	win.Resize(fyne.NewSize(640, 480))
	win.SetContent(out)
	win.ShowAndRun()
	ctx.Uninit()
	app.Preferences().SetFloat(SQL, sql.Value)
	app.Preferences().SetString(DEV, sel.Selected)
	return
}

func Script(rate int) (decoder core.Decoder, err error) {
	decoder = core.DefaultDecoder(rate)
	vm := goja.New()
	vm.Set("call", util.Call)
	vm.Set("plot", util.Plot)
	vm.Set("decoder", decoder)
	code, _ := os.ReadFile("cw4i.js")
	if _, err = vm.RunString(string(code)); err == nil {
		err = vm.ExportTo(vm.Get("decoder"), &decoder)
	}
	return
}
