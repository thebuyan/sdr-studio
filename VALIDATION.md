# Validación local

Fedora 44 x86_64, X11, Rust 1.99.1. Fecha 11 de noviembre de 2026.

- 29 pruebas aprobadas con `cargo test --locked -- --include-ignored` y el módulo DMR configurado. Incluyen el NID P25 de referencia,
  corrección de 11 bits, rechazo de cabeceras inválidas y extracción del NID
  tras retirar el símbolo de estado en ambas polaridades.
- `cargo clippy --locked --all-targets -- -D warnings` y `cargo fmt --check`: aprobados.
- `--self-check --plugin`: la demo IQ produjo sincronismos DMR y el NID
  P25 con NAC 293 y DUID 5. Se cargó el complemento de ejemplo y procesó 4 dibits.
- La ventana se abrió en X11 con la demo. En una pantalla de prueba, un clic
  en el espectro cambió la frecuencia mientras seguía la recepción. En modo
  Libre, al pasar de 446,000000 a 446,060317 MHz el rango siguió en
  445,880000–446,120000 MHz. Al volver a Centrado, el rango siguió al VFO.
  La prueba TCP comprobó que sintonizar dentro del rango fijo no envía un
  nuevo comando al sintonizador. La prueba de continuidad comprobó que la
  traza conserva el último cuadro hasta completar una FFT nueva, con numeración
  ascendente y con el centro RF correspondiente a cada cuadro.
- El panel clásico se revisó a 1250 × 840 y 930 × 660. En la demo se comprobó
  la rueda sobre el dial, el arrastre y la entrada directa mediante el visor.
- La rueda sobre un dígito ajustó la frecuencia según su posición.
  La prueba de cascada comprueba que el historial permanece en sus frecuencias
  RF al cambiar el centro y que una sesión nueva comienza con la cascada vacía.
- En X11, se cambió a Digital, 144,420 MHz, vista Libre y zoom ×2.
  Tras cerrar y abrir el programa con la misma carpeta de configuración,
  los cuatro valores se recuperaron desde el archivo JSON.
- Se aplicó una tasa personalizada de 3 100 000 S/s desde la interfaz y se
  recuperó al volver a abrir. En una prueba USB breve, el Blog V4 aceptó
  3 200 000 S/s. Se observaron pérdidas de muestras a 2,8 y 3,2 MS/s.
  No se observaron pérdidas durante las pruebas a 2,4 y 2,56 MS/s.
- Con el Blog V4 se cambió de ganancia automática a 28,0 dB, luego a
  34,4 dB y de vuelta a automática sin cerrar la sesión USB. Se recibieron
  8192 muestras IQ tras cada cambio. Una prueba rtl_tcp verificó los
  comandos de modo y nivel durante la recepción.
- Se cambió la interfaz de español a inglés en X11. Tras cerrar y abrir,
  el idioma inglés se restauró con la misma configuración de frecuencia y modo.
- El controlador abrió un RTL-SDR Blog V4 con librtlsdr 2.0.1, a 240 kS/s.
  Pasó de 100,0 a 100,1 y 100,2 MHz en la misma sesión, y de WFM a NFM.
  El contador IQ avanzó y el dispositivo se liberó al detener.
- PipeWire recibió el flujo PCM de SDR Studio con el audio silenciado durante
  la prueba USB. Señales AM, NFM y WFM sintéticas produjeron un tono de 1 kHz
  a 48 kHz y verificaron el funcionamiento del squelch y el mute.
- El servidor `rtl_tcp` simulado recibió el comando de sintonía y continuó
  enviando IQ por la misma conexión.
- En AM HF, el Blog V4 entregó IQ a 6,050, 9,650 y 15,450 MHz a 240 kS/s
  en una sola sesión. El flujo de audio se abrió con el sonido silenciado.
  La prueba de filtro AM recuperó una señal sintética débil junto a una portadora
  más fuerte separada por 5 kHz.
- Con el perfil Ham It Up Nano, un servidor rtl_tcp simulado recibió 134,650
  y 131,050 MHz para las frecuencias RF de 9,650 y 6,050 MHz. La prueba también
  verificó el rechazo de una sintonía fuera del rango sin cerrar la sesión.
  El conversor físico estaba desconectado.
- El catálogo de bandas cubre de 100 kHz a 1 GHz sin huecos y las frecuencias
  iniciales respetan sus pasos. El selector muestra los 46 accesos a 930 × 660.
  Se comprobó el cambio de modo, filtro, paso, tasa y span desde el panel.
- En señales sintéticas, USB y LSB separaron las dos bandas, DSB recuperó
  ambas, CW produjo el tono configurado de 850 Hz y RAW conservó I y Q en
  canales separados. PipeWire recibió la secuencia USB mono → RAW estéreo →
  CW mono a 48 kHz mientras el contador IQ siguió avanzando. La prueba se
  hizo con la señal sintética silenciada.
- El escáner recibió un rango de 445,950 a 446,000 MHz con paso de 25 kHz
  en la interfaz. La demo se pausó por señal y **Continuar** avanzó al
  canal siguiente. La prueba automatizada cubre el descarte de FFT antiguas
  y el final del rango.
- Al elegir FM comercial, el escáner cargó 87,5–108 MHz con paso de 100 kHz.
  En 31 m cargó 9,4–9,9 MHz con paso de 5 kHz.
- El panel de propagación recibió observaciones reales de Kp y F10.7 de NOAA.
  Las pruebas cubren el cálculo solar por ubicación y la lectura del índice
  más reciente dentro del rango válido.
- Con una consulta NOAA retrasada cinco segundos, Propagación HF mantuvo
  su posición al escribir coordenadas y al recibir los datos. Se pudo
  cambiar el modo de la radio con la ventana abierta a 930 × 660.
- Se detectaron los ocho patrones DMR de estación base, móvil y modo directo
  a 240 y 256 kS/s, con fase inicial variable, deriva de reloj de 50 ppm y
  desplazamientos de portadora de +1050 y −920 Hz en IQ sintético.
- Una señal IQ DMR sintética con voz en un intervalo y datos en el otro produjo
  360 ms de PCM mono a 8 kHz con un tono de 1 kHz. La misma prueba comprobó
  el control de silencio y el rechazo de ráfagas con el indicador de privacidad.

- Con una captura real de DMR Directo TS2 en 144,420 MHz y el Blog V4,
  el filtro adaptado y el seguimiento por ráfaga recuperaron 220 bloques con
  señal de audio en la ventana analizada. La versión anterior recuperó 74.
  La reproducción de un fragmento de esa captura llegó a una salida virtual
  de PipeWire en PCM mono a 8 kHz.

La decodificación P25 se comprobó con cabeceras de referencia y señal sintética.
La voz P25 y los mensajes completos siguen pendientes.

El vector de prueba para NAC 293 y DUID 5 usa la matriz del generador de
[OP25](https://github.com/boatbod/op25/blob/master/op25/gr-op25_repeater/apps/tx/p25craft.py).
