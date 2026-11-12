# SDR Studio — Linux / Rust

Primera versión **experimental y parcial** de un receptor extensible para RTL-SDR.
Interfaz de escritorio en español e inglés con egui, sin servidor web ni Electron.

Copyright © 2026 SDR Studio contributors. El código del proyecto se distribuye
bajo la [GNU GPL versión 2 exclusivamente](LICENSE) (`GPL-2.0-only`).

El receptor muestra actividad DMR y P25 Phase 1. Con el módulo de voz
instalado, reproduce DMR convencional sin cifrar. La voz P25 y la
decodificación completa de datos digitales siguen pendientes.

## Estado real

| Función | Estado |
| --- | --- |
| RTL-SDR USB: frecuencia, tasa, ganancia, PPM, índice | Verificado con RTL-SDR Blog V4 en Fedora 44 |
| Servidor rtl_tcp | Implementado, probado contra servidor simulado |
| Archivos IQ CU8 | Implementado, I/Q intercalados, unsigned 8 bits, sin cabecera |
| Demo IQ sintética | Incluye una cabecera P25 NID válida y sincronismos DMR. Sin voz ni tramas completas |
| Espectro FFT / cascada | Zoom ×1–×8, escala RF en MHz y sintonía por clic, con vista centrada o fija |
| Audio analógico | AM, NFM, WFM mono, USB, LSB, DSB y CW, con filtro, volumen, mute y squelch en vivo |
| Discriminador FM / decisiones 4FSK a 4800 símbolos/s | Experimental, reloj fijo, sin recuperación de temporización |
| Sincronismos DMR | Estación base, móvil y modo directo TS1/TS2. Búsqueda de fase y compensación de desajuste de frecuencia |
| P25 Phase 1: sincronismo, NAC y DUID | NID validado mediante BCH(63,16) y el bit adicional de DUID. Corrige hasta 11 bits del NID |
| DMR IDs, grupos y mensajes de datos | Pendiente |
| P25 mensajes, FEC, IDs, grupos, datos | Pendiente |
| Voz DMR AMBE+2 | Un intervalo de voz a la vez, PCM mono a 8 kHz mediante el módulo local de mbelib |
| Voz P25 Phase 1 IMBE | Pendiente |
| P25 Phase 2 / CQPSK / trunking | Pendiente |
| Plugins nativos `.so`, ABI C v1 | Implementado con ejemplo Rust y cabecera C |
| Airspy / HackRF / SDRplay / SoapySDR | Pendiente; entrada desacoplada con `SampleSource` |

## Compilar y ejecutar

Necesitas Rust estable reciente (1.88 o superior; validado aquí con 1.99.1), un
compilador C, pkg-config y un escritorio X11 o Wayland con OpenGL.
En Debian/Ubuntu, instala las bibliotecas de ejecución y desarrollo de tu distribución:

```sh
sudo apt install build-essential pkg-config librtlsdr0 rtl-sdr curl \
  libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxkbcommon-dev \
  libgl1-mesa-dev libegl1-mesa-dev libwayland-dev
cargo build --release --locked
./target/release/sdr-studio
```

Usa **Idioma / Language** en la parte superior del panel izquierdo para
alternar entre **Español** y **English** sin reiniciar. La elección se guarda
con los ajustes de la radio.

Selecciona **RTL-SDR · USB** y pulsa **Encender**. La tasa inicial de 240000 S/s
reduce la carga. La aplicación abre con el modo **WFM** y 100 MHz como punto de
partida. Haz clic en el visor para escribir una frecuencia y confirma con **Enter**.
También puedes arrastrar el dial, girar la rueda sobre él, usar **− / +** con
el paso seleccionado o hacer clic en el espectro o la cascada mientras recibes.
Sobre un dígito del visor, la rueda ajusta en pasos según la posición que
se resalta. El último dígito sintoniza en pasos de 1 Hz y el anterior en
pasos de 10 Hz. Haz clic derecho sobre un dígito para ponerlo en cero.
El dial conserva el paso de sintonía elegido.
El medidor de aguja muestra el nivel relativo de señal.

Con el receptor apagado, puedes seleccionar una tasa predefinida o escribir
una personalizada en **Tasa en S/s** y pulsar **Aplicar**. Se admiten
225 001–300 000 y 900 001–3 200 000 S/s. Con zoom ×1, el ancho mostrado
coincide con la tasa elegida y llega a 3,2 MHz.

Para ajustar la ganancia RF mientras recibes por USB o rtl_tcp, desmarca
**Ganancia automática del sintonizador** en **Entrada** y mueve **Ganancia RF**.
Puedes volver a activar el modo automático durante la misma recepción.

El visor RF está inspirado en el [Icom IC-7300MK2](https://www.icomjapan.com/lineup/products/IC-7300MK2/).
Al sintonizar, la traza del espectro sigue visible hasta que llegan muestras
nuevas. La cascada conserva las filas en sus frecuencias originales al cambiar
la sintonía. La escala y el rango bajo la cascada muestran
frecuencias RF en MHz. En **Centrado**, el rango acompaña la sintonía. En **Libre**,
el rango queda fijo y el marcador se mueve al sintonizar dentro de él. Para
llegar a una frecuencia fuera del rango, reduce el zoom o selecciona **Centrado**.

Los ajustes de radio, entrada, vista, barrido y ubicación se guardan
automáticamente en `${XDG_CONFIG_HOME:-$HOME/.config}/sdr-studio/settings.json`.
Al volver a abrir el programa, pulsa **Encender** para iniciar la recepción con
los valores recuperados. Los accesos de prueba `--demo` y `--hf` tienen sus
propios valores iniciales para la sesión.

- **WFM** para radiodifusión FM en mono, **NFM** para canales FM estrechos y **AM**
  para señales de amplitud modulada. **Digital** muestra sincronismos DMR y
  cabeceras P25.
- **Filtro** ajusta el ancho de banda del canal analógico. El medidor muestra
  potencia relativa en dBFS y el **Squelch** corta el audio por debajo del umbral.
  El volumen, el modo y el filtro se pueden cambiar durante la recepción.
- El audio usa `paplay` con PulseAudio o PipeWire compatible con PulseAudio.
  Quedan pendientes FM estéreo, RDS y voz P25.

### Bandas laterales, CW y RAW

Los botones de modo están bajo el visor y se pueden cambiar en vivo.

- **USB / LSB** demodulan la banda lateral superior o inferior. El paso
  inicial es de 100 Hz y el filtro de 2,7 kHz. El sombreado del espectro
  indica la banda lateral seleccionada.
- **DSB** combina ambas bandas laterales en audio mono, con un filtro inicial
  de 6 kHz.
- **CW** aplica un filtro inicial de 500 Hz y un paso de 10 Hz. El control
  **Tono CW** ajusta el batido audible entre 300 y 1200 Hz (700 Hz por defecto).
- **RAW** envía el canal IQ filtrado a la salida PCM estéreo de 48 kHz, con I a
  la izquierda y Q a la derecha. El ancho inicial del canal es de 12 kHz.

La sintonía se aplica entre bloques IQ con USB y `rtl_tcp`. El servidor
`rtl_tcp` no devuelve una confirmación de sintonía. La ganancia manual debe ser
una admitida por tu sintonizador.

### Escáner de rango

1. Pulsa **Escanear rango** en el panel izquierdo.
2. Elige una opción en **Bandas** para precargar **Inicio**, **Fin** y **Paso**.
   Puedes editar las frecuencias en MHz y el paso en kHz. Por ejemplo,
   445,950–446,000 MHz con un paso de 25 kHz recorre tres canales.
3. Pulsa **Iniciar barrido**. La radio se inicia si estaba apagada y usa el
   modo y el filtro actuales para comprobar cada canal.

El **Umbral de pausa**, en dBFS por bin, determina cuándo el barrido se queda
en una señal. Pulsa **Continuar** para pasar al canal siguiente o **Detener
barrido** para conservar la sintonía. **Espera por canal** ajusta el tiempo de
estabilización antes de medir. Cada barrido recorre el rango una vez en
sentido ascendente. La sintonía manual cancela el barrido.
El escáner funciona con USB, rtl_tcp y la demo. Al seleccionar otra banda
se cargan sus límites y su paso, conservando el umbral y la espera. Si hay
un conversor, los límites se recortan al rango compatible con él.

### Propagación HF

La ventana de **Propagación HF** permite seguir operando la radio.
Ábrela desde el panel izquierdo e introduce la latitud y la
longitud en grados decimales. Usa signo negativo para sur u oeste. La
aplicación calcula localmente la altura del Sol y orienta qué bandas
principales de radioafición y onda corta conviene explorar. En la orientación
diurna se considera el flujo solar y si Kp indica tormenta geomagnética.
Los resultados son orientativos y la apertura real depende de la ruta.

El panel consulta **Kp** y **flujo solar F10.7** públicos de NOAA SWPC mediante
`curl`, con sus fechas UTC. Pulsa **Actualizar** para consultar de nuevo.
Las solicitudes no contienen las coordenadas. Sin Internet sigue disponible
la orientación por luz local.
[NOAA SWPC](https://www.spaceweather.gov/products/planetary-k-index)
· [Cálculo de posición solar de USNO](https://aa.usno.navy.mil/faq/sun_approx)

### Selector de bandas

**Bandas** abre un panel con 46 accesos de 100 kHz a 1 GHz en cuatro columnas.
Cada selección ajusta la frecuencia central, el modo de escucha, el paso, el
filtro y el span del espectro. Por ejemplo, **FM comercial** carga WFM con
filtro de 180 kHz y paso de 100 kHz. Las filas **RF** abren la vista de
espectro con el modo Digital, sin audio. Son ajustes iniciales para explorar
frecuencias. Dentro de una banda puede haber señales con otros modos.

Con el receptor detenido, el preset puede ajustar también la tasa de muestreo.
En recepción cambia el zoom usando la tasa actual, lo que permite cambiar de
banda sin reiniciar la adquisición. Las filas incompatibles con el centro
admitido por el conversor activo quedan visibles y deshabilitadas. El límite
inferior especificado para el Blog V4 en modo Directo es 500 kHz. Para explorar
por debajo de 500 kHz, puedes usar el Nano, cuyo rango de entrada comienza en
100 kHz. [Ficha del Blog V4](https://www.rtl-sdr.com/wp-content/uploads/2024/12/RTLSDR_V4_Datasheet_V_1_0.pdf)

### AM en onda corta

Ejecuta `./run.sh --hf` para abrir el panel en AM a 9,650 MHz con la entrada
USB seleccionada. En **Bandas → Onda corta** están los accesos de 120 a 11 metros.
Cada preset de onda corta ajusta el centro, el modo AM, un paso de 5 kHz y un
filtro inicial de 6 u 8 kHz según la banda. También puedes escribir cualquier frecuencia
compatible directamente en el visor. Los centros de banda sirven como punto de
partida para explorar el espectro y no identifican emisoras activas.
[Asignaciones de radiodifusión HF de la UIT](https://www.itu.int/en/ITU-R/terrestrial/broadcast/Pages/bands.aspx)

Conecta una antena de recepción adecuada para onda corta y pulsa **Encender**.
Usa los botones de **4, 6, 8 o 10 kHz** para ajustar el ancho del filtro. El
**Nivel automático de audio AM** compensa variaciones de nivel en el audio.
Si estás buscando señales débiles, empieza con el squelch desactivado.

El RTL-SDR Blog V4 convierte HF internamente. Con un controlador compatible
basta con sintonizar la frecuencia deseada, sin activar el muestreo directo.
La prueba local usó librtlsdr 2.0.1 en Fedora.
[Guía del RTL-SDR Blog V4](https://www.rtl-sdr.com/v4/)

### Conversores externos

Con el receptor detenido, abre **Entrada → Conversor externo**. Este ajuste
funciona con **RTL-SDR · USB** y **RTL-SDR · TCP**.

- **Directo** usa la frecuencia del visor como frecuencia del sintonizador.
  Es la opción inicial y sirve para usar el Blog V4 con su conversor interno.
- **Ham It Up Nano · +125 MHz** usa el oscilador nominal del Nano. El visor,
  el espectro y la cascada conservan la frecuencia RF. Por ejemplo, **9,650 MHz**
  en el visor se sintoniza a **134,650 MHz** en el SDR. El perfil cubre
  0,100–65 MHz de entrada. Si lo seleccionas con una frecuencia fuera de ese
  intervalo, el panel pasa a 9,650 MHz en AM.
- **Desplazamiento personalizado** permite introducir la diferencia firmada
  **frecuencia del SDR − frecuencia del visor**, en Hz. Para un conversor
  que eleva la frecuencia en 125 MHz, introduce `125000000`.

Para el Nano, conecta la antena a **RF Input** y **IF Output** al SDR. Usa una
sola fuente de alimentación para el Nano, según su ficha. El perfil de SDR
Studio aplica el desplazamiento de frecuencia.
[Ficha de Nooelec](https://www.nooelec.com/store/downloads/dl/file/id/100/product/323/ham_it_up_nano_datasheet_revision_1.pdf)

La aplicación carga `librtlsdr.so.2` o `librtlsdr.so.0` en ejecución: no necesitas el SDR para
compilar, abrir la interfaz o usar la demo. Las reglas udev del paquete RTL-SDR
deben permitir acceso USB. Si el kernel ha tomado el dispositivo como DVB,
libéralo según las instrucciones de tu distribución; la app no cambia el sistema.
Una lectura USB síncrona puede retrasar la parada si el dispositivo deja de responder.

Para un servidor ya configurado con `rtl_tcp`, selecciona TCP e introduce
`host:puerto`. La conexión usa el protocolo RTL0; no aporta autenticación ni TLS.

Los archivos deben tener exactamente el formato **CU8 intercalado I,Q**. Selecciona
la tasa original de la grabación. No se aceptan WAV, CF32 o SigMF automáticamente.
La reproducción se realiza a velocidad real y finaliza al terminar el archivo.

```sh
cargo run --locked -- --demo        # interfaz con demostración iniciada
cargo run --locked -- --self-check  # comprobación sin pantalla ni hardware
cargo test --locked
```

## Complementos

```sh
rustc --edition=2021 --crate-type cdylib examples/counter_plugin.rs -o /tmp/counter.so
cargo run --locked -- --self-check --plugin /tmp/counter.so
```

En la interfaz, añade la ruta absoluta del `.so` antes de iniciar la recepción.
Se carga en el hilo receptor y recibe bloques de dibits 0..3 a 4800 símbolos/s.
La función exportada `sdr_plugin_v1` devuelve por valor la estructura documentada
en `include/sdr_plugin.h`. `shutdown` se invoca antes de descargar la biblioteca.
Carga solo complementos de confianza: tienen los permisos del proceso y una ABI
inválida puede cerrarlo. No hay aislamiento ni interfaz de audio/eventos para plugins
en v1; la futura ampliación debe usar un nuevo símbolo/ABI sin romper v1.

La extensión de hardware se hace actualmente en Rust mediante `SampleSource`,
no mediante los complementos de dibits. Un adaptador SoapySDR podrá implementar
esa misma interfaz y convertir las muestras a un formato común. La interfaz
deberá exponer también capacidades específicas de cada dispositivo.

## Estructura

- `source.rs`: fuentes IQ, control RTL-SDR USB/TCP y generador sintético.
- `audio.rs`: demodulación analógica, filtros de banda lateral y CW, nivel automático AM, squelch y salida PCM mono o IQ estéreo.
- `dmr_sync.rs`: sincronización DMR y extracción de ráfagas de voz de un intervalo.
- `dmr_audio.rs`: reconstrucción AMBE, señalización embebida y salida PCM por el módulo nativo.
- `dsp.rs`: filtro de canal inicial, discriminador, integración de símbolos y FFT.
- `protocol.rs`: sincronismos DMR/P25 y lectura de NID P25 con retirada del símbolo de estado.
- `p25_nid.rs`: corrección BCH y validación del NID P25.
- `receiver.rs`: hilo de adquisición, cancelación, alineación IQ y publicación de estado.
- `plugins.rs`: carga y ciclo de vida de bibliotecas con ABI C.
- `scope.rs`: espectro con modos Centrado y Libre, escala en MHz, zoom y cascada continua.
- `panel.rs`: visor de siete segmentos, dial giratorio y medidor de aguja.
- `main.rs`: interfaz nativa y comprobación sin pantalla.

## Escuchar voz DMR

En el ejecutable local, el módulo de voz se carga desde `bin/libsdstudio-dmr.so`.
Selecciona **Digital**, sintoniza la portadora y pulsa **Encender**. El audio de
voz DMR convencional sin cifrar usa los controles **Volumen** y **Silenciar**.
En un repetidor con dos intervalos activos, se escucha un intervalo a la vez.
La estructura de ráfagas sigue la [especificación ETSI TS 102 361-1](https://www.etsi.org/deliver/etsi_ts/102300_102399/10236101/02.05.01_60/ts_10236101v020501p.pdf).
La opción **Mostrar sincronismos DMR** permite distinguir actividad de voz y de
datos. Las ráfagas de datos aparecen en el panel de actividad digital.

Para compilar el módulo desde el paquete fuente, usa el código de
[mbelib](https://github.com/lwvmobile/mbelib/tree/30dc79074ca022366a27d705b8023011d9600339)
y un compilador C. El módulo se probó con esa revisión.

```sh
./native/build-dmr-audio.sh /ruta/a/mbelib
./run.sh
```

La comprobación de IQ a PCM usa una ráfaga sintética con un tono AMBE de
1 kHz. Se puede repetir con el módulo compilado y esta prueba.

```sh
SDR_STUDIO_DMR_LIBRARY="$PWD/bin/libsdstudio-dmr.so" \
  cargo test --locked dmr_iq_to_pcm -- --ignored
```

## Trabajo necesario para voz y datos completos

1. Capturas de referencia con resultados conocidos: DMR convencional y P25 Phase 1
   C4FM sin cifrar. Añadir pruebas independientes del generador interno, con ruido,
   offset de frecuencia, fases y errores de reloj.
2. Filtro de canal apropiado, decimación, recuperación de reloj y seguimiento de
   frecuencia. El integrador fijo actual no garantiza recepción real.
3. DMR: identificación y selección manual de intervalos, BPTC, trellis, CRC, LC y extracción de datos. P25: estados en toda la trama, deinterleaving,
   Golay/Hamming/Reed–Solomon/CRC según unidad, HDU/LDU/TSDU/PDU y metadatos.
4. Pruebas de conformidad AMBE+2, manejo de pérdida de ráfagas y reproducción IMBE. Revisar licencias de cualquier
   implementación incorporada. No se incluye descifrado de comunicaciones.
5. Grabación IQ/audio y exportación de eventos; perfiles persistentes; adaptadores
   SoapySDR y otros SDR. Después, CQPSK, trunking y P25 Phase 2.

No se afirma conformidad con ETSI/TIA ni recepción validada por hardware.
Los tests sintéticos verifican el cableado del procesamiento y casos de framing;
no certifican un decodificador de radio completo.

## Ejecutable local

Ejecuta `./run.sh --demo` desde esta carpeta para abrir la demostración.
Con `./run.sh` puedes seleccionar RTL-SDR USB e iniciar la recepción manualmente.
Desde el paquete fuente, el lanzador compila la aplicación con Cargo.
La copia de desarrollo local puede incluir un ejecutable para Fedora 44 x86_64.
Consulta `VALIDATION.md` para ver las comprobaciones locales.


## Distribución

El paquete fuente incluye el código de SDR Studio, `Cargo.lock`, el lanzador y
el texto completo de la licencia. Las dependencias se descargan por separado y
conservan sus licencias. La compilación actual enlaza dependencias con licencia
Apache-2.0, como `winit` y `glutin`. La Free Software Foundation considera esa
licencia [incompatible con GPLv2](https://www.gnu.org/licenses/license-list.html#apache2).
El archivo de distribución contiene únicamente el código fuente del proyecto.
