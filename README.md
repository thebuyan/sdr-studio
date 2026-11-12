# SDR Studio

SDR Studio is an experimental Linux desktop receiver for RTL-SDR, written in Rust with egui. The interface is available in English and Spanish. [Guía en español](README.es.md)

## Features

- RTL-SDR over USB, `rtl_tcp`, headerless interleaved CU8 IQ files, and a synthetic IQ demo.
- FFT spectrum and continuous waterfall with click-to-tune, centered and fixed views, and up to 3.2 MHz of display bandwidth at ×1 zoom. Usable bandwidth depends on hardware and USB throughput.
- AM, NFM, mono WFM, USB, LSB, DSB and CW audio. RAW sends filtered I and Q to separate stereo channels at 48 kHz.
- Range scanning, band presets, live RF gain control and external upconverter offsets.
- Experimental DMR synchronization and conventional unencrypted voice with the optional mbelib module, one timeslot at a time.
- P25 Phase 1 synchronization and NID decoding, including NAC and DUID. P25 voice, trunking and full digital data decoding are pending.

## Build and run

Install Rust 1.88 or newer and the development libraries for an X11 or Wayland desktop with OpenGL. On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config librtlsdr0 rtl-sdr curl \
  pulseaudio-utils libx11-dev libxi-dev libxcursor-dev libxrandr-dev \
  libxkbcommon-dev libgl1-mesa-dev libegl1-mesa-dev libwayland-dev
cargo build --release --locked
./target/release/sdr-studio --demo
```

The demo runs without radio hardware. To use a dongle, launch without `--demo`, select **RTL-SDR · USB**, and start the receiver. The system needs a compatible `librtlsdr` and USB permissions for the device. Audio playback uses `paplay` with PulseAudio or PipeWire's PulseAudio compatibility layer.

Use **Idioma / Language** at the top of the left panel to select **English**. The application remembers the language and receiver settings across sessions. Press the power button to resume reception after reopening it.

## Optional DMR voice

Build the native module against [the mbelib revision used in testing](https://github.com/lwvmobile/mbelib/tree/30dc79074ca022366a27d705b8023011d9600339). Select **Digital**, tune to a conventional unencrypted DMR carrier and start the receiver.

```sh
./native/build-dmr-audio.sh /path/to/mbelib
SDR_STUDIO_DMR_LIBRARY="$PWD/bin/libsdstudio-dmr.so" ./target/release/sdr-studio
```

## Checks

```sh
cargo run --locked -- --self-check
cargo test --locked
```

The [local validation notes in Spanish](VALIDATION.md) document USB reception on an RTL-SDR Blog V4 and the limits of the digital decoder.

## License

Copyright © 2026 SDR Studio contributors. Project source is licensed under [GPL-2.0-only](LICENSE). Dependencies retain their own licenses. Some linked dependencies use Apache-2.0, which the Free Software Foundation lists as incompatible with GPLv2. This publication includes project source only.
