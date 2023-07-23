/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/
package core

type History struct {
	MaxMiss int
	MaxHold int
	MinTone float64
	Present []Message
	History []Message
	Display []Message
}

func DefaultHistory() History {
	return History{
		MaxMiss: 2,
		MaxHold: 100,
		MinTone: 10,
	}
}

func (h *History) Push(list []Message) {
	h.Present = nil
	h.Display = nil
	for _, msg := range list {
		h.enqueue(msg)
	}
	for _, msg := range h.History {
		h.display(msg)
	}
	for _, msg := range h.Present {
		h.display(msg)
	}
}

func (h *History) enqueue(msg Message) {
	if msg.Miss <= h.MaxMiss {
		h.Present = append(h.Present, msg)
	} else {
		h.History = append(h.History, msg)
		if len(h.History) >= h.MaxHold {
			h.History = h.History[1:]
		}
	}
}

func (h *History) display(msg Message) {
	if msg.Tone > h.MinTone*msg.Mute {
		h.Display = append(h.Display, msg)
	}
}
