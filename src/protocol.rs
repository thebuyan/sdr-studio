// SPDX-License-Identifier: GPL-2.0-only

//! Native bit-level acquisition. No vocoder or complete frame FEC yet.
use crate::p25_nid;
use std::collections::VecDeque;

pub const P25_SYNC: u64 = 0x5575_f5ff_77ff;
const MASK: u64 = (1 << 48) - 1;

#[derive(Clone, Debug)]
pub struct Event {
    pub protocol: &'static str,
    pub text: String,
    english: Option<String>,
    pub errors: u32,
}

impl Event {
    pub fn display_text(&self) -> &str {
        match crate::i18n::Language::current() {
            crate::i18n::Language::Spanish => &self.text,
            crate::i18n::Language::English => self.english.as_deref().unwrap_or(&self.text),
        }
    }
}

#[derive(Default)]
pub struct Decoder {
    word: u64,
    bits: usize,
    pending_p25: Option<(Vec<u8>, usize, bool)>,
    pub events: VecDeque<Event>,
    pub hits: u64,
}

impl Decoder {
    fn event(
        &mut self,
        protocol: &'static str,
        text: String,
        english: Option<String>,
        errors: u32,
    ) {
        self.hits += 1;
        if self.events.len() == 200 {
            self.events.pop_front();
        }
        self.events.push_back(Event {
            protocol,
            text,
            english,
            errors,
        });
    }

    pub fn dmr_sync(&mut self, sync: crate::dmr_sync::Sync) {
        self.event(
            "DMR",
            format!(
                "{} · sincronismo · desvío RF {:+.0} Hz",
                sync.label, sync.offset_hz
            ),
            Some(format!(
                "{} · sync · RF offset {:+.0} Hz",
                crate::i18n::english_label(sync.label),
                sync.offset_hz
            )),
            sync.errors,
        );
    }

    pub fn push(&mut self, dibit: u8) {
        // First P25 status symbol is at dibit 35 from frame start (after 11 NID dibits).
        if let Some((nid, position, inverted)) = &mut self.pending_p25 {
            let d = dibit ^ if *inverted { 2 } else { 0 };
            if *position != 11 {
                nid.extend([d >> 1, d & 1]);
            }
            *position += 1;
            if nid.len() == 64 {
                let raw = nid.iter().fold(0u64, |a, b| (a << 1) | *b as u64);
                self.pending_p25 = None;
                if let Some((info, corrected)) = p25_nid::decode(raw) {
                    let nac = info >> 4;
                    let duid = (info & 15) as u8;
                    let kind = p25_nid::duid_name(duid).unwrap();
                    self.event(
                        "P25",
                        format!("NID: NAC {nac:03X} · DUID {duid:X} ({kind})"),
                        None,
                        corrected,
                    );
                }
            }
        }
        self.word = ((self.word << 2) | u64::from(dibit & 3)) & MASK;
        self.bits = (self.bits + 2).min(48);
        if self.bits < 48 {
            return;
        }
        // Exactly one erroneous bit permitted, no speculative identification of payloads.
        for inverted in [false, true] {
            let word = self.word ^ if inverted { 0xaaaa_aaaa_aaaa } else { 0 };
            let errors = (word ^ P25_SYNC).count_ones();
            if errors <= 1 {
                self.event(
                    "P25",
                    format!(
                        "Sincronismo Phase 1 · polaridad {}",
                        if inverted { "invertida" } else { "normal" }
                    ),
                    Some(format!(
                        "Phase 1 sync · {} polarity",
                        if inverted { "inverted" } else { "normal" }
                    )),
                    errors,
                );
                self.pending_p25 = Some((Vec::with_capacity(64), 0, inverted));
                return;
            }
        }
    }
}

pub fn word_dibits(word: u64, count: usize) -> Vec<u8> {
    (0..count)
        .rev()
        .map(|n| ((word >> (2 * n)) & 3) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn p25_sync_and_inverted_polarities() {
        let sync = P25_SYNC;
        {
            for polarity in [0, 2] {
                let mut decoder = Decoder::default();
                for d in word_dibits(sync ^ 1, 24) {
                    decoder.push(d ^ polarity);
                }
                assert_eq!(decoder.events.len(), 1);
                assert_eq!(decoder.events[0].errors, 1);
            }
        }
    }
    #[test]
    fn nid_skips_status_and_waits_for_complete_field() {
        for polarity in [0, 2] {
            let mut decoder = Decoder::default();
            for d in word_dibits(P25_SYNC, 24) {
                decoder.push(d ^ polarity);
            }
            let nid = word_dibits(0x2935_55ef_2c65_3437 ^ (1 << 60), 32);
            for (i, d) in nid.into_iter().enumerate() {
                if i == 11 {
                    decoder.push(3 ^ polarity);
                }
                assert_eq!(decoder.events.len(), 1);
                decoder.push(d ^ polarity);
            }
            let event = decoder.events.back().unwrap();
            assert!(event.text.contains("NAC 293 · DUID 5"));
            assert_eq!(event.errors, 1);
        }
    }
    #[test]
    fn silence_is_not_sync() {
        let mut decoder = Decoder::default();
        for _ in 0..10000 {
            decoder.push(0);
        }
        assert_eq!(decoder.hits, 0);
    }
}
