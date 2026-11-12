// SPDX-License-Identifier: GPL-2.0-only

use crate::dmr_sync::{embedded_signalling, fragment_matches, VoiceBurst};
use libloading::Library;
use std::{
    ffi::{c_char, c_void},
    path::PathBuf,
    ptr::NonNull,
    time::{Duration, Instant},
};

pub const RATE: u32 = 8000;
const BURST_BYTES: usize = 3 * 160 * size_of::<i16>();
type Frame = [[c_char; 24]; 4];
type Allocate = unsafe extern "C" fn() -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);
type Decode = unsafe extern "C" fn(*mut c_void, *mut [c_char; 24], *mut i16);

pub struct Vocoder {
    state: NonNull<c_void>,
    allocate: Allocate,
    free: Free,
    decode: Decode,
    _library: Library,
    pending: Option<VoiceBurst>,
    next_burst: usize,
    color: Option<u8>,
    private: bool,
    last_audio: Option<Instant>,
}

impl Vocoder {
    pub fn open() -> Result<Self, String> {
        let path = std::env::var_os("SDR_STUDIO_DMR_LIBRARY")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .map(|path| path.with_file_name("libsdstudio-dmr.so"))
            })
            .ok_or(crate::tr!(
                "No se pudo localizar el módulo de voz DMR",
                "Could not locate the DMR voice module"
            ))?;
        // These signatures are the ABI of the companion native/dmr_audio.c.
        unsafe {
            let lib = Library::new(path)
                .map_err(|error| crate::tr_format!("Voz DMR: {error}", "DMR voice: {error}"))?;
            let allocate: Allocate = *lib.get(b"sdr_dmr_voice_new\0").map_err(module_error)?;
            let free: Free = *lib.get(b"sdr_dmr_voice_free\0").map_err(module_error)?;
            let decode: Decode = *lib.get(b"sdr_dmr_voice_decode\0").map_err(module_error)?;
            let state = NonNull::new(allocate()).ok_or(crate::tr!(
                "No se pudo iniciar el vocodificador DMR",
                "Could not start the DMR vocoder"
            ))?;
            Ok(Self {
                state,
                allocate,
                free,
                decode,
                _library: lib,
                pending: None,
                next_burst: 0,
                color: None,
                private: false,
                last_audio: None,
            })
        }
    }

    pub fn process(
        &mut self,
        burst: VoiceBurst,
        first: bool,
        volume: f32,
    ) -> Result<Vec<u8>, String> {
        if first {
            // Burst A carries sync in place of the privacy indicator.
            self.pending = Some(burst);
            self.next_burst = 1;
            self.color = None;
            self.private = false;
            return Ok(Vec::new());
        }
        if self.next_burst == 0 || self.next_burst > 5 || self.private {
            return Ok(Vec::new());
        }
        let index = self.next_burst;
        self.next_burst += 1;
        let first_burst = self.pending.take();
        let embedded = embedded_signalling(&burst).filter(|&(data, _)| {
            fragment_matches(index, data) && self.color.is_none_or(|color| color == data >> 3)
        });
        let Some((embedded, _)) = embedded else {
            // Keep the playback clock running while waiting for the next
            // validated burst of this call.
            return Ok(if self.color.is_some() {
                vec![0; BURST_BYTES]
            } else {
                Vec::new()
            });
        };
        if embedded & 0b100 != 0 {
            self.private = true;
            return Ok(Vec::new());
        }
        self.color = Some(embedded >> 3);
        let now = Instant::now();
        if self
            .last_audio
            .is_some_and(|t| now - t > Duration::from_millis(190))
        {
            let state = NonNull::new(unsafe { (self.allocate)() }).ok_or(crate::tr!(
                "No se pudo reiniciar el vocodificador DMR",
                "Could not reset the DMR vocoder"
            ))?;
            unsafe { (self.free)(self.state.as_ptr()) };
            self.state = state;
        }
        self.last_audio = Some(now);
        let mut pcm = Vec::with_capacity(BURST_BYTES * (1 + usize::from(first_burst.is_some())));
        for burst in first_burst.iter().chain(std::iter::once(&burst)) {
            for mut frame in unpack(burst) {
                let mut samples = [0i16; 160];
                unsafe {
                    (self.decode)(
                        self.state.as_ptr(),
                        frame.as_mut_ptr(),
                        samples.as_mut_ptr(),
                    )
                };
                for sample in samples {
                    pcm.extend_from_slice(&((f32::from(sample) * volume) as i16).to_le_bytes());
                }
            }
        }
        Ok(pcm)
    }
}

impl Drop for Vocoder {
    fn drop(&mut self) {
        unsafe { (self.free)(self.state.as_ptr()) };
    }
}

fn module_error(error: libloading::Error) -> String {
    crate::tr_format!("Módulo de voz DMR: {error}", "DMR voice module: {error}")
}

// DMR AMBE interleave schedule from DSD (dmr_const.h).
// Copyright (C) 2010 DSD Author
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND ISC DISCLAIMS ALL WARRANTIES WITH
// REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
// AND FITNESS. IN NO EVENT SHALL ISC BE LIABLE FOR ANY SPECIAL, DIRECT,
// INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
// LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE
// OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
// PERFORMANCE OF THIS SOFTWARE.
const MSB_ROW: [usize; 36] = [
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2,
    0, 2, 0, 2,
];
const MSB_COL: [usize; 36] = [
    23, 10, 22, 9, 21, 8, 20, 7, 19, 6, 18, 5, 17, 4, 16, 3, 15, 2, 14, 1, 13, 0, 12, 10, 11, 9,
    10, 8, 9, 7, 8, 6, 7, 5, 6, 4,
];
const LSB_ROW: [usize; 36] = [
    0, 2, 0, 2, 0, 2, 0, 2, 0, 3, 0, 3, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3,
    1, 3, 1, 3,
];
const LSB_COL: [usize; 36] = [
    5, 3, 4, 2, 3, 1, 2, 0, 1, 13, 0, 12, 22, 11, 21, 10, 20, 9, 19, 8, 18, 7, 17, 6, 16, 5, 15, 4,
    14, 3, 13, 2, 12, 1, 11, 0,
];

fn unpack(burst: &VoiceBurst) -> [Frame; 3] {
    let mut frames = [[[0; 24]; 4]; 3];
    for (index, dibit) in burst[..54].iter().chain(&burst[78..]).copied().enumerate() {
        let frame = &mut frames[index / 36];
        let i = index % 36;
        frame[MSB_ROW[i]][MSB_COL[i]] = (dibit >> 1) as c_char;
        frame[LSB_ROW[i]][LSB_COL[i]] = (dibit & 1) as c_char;
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dmr_sync::PATTERNS,
        dsp::{DigitalEvent, Dsp},
        protocol::word_dibits,
    };

    // Synthetic AMBE tone frame, with the two Golay words and C1 whitening.
    const TONE: &[u8; 36] = b"303030303032222030203020303020203030";

    fn radio_bursts(privacy: bool) -> Vec<(VoiceBurst, bool)> {
        let mut symbols = vec![0; 12];
        let payload: Vec<_> = TONE.iter().cycle().take(108).map(|&n| n - b'0').collect();
        for index in 0..6 {
            let center = if index == 0 {
                word_dibits(PATTERNS[0].0, 24)
            } else {
                let emb = [0, 0x0273, 0x0696, 0x0696, 0x04e5, 0][index]
                    ^ if privacy { 0x09c9 } else { 0 };
                let mut field = word_dibits(emb >> 8, 4);
                field.extend([0; 16]);
                field.extend(word_dibits(emb & 255, 4));
                field
            };
            symbols.extend_from_slice(&payload[..54]);
            symbols.extend(center);
            symbols.extend_from_slice(&payload[54..]);
            // The other slot carries data with its own sync.
            symbols.extend([0; 66]);
            symbols.extend(word_dibits(PATTERNS[1].0, 24));
            symbols.extend([0; 66]);
        }
        let mut phase = 0.0_f64;
        let rate = 240_000;
        let baud = 4800.0 * 1.000_05;
        let iq: Vec<_> = (0..(symbols.len() as f64 * rate as f64 / baud) as usize)
            .flat_map(|n| {
                let t = n as f64 * baud / f64::from(rate) - 0.47;
                let i = t.floor() as usize;
                let levels = [648.0, 1944.0, -648.0, -1944.0];
                let current = levels[symbols[i] as usize];
                let previous = levels[symbols[i.saturating_sub(1)] as usize];
                let hz = 900.0
                    + previous
                    + (current - previous) * ((t - t.floor()) * 5.0).clamp(0.0, 1.0);
                phase += std::f64::consts::TAU * hz / f64::from(rate);
                [
                    (127.5 + 82.0 * phase.cos()) as u8,
                    (127.5 + 82.0 * phase.sin()) as u8,
                ]
            })
            .collect();
        let mut bursts = Vec::new();
        let mut dsp = Dsp::new(rate);
        for block in iq.chunks(770) {
            dsp.process(block, |event| {
                if let DigitalEvent::DmrVoice(burst, first) = event {
                    bursts.push((burst, first));
                }
            });
        }
        bursts
    }

    #[test]
    fn dmr_voice_retains_the_selected_slot_and_reassembles_ambe() {
        let bursts = radio_bursts(false);
        assert_eq!(bursts.len(), 6);
        assert!(bursts[0].1);
        let mut expected = [[0; 24]; 4];
        for (i, &symbol) in TONE.iter().enumerate() {
            expected[MSB_ROW[i]][MSB_COL[i]] = ((symbol - b'0') >> 1) as c_char;
            expected[LSB_ROW[i]][LSB_COL[i]] = ((symbol - b'0') & 1) as c_char;
        }
        for (index, (burst, first)) in bursts.iter().enumerate() {
            assert_eq!(*first, index == 0);
            assert_eq!(unpack(burst), [expected; 3], "burst {index}");
            if !*first {
                assert_eq!(
                    embedded_signalling(burst).map(|(data, _)| data),
                    Some([0, 1, 3, 3, 2, 0][index])
                );
            }
        }
    }

    #[test]
    #[ignore = "requiere compilar el módulo de voz con native/build-dmr-audio.sh"]
    fn dmr_iq_to_pcm_reproduces_clear_audio_and_suppresses_private_bursts() {
        for (private, volume) in [(false, 1.0), (false, 0.0), (true, 1.0)] {
            let mut vocoder = Vocoder::open().unwrap();
            let pcm: Vec<_> = radio_bursts(private)
                .into_iter()
                .flat_map(|(burst, first)| vocoder.process(burst, first, volume).unwrap())
                .collect();
            if private {
                assert!(pcm.is_empty());
            } else {
                assert_eq!(pcm.len(), 6 * 3 * 160 * 2);
                let samples: Vec<_> = pcm
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(i16::from_le_bytes)
                    .collect();
                if volume == 0.0 {
                    assert!(samples.iter().all(|&sample| sample == 0));
                } else {
                    assert!(samples.iter().any(|&sample| sample > 4000));
                    let crossings = samples.windows(2).filter(|p| p[0] <= 0 && p[1] > 0).count();
                    assert!(
                        (355..=365).contains(&crossings),
                        "expected 1 kHz: {crossings}"
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "requiere compilar el módulo de voz con native/build-dmr-audio.sh"]
    fn dmr_voice_resumes_after_a_burst_with_invalid_signalling() {
        let mut vocoder = Vocoder::open().unwrap();
        for (index, (mut burst, first)) in radio_bursts(false).into_iter().enumerate() {
            if index == 2 {
                // A valid QR word from a different colour code must not
                // interrupt playback of the following valid burst.
                let embedded = 0x804f ^ 0x0696;
                burst[54..58].copy_from_slice(&word_dibits(embedded >> 8, 4));
                burst[74..78].copy_from_slice(&word_dibits(embedded & 255, 4));
            }
            let pcm = vocoder.process(burst, first, 1.0).unwrap();
            if index == 2 {
                assert_eq!(pcm, vec![0; BURST_BYTES]);
            } else if index > 0 {
                assert!(pcm.iter().any(|&byte| byte != 0), "burst {index}");
            }
        }
    }
}
