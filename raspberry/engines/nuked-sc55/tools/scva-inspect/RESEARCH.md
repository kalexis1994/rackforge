# Sound Canvas VA 1.1.2: notas de RE

Fecha del análisis: 2026-07-29.

Estas notas describen una copia local legítima. Los binarios, bancos extraídos
y WAV de prueba no forman parte de RackForge y no deben redistribuirse.

## Binario analizado

```text
archivo: SCCore.dll
tamaño: 27,358,208 bytes
arquitectura: PE x86-64 para Windows
SHA-256: 0635cc2bfced7876694f362f29719bae58e4539d576af9321673f6ffc31f6735
```

La mayor parte de los datos de ondas reside en `.rdata`. Los marcadores
internos revelan cuatro grupos de segmentos de 1 MiB:

| Candidato extraído | Offset | Tamaño | SHA-256 |
|---|---:|---:|---|
| `wave_1994_ver200_8mib.bin` | `0x966c0` | 8 MiB | `05a36e2e354611e667b643d619c9c1d2a2f0836bd585189e061b82f27b827385` |
| `wave_1996_rom_make_a_8mib.bin` | `0x8966c0` | 8 MiB | `0e5edc077367165751464ee8d9028a5c6b23cf57ad69254d3ff687da5c2de0a6` |
| `wave_1996_rom_make_b_4mib.bin` | `0x10966f0` | 4 MiB | `bc96fb86fae38ce1b187e48b75e3bcbca444821522deb7b5105821759b51d391` |
| `wave_1999_sc8820_4mib.bin` | `0x14966f0` | 4 MiB | `5e7c4e32963da835db54e3663221606ee875bf1b20a0c4f0d57ebacdc5085be2` |

El tramo completo, incluyendo el desplazamiento de `0x30` bytes observado
entre grupos, comienza en `0x966c0`, ocupa `0x1800030` bytes y tiene SHA-256
`437692123a2e5e2516eb9f3b2c90415719b8e31a66bfd0eb224bf2e79a6860e0`.

Los 24 segmentos de 1 MiB y todas las parejas contiguas de 2 MiB se compararon
con los hashes SC-55mkII conocidos por Nuked-SC55. No hubo coincidencias
exactas. Por tanto, estos candidatos no son un reemplazo directo de
`waverom1.bin` y `waverom2.bin`.

## ABI del sintetizador

`SCCore.dll` exporta una API C pequeña, incluida:

```text
TG_initialize
TG_activate
TG_deactivate
TG_setSampleRate
TG_setMaxBlockSize
TG_setInterruptThreadIdAtThisTime
TG_ShortMidiIn
TG_LongMidiIn
TG_Process
TG_XPgetCurTotalRunningVoices
```

Las firmas usadas por `scva-render` se contrastaron con la implementación
pública del host SCCore de kode54:

<https://gist.github.com/kode54/01929e2f1dfc9ee4f8f1>

La sonda cargó la DLL, seleccionó Program 0, envió C4 con velocidad 100 y
renderizó cuatro segundos a 44,1 kHz:

```text
frames: 176400
peak: 0.04777012
rms: 0.00322477
```

Esto demuestra que el ABI y los datos internos son funcionales en Windows. No
demuestra compatibilidad ARM64: la DLL contiene código x86-64 y depende de la
API de Windows. Llevar este motor a la Raspberry exige una reimplementación
nativa del DSP y de su lector de bancos, o una capa de traducción que habrá que
medir antes de considerarla apta para baja latencia.

## Decodificación nativa

Cada segmento de 1 MiB contiene una tabla de escalas de `0x8000` bytes. Para
una dirección de muestra `a`, el byte de escala está en `a >> 5`; el bit 4 de
`a` selecciona su nibble bajo o alto. El byte de muestra es un delta con signo
y el PCM se reconstruye acumulando:

```text
pcm[n] = pcm[n - 1] + (signed_byte[a] << scale_nibble[a])
```

Este esquema coincide con la documentación y utilidad pública de FCE-DPCM para
ROMs Roland:

<https://gist.github.com/giulioz/39e96282371ffb5059e112f6281efa60>

`engines/scva-arm64` implementa esa lectura sin descrambling adicional, ya que
los datos incluidos en `SCCore.dll` están ordenados. La salida de un rango de
prueba fue idéntica en Windows x86-64 y Debian ARM64:

```text
grupo: sc88-rev200
segmento: 0
rango: 0x8000..0x18000
peak: 302076
WAV SHA-256: 52da487847336bb7839b82cfd9e349b49e9e1367015d3ded496c0013cc044958
```

## Tablas de control

Una tabla de descriptores en `.data` referencia seis bloques estáticos de
control. Sus callbacks de tamaño y los accesos del DSP confirman estos layouts:

| Bloque | Offset | Tamaño | Estructura confirmada |
|---|---:|---:|---|
| sistema | `0x189a5d0` | `0x58` | configuración global; `u16 +2 = 2` parciales |
| direcciones con nombre | `0x189a630` | `0x4b0` | 50 registros de `0x18` |
| descriptores de muestra | `0x189aae0` | `0x16e04` | 4259 registros de `0x16` y 2 bytes finales |
| kits de batería | `0x18b18f0` | `0x1bc20` | 88 registros de `0x50c` |
| mapas de onda | `0x18cd510` | `0x28294` | 1175 registros de `0x8c` |
| tonos | `0x18f57b0` | `0x93b00` | 2363 registros de `0x100` |

Cada tono tiene un encabezado de `0x24` bytes y dos parciales contiguos de
`0x6e`. Dentro de cada parcial, el `u16 +2` selecciona un mapa de onda y el
byte `+4` ajusta la nota alrededor de `0x40`.

Cada mapa de onda contiene:

```text
+0x00  nombre de 12 bytes
+0x0c  32 límites superiores de tecla (u8)
+0x2c  32 índices de descriptor de muestra (u16 LE)
+0x6c  32 parámetros por zona (u8)
```

Para `Piano 1` (tono 0) y C4 (nota 60), el resolvedor obtiene:

```text
parcial 1: mapa 0 "Stway end"    -> zona 7 -> muestra 7
parcial 2: mapa 1 "Steinway-D p" -> zona 7 -> muestra 20
```

## Descriptor de muestra

El DSP reconstruye cada descriptor de `0x16` bytes de esta forma:

- `byte 0`: segmento plano de Wave ROM (`0..23`);
- `bytes 1..3`, `7..9` y `11..13`: tres direcciones de 20 bits, usando sólo
  el nibble bajo del primer byte;
- `byte 10`: flags; bit 1 marca una onda *one-shot* (si está limpio se usa
  loop) y bit 2 invierte la disposición;
- `u16 LE +4` y `byte +6`: afinación fina y tecla raíz.

Los segmentos planos se corresponden con los grupos extraídos:

```text
0..7   -> wave_1994_ver200
8..15  -> wave_1996_rom_make_a
16..19 -> wave_1996_rom_make_b
20..23 -> wave_1999_sc8820
```

Para las dos muestras de `Piano 1/C4`:

```text
muestra 7:
  rom-make-a segmento 0
  start=0x74ee0 loop=0x7ed23 end=0x836de
  root=64 fine=972 flags=0

muestra 20:
  rom-make-a segmento 2
  start=0x290a0 loop=0x2e8db end=0x311df
  root=74 fine=1000 flags=0
```

`rackforge-scva-bank render-tone` sigue toda esta cadena, decodifica FCE-DPCM,
aplica la afinación raíz y mezcla ambos parciales. El preview C4 fue idéntico
en Windows x86-64 y Debian ARM64:

```text
frames: 75054 a 32 kHz
SHA-256: 286b5a34530c6b87ac77e72c62b41cc50ea34a60a5906b8168bf24d6d303106f
```

## Layout de reproducción del runtime

La decodificación de un rango arbitrario y la reproducción de un descriptor no
usan exactamente las mismas bases. La rutina que prepara una voz alinea la
primera dirección a 32 bytes y SCCore conserva bases independientes:

```text
aligned_start = descriptor_start & ~0x1f
data_base     = aligned_start - 0x20
scale_base    = (aligned_start >> 5) - 0x20

delta[n]      = segment[data_base + n]
scale_byte[n] = segment[scale_base + (n >> 5)]
```

Es incorrecto calcular el exponente con
`(data_base + n) >> 5`: selecciona otra zona de la tabla de escalas y produce
el timbre áspero que se observó en las primeras pruebas nativas.

La comprobación dinámica de `Strings 1`, parcial 2, en el bloque 16 dio:

```text
descriptor start: 0x1ff22
aligned_start:    0x1ff20
data_base:        0x1ff00
scale_base:       0x0fd9
posición:         278
acumulador SCCore (antes de <<10): -7447
acumulador RackForge:                 -7447
```

Los punteros de datos y escalas se volcaron por separado y sus primeros 32
bytes se localizaron de forma única en el segmento de ROM. Los volcados y WAV
derivados permanecen fuera de Git.

## Cursor e interpolación

El banco tiene una tasa nominal de 44,1 kHz. SCCore usa un acumulador de fase
de 16 bits y las siete posiciones superiores de la fracción seleccionan una de
128 fases FIR. Los cuatro coeficientes se aplican al historial:

```text
pcm[position - 3], pcm[position - 2], pcm[position - 1], pcm[position]
```

El decoder prepara cuatro muestras antes del primer frame, por lo que el cursor
inicial del interpolador es 3. Para `Strings 1` (programa MIDI 48, tono interno
390) y C4 se observaron estos incrementos a 44,1 kHz:

```text
parcial 1: 45774 / 65536 = 0.698455810546875
parcial 2: 56467 / 65536 = 0.8616180419921875
```

Después de corregir el mapa ROM, un bloque de 32 frames de la parcial 2,
quitando las ganancias TVA observadas, correlacionó con la implementación
ARM64 en `0.9999999999999991`.

## Continuidad de loops

Las variantes loop del decoder conservan el acumulador DPCM y la fase FIR al
volver desde `end` a `loop_start`. En `Strings 1/C4` la suma de deltas de cada
ciclo cierra exactamente:

```text
parcial 1: accumulator(end) - accumulator(before_loop) = 0
parcial 2: accumulator(end) - accumulator(before_loop) = 0
```

Por tanto no existe deriva de continua entre ciclos. El minicorte de la primera
implementación ARM64 provenía de redondear `loop_start / increment` a un frame
entero del cache a 48 kHz: el salto volvía con otra fase fraccional.

Hasta que el decoder pase a ser completamente incremental por voz, el cache
nativo usa un puente lineal de 192 frames (4 ms) en la unión y reinicia después
del tramo de cabeza ya consumido por la mezcla. En C2, parcial 1 de Strings, la
diferencia de la unión bajó de `5224` a `155` unidades PCM sin normalizar.

## TVF

El filtro para el modo normal es un low-pass de variable de estado. Para cada
muestra y cada lane:

```text
s2 += cutoff * s1
output = s2
s1 += cutoff * (input - (resonance * s1 + s2))
```

En `Strings 1/C4` los coeficientes observados durante el bloque fueron
`cutoff=1` y `resonance=1`, es decir, el TVF está abierto. El filtro no era la
causa del audio roto; el problema estaba antes, en el mapa de datos/escalas y
en la tasa de reproducción.

## Reverb y repetición perceptible

Una captura sostenida de siete segundos de `Strings 1/C4` mostró que el ataque
de SCCore comienza prácticamente mono. Durante el sustain, la diferencia
estéreo crece hasta aproximadamente un 8–17 % del nivel central y cambia a lo
largo del tiempo. Por tanto, la unión de la muestra no es la única responsable
de que el loop resulte perceptible: el motor original superpone una cola
temporal decorrelacionada que evita repetir exactamente el mismo bloque.

Las capturas de control confirmaron que esa cola pertenece principalmente a la
reverb:

```text
CC91=0, CC93=0: salida prácticamente seca y mono
CC91=0:         prácticamente idéntica a la anterior
CC93=0:         conserva casi toda la apertura del render por defecto
```

En el sustain, la diferencia entre el render por defecto y el seco tuvo RMS
`0.00187` a izquierda y `0.00372` a derecha, frente a un RMS seco aproximado
de `0.0164`. `scva-render` admite ahora `--duration-ms`, `--note-off-ms` y
múltiples pares `--cc NUMBER VALUE` para repetir estos experimentos.

La primera etapa nativa de RackForge usa una red estéreo estable de combs y
all-pass a 48 kHz con una devolución discreta. Es una implementación
provisional y separada del decoder: permite verificar perceptualmente cuánto
enmascara el ciclo, mientras se terminan de identificar la topología y los
coeficientes exactos de SCCore.

## Estado pendiente

El lector ARM64 ya reproduce las ondas con el layout, interpolador, afinación
y loops verificados. Todavía faltan las envolventes TVA/TVF exactas, curvas de
velocidad/volumen, modulación, efectos y los demás modos de descriptor antes de
considerarlo una réplica completa de SCCore.
