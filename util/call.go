/*******************************************************************************
 * Audio Morse Decoder 'CW4ISR' (forked from Project ZyLO since 2023 July 15th)
 * Released under the MIT License (or GPL v3 until 2021 Oct 28th) (see LICENSE)
 * Univ. Tokyo Amateur Radio Club Development Task Force (https://nextzlog.dev)
*******************************************************************************/

package util

import "os/exec"

func Call(cmd string, args ...string) (result string) {
	out, _ := exec.Command(cmd, args...).Output()
	return string(out)
}
