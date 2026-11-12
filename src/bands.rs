// SPDX-License-Identifier: GPL-2.0-only

use crate::audio::Mode;

#[derive(Clone, Copy)]
pub struct Preset {
    pub mode: Mode,
    pub step_hz: u32,
    pub filter_hz: u32,
    pub span_hz: u32,
    pub sample_rate: u32,
}

const AM_LF: Preset = Preset {
    mode: Mode::Am,
    step_hz: 1000,
    filter_hz: 6000,
    span_hz: 60_000,
    sample_rate: 240_000,
};
const AM_MW: Preset = Preset {
    step_hz: 10_000,
    filter_hz: 8000,
    span_hz: 120_000,
    ..AM_LF
};
const AM_HF: Preset = Preset {
    step_hz: 5000,
    filter_hz: 8000,
    span_hz: 120_000,
    ..AM_LF
};
const AM_TROPICAL: Preset = Preset {
    filter_hz: 6000,
    ..AM_HF
};
const AM_AIR: Preset = Preset {
    step_hz: 25_000,
    filter_hz: 10_000,
    span_hz: 240_000,
    ..AM_LF
};
const NFM: Preset = Preset {
    mode: Mode::Nfm,
    step_hz: 12_500,
    filter_hz: 12_500,
    span_hz: 240_000,
    sample_rate: 240_000,
};
const NFM_MARINE: Preset = Preset {
    step_hz: 25_000,
    filter_hz: 16_000,
    ..NFM
};
const WFM: Preset = Preset {
    mode: Mode::Wfm,
    step_hz: 100_000,
    filter_hz: 180_000,
    span_hz: 480_000,
    sample_rate: 960_000,
};
const DIGITAL: Preset = Preset {
    mode: Mode::Digital,
    step_hz: 25_000,
    filter_hz: 12_500,
    span_hz: 960_000,
    sample_rate: 960_000,
};

pub struct Band {
    pub label: &'static str,
    pub low_khz: u32,
    pub high_khz: u32,
    pub preset: Preset,
}

impl Band {
    const fn new(label: &'static str, low_khz: u32, high_khz: u32, preset: Preset) -> Self {
        Self {
            label,
            low_khz,
            high_khz,
            preset,
        }
    }

    pub fn center_hz(&self) -> u32 {
        let middle = (self.low_khz + self.high_khz) * 500;
        let step = self.preset.step_hz;
        (middle + step / 2) / step * step
    }

    pub fn contains(&self, hz: u32) -> bool {
        (self.low_khz * 1000..=self.high_khz * 1000).contains(&hz)
    }

    pub fn range_label(&self) -> String {
        fn mhz(khz: u32) -> String {
            format!("{:.3}", f64::from(khz) / 1000.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        }
        format!("{}–{} MHz", mhz(self.low_khz), mhz(self.high_khz))
    }
}

const LOWER: &[Band] = &[
    Band::new("Onda larga", 100, 530, AM_LF),
    Band::new("AM · Onda media", 530, 1710, AM_MW),
    Band::new("MF alta", 1710, 3000, AM_TROPICAL),
    Band::new("HF baja", 3000, 5000, AM_HF),
    Band::new("HF 5–8", 5000, 8000, AM_HF),
    Band::new("HF 8–12", 8000, 12000, AM_HF),
    Band::new("HF 12–18", 12000, 18000, AM_HF),
    Band::new("HF 18–24", 18000, 24000, AM_HF),
    Band::new("HF alta", 24000, 30000, AM_HF),
];

const SHORTWAVE: &[Band] = &[
    Band::new("120 m · Tropical", 2300, 2495, AM_TROPICAL),
    Band::new("90 m · Tropical", 3200, 3400, AM_TROPICAL),
    Band::new("75 m · Regional", 3900, 4000, AM_TROPICAL),
    Band::new("60 m · Tropical", 4750, 4995, AM_TROPICAL),
    Band::new("60 m · Segmento alto", 5005, 5060, AM_TROPICAL),
    Band::new("49 m", 5900, 6200, AM_HF),
    Band::new("41 m", 7300, 7400, AM_HF),
    Band::new("31 m", 9400, 9900, AM_HF),
    Band::new("25 m", 11600, 12100, AM_HF),
    Band::new("22 m", 13570, 13870, AM_HF),
    Band::new("19 m", 15100, 15800, AM_HF),
    Band::new("16 m", 17480, 17900, AM_HF),
    Band::new("15 m", 18900, 19020, AM_HF),
    Band::new("13 m", 21450, 21850, AM_HF),
    Band::new("11 m", 25670, 26100, AM_HF),
];

const VHF: &[Band] = &[
    Band::new("VHF baja", 30000, 50000, NFM),
    Band::new("6 m · Aficionados", 50000, 54000, NFM),
    Band::new("VHF 54–87,5", 54000, 87500, NFM),
    Band::new("FM comercial", 87500, 108000, WFM),
    Band::new("Navegación aérea", 108000, 118000, DIGITAL),
    Band::new("Aeronáutica civil", 118000, 137000, AM_AIR),
    Band::new("VHF 137–144", 137000, 144000, NFM),
    Band::new("2 m · Aficionados", 144000, 148000, NFM),
    Band::new("VHF 148–156", 148000, 156000, NFM),
    Band::new("VHF marina", 156000, 162000, NFM_MARINE),
    Band::new("VHF 162–174", 162000, 174000, NFM),
    Band::new("VHF 174–230", 174000, 230000, DIGITAL),
    Band::new("VHF alta", 230000, 300000, AM_AIR),
];

const UHF: &[Band] = &[
    Band::new("UHF baja", 300000, 400000, AM_AIR),
    Band::new("UHF 400–420", 400000, 420000, NFM),
    Band::new("70 cm · Aficionados", 420000, 450000, NFM),
    Band::new("UHF 450–470", 450000, 470000, NFM),
    Band::new("UHF 470–698", 470000, 698000, DIGITAL),
    Band::new("UHF 698–806", 698000, 806000, DIGITAL),
    Band::new("UHF 806–902", 806000, 902000, DIGITAL),
    Band::new("UHF 902–928", 902000, 928000, DIGITAL),
    Band::new("UHF hasta 1 GHz", 928000, 1000000, DIGITAL),
];

pub const GROUPS: [(&str, &[Band]); 4] = [
    ("LF · MF · HF", LOWER),
    ("ONDA CORTA", SHORTWAVE),
    ("VHF", VHF),
    ("UHF", UHF),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_cover_through_one_ghz_and_centers_follow_the_preset_step() {
        let mut end = 100;
        for band in LOWER.iter().chain(VHF).chain(UHF) {
            assert_eq!(band.low_khz, end);
            end = band.high_khz;
        }
        assert_eq!(end, 1_000_000);
        for (_, bands) in GROUPS {
            for band in bands {
                assert!(band.contains(band.center_hz()));
                assert_eq!(band.center_hz() % band.preset.step_hz, 0);
                assert!(band.preset.span_hz <= band.preset.sample_rate);
            }
        }
        assert_eq!(VHF[3].preset.mode, Mode::Wfm);
        assert_eq!(VHF[5].preset.mode, Mode::Am);
    }
}
