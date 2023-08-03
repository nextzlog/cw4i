/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package util

import (
	"bytes"
	"github.com/nextzlog/cw4i/core"
	"github.com/wcharczuk/go-chart/v2"
	"os"
)

func Plot(path string, message core.Message) {
	time := float64(int(message.Time))
	last := float64(len(message.Data)) + time
	plot := chart.ContinuousSeries{
		XValues: chart.LinearRange(time, last-1),
		YValues: message.Data,
		Style: chart.Style{
			FillColor: chart.GetDefaultColor(0),
		},
	}
	graph := chart.Chart{
		Title:  message.Text,
		Series: []chart.Series{plot},
		XAxis: chart.XAxis{
			Name: "Time",
		},
		YAxis: chart.YAxis{
			Name: "Amplitude",
		},
	}
	var buffer bytes.Buffer
	graph.Render(chart.SVG, &buffer)
	os.WriteFile(path, buffer.Bytes(), 0666)
}
