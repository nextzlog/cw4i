/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package morse

import (
	_ "embed"
	"gopkg.in/yaml.v2"
	"strings"
)

//go:embed latin.yaml
var latin string
var reverse = make(map[string]string)
var forward = make(map[string]string)

func init() {
	yaml.UnmarshalStrict([]byte(latin), &forward)
	for key, value := range forward {
		reverse[value] = key
	}
}

func CodeToText(code string) (result string) {
	for _, s := range strings.Split(code, " ") {
		if val, ok := reverse[s]; ok {
			result += string(val)
		} else {
			result += "?"
		}
	}
	return
}

func TextToCode(text string) (result string) {
	for _, s := range strings.Split(text, "") {
		result += " " + forward[s]
	}
	if result != "" {
		return result[1:]
	} else {
		return
	}
}
