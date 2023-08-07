/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package core

type History struct {
	Items []Message
	Added func()
}

func (h *History) Length() (count int) {
	return len(h.Items)
}

func (h *History) Add(items []Message) {
	for _, message := range items {
		h.add(message)
	}
	if h.Added != nil {
		h.Added()
	}
}

func (h *History) add(message Message) {
	for n, prev := range h.Items {
		time := prev.Time == message.Time
		freq := prev.Freq == message.Freq
		if time && freq {
			h.Items[n] = message
			return
		}
	}
	h.Items = append(h.Items, message)
}
