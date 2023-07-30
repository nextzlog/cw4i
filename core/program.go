/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

import (
	"errors"
	"github.com/dop251/goja"
	"os"
)

func ScriptPath() string {
	return "cw4i.js"
}

func Program(rate int) (decoder Decoder, err error) {
	decoder = DefaultDecoder(rate)
	vm := goja.New()
	code, e1 := os.ReadFile(ScriptPath())
	fun, e2 := vm.RunString(string(code))
	if err = errors.Join(e1, e2); err == nil {
		err = vm.ExportTo(fun, &decoder.Program)
	}
	return
}
