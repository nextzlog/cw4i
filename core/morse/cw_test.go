/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package morse

import "testing"

const (
	RATE = 48000
	TEXT = "CQ DE JA1ZLO"
)

func Test(t *testing.T) {
	failure := true
	encoder := Encoder{
		Freq: 600,
		WPMs: 10,
		Rate: RATE,
	}
	decoder := DefaultDecoder(RATE)
	tone := encoder.Tone(TextToCode(TEXT))
	for _, msg := range decoder.Read(tone) {
		if CodeToText(msg.Code) == TEXT {
			failure = false
		}
	}
	if failure {
		t.Error("no text decoded successfully")
	}
}
