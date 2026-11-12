// SPDX-License-Identifier: GPL-2.0-only

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Spanish,
    English,
}

impl Language {
    pub fn current() -> Self {
        if ENGLISH.load(Ordering::Relaxed) {
            Self::English
        } else {
            Self::Spanish
        }
    }

    pub fn set(self) {
        ENGLISH.store(self == Self::English, Ordering::Relaxed);
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Spanish => "es",
            Self::English => "en",
        }
    }
}

#[macro_export]
macro_rules! tr {
    ($es:literal, $en:literal) => {
        match $crate::i18n::Language::current() {
            $crate::i18n::Language::Spanish => $es,
            $crate::i18n::Language::English => $en,
        }
    };
}

#[macro_export]
macro_rules! tr_format {
    ($es:literal, $en:literal $(, $args:expr)* $(,)?) => {
        match $crate::i18n::Language::current() {
            $crate::i18n::Language::Spanish => format!($es $(, $args)*),
            $crate::i18n::Language::English => format!($en $(, $args)*),
        }
    };
}

pub fn label(spanish: &str) -> &str {
    match Language::current() {
        Language::Spanish => spanish,
        Language::English => english_label(spanish),
    }
}

pub fn english_label(spanish: &str) -> &str {
    match spanish {
        "Onda larga" => "Longwave",
        "AM · Onda media" => "AM · Medium wave",
        "MF alta" => "Upper MF",
        "HF baja" => "Lower HF",
        "HF alta" => "Upper HF",
        "60 m · Segmento alto" => "60 m · Upper segment",
        "VHF baja" => "Lower VHF",
        "6 m · Aficionados" => "6 m · Amateur",
        "FM comercial" => "FM broadcast",
        "Navegación aérea" => "Air navigation",
        "Aeronáutica civil" => "Civil aviation",
        "2 m · Aficionados" => "2 m · Amateur",
        "VHF marina" => "Marine VHF",
        "VHF alta" => "Upper VHF",
        "UHF baja" => "Lower UHF",
        "70 cm · Aficionados" => "70 cm · Amateur",
        "UHF hasta 1 GHz" => "UHF up to 1 GHz",
        "ONDA CORTA" => "SHORTWAVE",
        "Conectando…" => "Connecting…",
        "Desconectado" => "Disconnected",
        "Deteniendo…" => "Stopping…",
        "Recibiendo" => "Receiving",
        "Falló el hilo receptor" => "Receiver thread failed",
        "Fin de la entrada IQ" => "End of IQ input",
        "Estación base · voz" => "Base station · voice",
        "Estación base · datos" => "Base station · data",
        "Móvil · voz" => "Mobile · voice",
        "Móvil · datos" => "Mobile · data",
        "Directo TS1 · voz" => "Direct TS1 · voice",
        "Directo TS1 · datos" => "Direct TS1 · data",
        "Directo TS2 · voz" => "Direct TS2 · voice",
        "Directo TS2 · datos" => "Direct TS2 · data",
        _ => spanish,
    }
}
