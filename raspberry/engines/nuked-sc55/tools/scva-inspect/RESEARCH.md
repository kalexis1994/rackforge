# Sound Canvas VA 1.1.2: notas de RE

Fecha del análisis: 2026-07-29.

Estas notas describen una copia local legítima. Los binarios, bancos extraídos
y WAV de prueba no forman parte de ArtuPy y no deben redistribuirse.

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
