/*******************************************************************************
 * Amateur Radio Operational Logging Software 'ZyLO' since 2020 June 22nd
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

import "testing"

const (
	RATE = 48000
	TEXT = "CQ DE JA1ZLO"
)

func TestEnDe(t *testing.T) {
	encoder := Encoder{
		Freq: 600,
		WPMs: 10,
		Rate: RATE,
	}
	decoder := DefaultDecoder(RATE)
	tone := encoder.Tone(TextToCode(TEXT))
	for _, msg := range decoder.Read(tone) {
		if CodeToText(msg.Code) != TEXT {
			t.Errorf("%s != %s", msg.Code, TEXT)
		} else {
			return
		}
	}
	t.Error("no text decoded successfully")
}
