// SPDX-License-Identifier: GPL-2.0-only

use std::sync::OnceLock;

const GENERATOR: u64 = 0xcd93_0bdd_3b2b;
const MAX_ERRORS: u32 = 11;

pub fn duid_name(duid: u8) -> Option<&'static str> {
    Some(match duid {
        0 => "HDU",
        3 => "TDU",
        5 => "LDU1",
        7 => "TSDU",
        10 => "LDU2",
        12 => "PDU",
        15 => "TDULC",
        _ => return None,
    })
}

pub fn encode(info: u16) -> u64 {
    // Systematic BCH(63,16), with one additional bit set for LDU1 and LDU2.
    let data = u64::from(info) << 47;
    let mut remainder = data;
    for shift in (0..16).rev() {
        if remainder & (1 << (47 + shift)) != 0 {
            remainder ^= GENERATOR << shift;
        }
    }
    ((data | remainder) << 1) | u64::from(matches!(info & 0xf, 5 | 10))
}

pub fn decode(raw: u64) -> Option<(u16, u32)> {
    let info = (raw >> 48) as u16;
    if duid_name((info & 0xf) as u8).is_some() && encode(info) == raw {
        return Some((info, 0));
    }

    // The BCH minimum distance is 23, so at most one codeword can be
    // within 11 bit flips. Cache valid words for damaged headers.
    static WORDS: OnceLock<Vec<(u16, u64)>> = OnceLock::new();
    WORDS
        .get_or_init(|| {
            (0..=u16::MAX)
                .filter(|info| duid_name((info & 0xf) as u8).is_some())
                .map(|info| (info, encode(info)))
                .collect()
        })
        .iter()
        .find_map(|&(info, word)| {
            let errors = (word ^ raw).count_ones();
            (errors <= MAX_ERRORS).then_some((info, errors))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // NAC 293 and LDU1, independently encoded using OP25's generator matrix.
    const LDU1: u64 = 0x2935_55ef_2c65_3437;

    #[test]
    fn matches_reference_and_repairs_eleven_errors() {
        assert_eq!(encode(0x2935), LDU1);
        assert_eq!(decode(LDU1), Some((0x2935, 0)));
        assert_eq!(decode(LDU1 ^ 0xf001_1202_0409), Some((0x2935, 11)));
    }

    #[test]
    fn rejects_invalid_header() {
        assert_eq!(decode(encode(0x2936)), None);
        assert_eq!(decode(0xdead_beef_cafe_1234), None);
    }
}
