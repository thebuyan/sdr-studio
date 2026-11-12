// SPDX-License-Identifier: GPL-2.0-only

use crate::protocol::{word_dibits, P25_SYNC};
use libloading::Library;
use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SourceKind {
    Usb,
    Tcp,
    File,
    Demo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Converter {
    #[default]
    Direct,
    HamItUpNano,
    Custom(i64),
}

impl Converter {
    pub const NANO_LO: i64 = 125_000_000;
    const NANO_RANGE: std::ops::RangeInclusive<u32> = 100_000..=65_000_000;

    fn tuner_frequency(self, rf_hz: u32) -> Result<u32, String> {
        let shift = match self {
            Self::Direct => 0,
            Self::HamItUpNano => {
                if !Self::NANO_RANGE.contains(&rf_hz) {
                    return Err(crate::tr!(
                        "Ham It Up Nano: frecuencia RF fuera de 0,100–65 MHz",
                        "Ham It Up Nano: RF frequency outside 0.100–65 MHz"
                    )
                    .into());
                }
                Self::NANO_LO
            }
            Self::Custom(shift) => shift,
        };
        i64::from(rf_hz)
            .checked_add(shift)
            .and_then(|frequency| u32::try_from(frequency).ok())
            .filter(|&frequency| frequency > 0)
            .ok_or_else(|| {
                crate::tr!(
                    "El desplazamiento deja la frecuencia del SDR fuera de rango",
                    "The offset puts the SDR frequency out of range"
                )
                .into()
            })
    }
}

#[derive(Clone)]
pub struct Config {
    pub kind: SourceKind,
    pub frequency: u32,
    pub sample_rate: u32,
    pub gain: Option<i32>,
    pub ppm: i32,
    pub device: u32,
    pub address: String,
    pub path: String,
    pub converter: Converter,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            kind: SourceKind::Demo,
            frequency: 446_000_000,
            sample_rate: 240_000,
            gain: None,
            ppm: 0,
            device: 0,
            address: "127.0.0.1:1234".into(),
            path: String::new(),
            converter: Converter::Direct,
        }
    }
}

pub fn sample_rate_error() -> &'static str {
    crate::tr!(
        "Tasa válida: 225001–300000 o 900001–3200000 S/s",
        "Valid rates: 225001–300000 or 900001–3200000 S/s"
    )
}

pub fn valid_sample_rate(rate: u32) -> bool {
    (225_001..=300_000).contains(&rate) || (900_001..=3_200_000).contains(&rate)
}

impl Config {
    pub fn scan_limits(&self, first: u32, last: u32) -> Option<(u32, u32)> {
        let (low, high) = match (self.kind, self.converter) {
            (SourceKind::Usb | SourceKind::Tcp, Converter::HamItUpNano) => {
                (*Converter::NANO_RANGE.start(), *Converter::NANO_RANGE.end())
            }
            (SourceKind::Usb | SourceKind::Tcp, Converter::Custom(shift)) => {
                let shift = i128::from(shift);
                (
                    u32::try_from((1 - shift).max(1)).ok()?,
                    u32::try_from((i128::from(u32::MAX) - shift).min(i128::from(u32::MAX))).ok()?,
                )
            }
            _ => (1, u32::MAX),
        };
        let first = first.max(low);
        let last = last.min(high);
        (first <= last).then_some((first, last))
    }

    pub fn tuner_frequency(&self, rf_hz: u32) -> Result<u32, String> {
        match self.kind {
            SourceKind::Usb | SourceKind::Tcp => self.converter.tuner_frequency(rf_hz),
            SourceKind::File | SourceKind::Demo => Ok(rf_hz),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !valid_sample_rate(self.sample_rate) {
            return Err(sample_rate_error().into());
        }
        if self.frequency == 0 {
            return Err(crate::tr!(
                "La frecuencia debe ser mayor que cero",
                "Frequency must be greater than zero"
            )
            .into());
        }
        self.tuner_frequency(self.frequency)?;
        if !(-1000..=1000).contains(&self.ppm) {
            return Err(crate::tr!(
                "PPM fuera de rango (-1000 a 1000)",
                "PPM out of range (-1000 to 1000)"
            )
            .into());
        }
        if self.gain.is_some_and(|g| !(-100..=600).contains(&g)) {
            return Err(crate::tr!(
                "Ganancia fuera de rango (-10 a 60 dB)",
                "Gain out of range (-10 to 60 dB)"
            )
            .into());
        }
        if self.kind == SourceKind::File && self.path.is_empty() {
            return Err(crate::tr!(
                "Selecciona la ruta del archivo IQ CU8",
                "Choose the path to a CU8 IQ file"
            )
            .into());
        }
        Ok(())
    }
}

pub trait SampleSource: Send {
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, String>;
    fn set_gain(&mut self, _previous: Option<i32>, _gain: Option<i32>) -> Result<(), String> {
        Err(crate::tr!(
            "La entrada no tiene control de ganancia RF",
            "This input has no RF gain control"
        )
        .into())
    }
    fn tune(&mut self, _frequency: u32) -> Result<(), String> {
        Err(crate::tr!(
            "La entrada no permite sintonizar",
            "This input cannot be tuned"
        )
        .into())
    }
}

pub fn open(config: &Config) -> Result<Box<dyn SampleSource>, String> {
    config.validate()?;
    let tuner_hz = config.tuner_frequency(config.frequency)?;
    match config.kind {
        SourceKind::Usb => Ok(Box::new(RtlUsb::open(config, tuner_hz)?)),
        SourceKind::Tcp => Ok(Box::new(RtlTcp::open(config, tuner_hz)?)),
        SourceKind::File => Ok(Box::new(IqFile(
            File::open(&config.path).map_err(|e| e.to_string())?,
        ))),
        SourceKind::Demo => Ok(Box::new(Demo::new(config.sample_rate))),
    }
}

struct IqFile(File);
impl SampleSource for IqFile {
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, String> {
        self.0.read(bytes).map_err(|e| e.to_string())
    }
}

struct RtlTcp(TcpStream);
impl RtlTcp {
    fn open(c: &Config, tuner_hz: u32) -> Result<Self, String> {
        let addresses = c.address.to_socket_addrs().map_err(|e| e.to_string())?;
        let mut connection = None;
        for address in addresses.take(8) {
            if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
                connection = Some(stream);
                break;
            }
        }
        let mut stream = connection.ok_or(crate::tr!(
            "No se pudo conectar a rtl_tcp",
            "Could not connect to rtl_tcp"
        ))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        let mut header = [0; 12];
        stream
            .read_exact(&mut header)
            .map_err(|e| crate::tr_format!("Cabecera rtl_tcp: {e}", "rtl_tcp header: {e}"))?;
        if &header[..4] != b"RTL0" {
            return Err(crate::tr!(
                "El servidor no responde con el protocolo rtl_tcp",
                "The server did not respond with the rtl_tcp protocol"
            )
            .into());
        }
        for command in commands(c, tuner_hz) {
            stream.write_all(&command).map_err(|e| e.to_string())?;
        }
        Ok(Self(stream))
    }
}
impl SampleSource for RtlTcp {
    fn tune(&mut self, frequency: u32) -> Result<(), String> {
        self.0
            .write_all(&command(1, frequency))
            .map_err(|e| e.to_string())
    }

    fn set_gain(&mut self, previous: Option<i32>, gain: Option<i32>) -> Result<(), String> {
        if previous.is_some() != gain.is_some() {
            self.0
                .write_all(&command(3, u32::from(gain.is_some())))
                .map_err(|e| e.to_string())?;
        }
        if let Some(gain) = gain {
            self.0
                .write_all(&command(4, gain as u32))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, String> {
        match self.0.read(bytes) {
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                Err("timeout".into())
            }
            other => other.map_err(|e| e.to_string()),
        }
    }
}
fn command(code: u8, value: u32) -> [u8; 5] {
    let mut bytes = [code, 0, 0, 0, 0];
    bytes[1..].copy_from_slice(&value.to_be_bytes());
    bytes
}

fn commands(c: &Config, tuner_hz: u32) -> Vec<[u8; 5]> {
    let values = [
        (2, c.sample_rate),
        (1, tuner_hz),
        (5, c.ppm as u32),
        (3, u32::from(c.gain.is_some())),
    ];
    let mut commands: Vec<_> = values
        .into_iter()
        .map(|(code, value)| command(code, value))
        .collect();
    if let Some(gain) = c.gain {
        commands.push(command(4, gain as u32));
    }
    commands
}

type Close = unsafe extern "C" fn(*mut c_void) -> i32;
type ReadSync = unsafe extern "C" fn(*mut c_void, *mut c_void, i32, *mut i32) -> i32;
struct RtlUsb {
    _library: Library,
    device: *mut c_void,
    close: Close,
    read_sync: ReadSync,
}
// Device is created, used and dropped solely by the receiver worker thread.
unsafe impl Send for RtlUsb {}
impl RtlUsb {
    fn open(c: &Config, tuner_hz: u32) -> Result<Self, String> {
        unsafe {
            let library = Library::new("librtlsdr.so.2")
                .or_else(|_| Library::new("librtlsdr.so.0"))
                .map_err(|e| {
                    crate::tr_format!(
                        "Instala librtlsdr y sus reglas udev: {e}",
                        "Install librtlsdr and its udev rules: {e}"
                    )
                })?;
            let open = *library
                .get::<unsafe extern "C" fn(*mut *mut c_void, u32) -> i32>(b"rtlsdr_open\0")
                .map_err(|e| e.to_string())?;
            let close = *library
                .get::<Close>(b"rtlsdr_close\0")
                .map_err(|e| e.to_string())?;
            let read_sync = *library
                .get::<ReadSync>(b"rtlsdr_read_sync\0")
                .map_err(|e| e.to_string())?;
            let mut device = std::ptr::null_mut();
            if open(&mut device, c.device) != 0 || device.is_null() {
                return Err(
                    crate::tr!("No se pudo abrir RTL-SDR: revisa índice, permisos USB y controlador DVB", "Could not open RTL-SDR. Check the device index, USB permissions and DVB driver.")
                        .into(),
                );
            }
            let receiver = Self {
                _library: library,
                device,
                close,
                read_sync,
            };
            receiver.set_u32(b"rtlsdr_set_sample_rate\0", c.sample_rate)?;
            receiver.set_u32(b"rtlsdr_set_center_freq\0", tuner_hz)?;
            receiver.set_i32(b"rtlsdr_set_freq_correction\0", c.ppm)?;
            receiver.set_i32(b"rtlsdr_set_tuner_gain_mode\0", i32::from(c.gain.is_some()))?;
            if let Some(gain) = c.gain {
                receiver.set_i32(b"rtlsdr_set_tuner_gain\0", gain)?;
            }
            let reset = receiver
                ._library
                .get::<unsafe extern "C" fn(*mut c_void) -> i32>(b"rtlsdr_reset_buffer\0")
                .map_err(|e| e.to_string())?;
            if reset(device) < 0 {
                return Err(crate::tr!(
                    "No se pudo reiniciar el búfer USB",
                    "Could not reset the USB buffer"
                )
                .into());
            }
            Ok(receiver)
        }
    }
    unsafe fn set_u32(&self, name: &[u8], value: u32) -> Result<(), String> {
        let function = self
            ._library
            .get::<unsafe extern "C" fn(*mut c_void, u32) -> i32>(name)
            .map_err(|e| e.to_string())?;
        if function(self.device, value) < 0 {
            return Err(crate::tr_format!(
                "Falló {} ({value})",
                "{} failed ({value})",
                String::from_utf8_lossy(name)
            ));
        }
        Ok(())
    }
    unsafe fn set_i32(&self, name: &[u8], value: i32) -> Result<(), String> {
        let function = self
            ._library
            .get::<unsafe extern "C" fn(*mut c_void, i32) -> i32>(name)
            .map_err(|e| e.to_string())?;
        let result = function(self.device, value);
        // librtlsdr returns -2 when the requested PPM correction is already active.
        if result < 0 && !(name == b"rtlsdr_set_freq_correction\0" && result == -2) {
            return Err(crate::tr_format!(
                "Falló {} ({value})",
                "{} failed ({value})",
                String::from_utf8_lossy(name)
            ));
        }
        Ok(())
    }
}
impl SampleSource for RtlUsb {
    fn tune(&mut self, frequency: u32) -> Result<(), String> {
        unsafe { self.set_u32(b"rtlsdr_set_center_freq\0", frequency) }
    }

    fn set_gain(&mut self, previous: Option<i32>, gain: Option<i32>) -> Result<(), String> {
        const MODE: &[u8] = b"rtlsdr_set_tuner_gain_mode\0";
        const GAIN: &[u8] = b"rtlsdr_set_tuner_gain\0";
        unsafe {
            if previous.is_some() != gain.is_some() {
                self.set_i32(MODE, i32::from(gain.is_some()))?;
            }
            if let Some(gain) = gain {
                if let Err(error) = self.set_i32(GAIN, gain) {
                    if previous.is_none() {
                        self.set_i32(MODE, 0)
                            .map_err(|rollback| format!("{error}; {rollback}"))?;
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, String> {
        let mut count = 0;
        let result = unsafe {
            (self.read_sync)(
                self.device,
                bytes.as_mut_ptr().cast(),
                bytes.len() as i32,
                &mut count,
            )
        };
        if result < 0 || count < 0 || count as usize > bytes.len() {
            return Err(crate::tr_format!(
                "Error de lectura USB: {result}",
                "USB read error: {result}"
            ));
        }
        Ok(count as usize)
    }
}
impl Drop for RtlUsb {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.device);
        }
    }
}

pub struct Demo {
    rate: u32,
    phase: f32,
    sample: u64,
    dibits: Vec<u8>,
}
impl Demo {
    pub fn new(rate: u32) -> Self {
        let mut dibits = vec![0; 100];
        dibits.extend(word_dibits(P25_SYNC, 24));
        let mut nid = word_dibits(crate::p25_nid::encode(0x2935), 32);
        nid.insert(11, 3);
        dibits.extend(nid);
        dibits.extend(vec![0; 100]);
        dibits.extend(word_dibits(crate::dmr_sync::PATTERNS[0].0, 24));
        dibits.extend(vec![0; 100]);
        Self {
            rate,
            phase: 0.0,
            sample: 0,
            dibits,
        }
    }
    pub fn generate(&mut self, bytes: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(bytes);
        for _ in 0..bytes / 2 {
            let index =
                ((self.sample * 4800 / u64::from(self.rate)) % self.dibits.len() as u64) as usize;
            let frequency = [600.0, 1800.0, -600.0, -1800.0][self.dibits[index] as usize];
            self.phase = (self.phase + 2.0 * std::f32::consts::PI * frequency / self.rate as f32)
                % (2.0 * std::f32::consts::PI);
            output.push((127.5 + 100.0 * self.phase.cos()) as u8);
            output.push((127.5 + 100.0 * self.phase.sin()) as u8);
            self.sample += 1;
        }
        output
    }
}
impl SampleSource for Demo {
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, String> {
        let samples = self.generate(bytes.len());
        bytes.copy_from_slice(&samples);
        Ok(bytes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tcp_commands_use_signed_big_endian_values() {
        let c = Config {
            ppm: -12,
            gain: Some(280),
            ..Config::default()
        };
        let commands = commands(&c, c.frequency);
        assert_eq!(commands[2], [5, 255, 255, 255, 244]);
        assert_eq!(commands[4], [4, 0, 0, 1, 24]);
    }
    #[test]
    fn mock_tcp_validates_header_and_configures_receiver() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"RTL0\0\0\0\0\0\0\0\0").unwrap();
            let mut commands = [0; 20];
            stream.read_exact(&mut commands).unwrap();
            assert_eq!(commands[0], 2);
            stream.write_all(&[128, 127, 129, 126]).unwrap();
            let mut tune = [0; 5];
            stream.read_exact(&mut tune).unwrap();
            assert_eq!(tune[0], 1);
            assert_eq!(
                u32::from_be_bytes(tune[1..].try_into().unwrap()),
                446_012_500
            );
            stream.write_all(&[122, 123, 124, 125]).unwrap();
        });
        let config = Config {
            address,
            ..Config::default()
        };
        let mut source = RtlTcp::open(&config, config.frequency).unwrap();
        let mut bytes = [0; 4];
        source.0.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [128, 127, 129, 126]);
        source.tune(446_012_500).unwrap();
        source.0.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [122, 123, 124, 125]);
        server.join().unwrap();
    }

    #[test]
    fn converter_mapping_checks_frequency_limits_and_signed_shifts() {
        let nano = Converter::HamItUpNano;
        assert_eq!(nano.tuner_frequency(100_000).unwrap(), 125_100_000);
        assert_eq!(nano.tuner_frequency(9_650_000).unwrap(), 134_650_000);
        assert_eq!(nano.tuner_frequency(65_000_000).unwrap(), 190_000_000);
        assert!(nano.tuner_frequency(99_999).is_err());
        assert!(nano.tuner_frequency(65_000_001).is_err());
        assert_eq!(
            Converter::Custom(-9_000_000)
                .tuner_frequency(10_000_000)
                .unwrap(),
            1_000_000
        );
        assert!(Converter::Custom(-10_000_000)
            .tuner_frequency(10_000_000)
            .is_err());
        assert!(Converter::Custom(1).tuner_frequency(u32::MAX).is_err());
        let mut config = Config {
            kind: SourceKind::Usb,
            converter: nano,
            ..Config::default()
        };
        assert_eq!(
            config.scan_limits(54_000_000, 87_500_000),
            Some((54_000_000, 65_000_000))
        );
        assert_eq!(config.scan_limits(87_500_000, 108_000_000), None);
        config.converter = Converter::Custom(-9_000_000);
        assert_eq!(
            config.scan_limits(8_000_000, 12_000_000),
            Some((9_000_001, 12_000_000))
        );
    }
}
