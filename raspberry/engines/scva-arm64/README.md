# Lector SCVA nativo

`artupy-scva-bank` es la primera capa nativa ARM64 del futuro motor SC-8820 de
ArtuPy. Lee los candidatos de Wave ROM extraídos de una copia legítima de Sound
Canvas VA 1.1.2; no carga ni ejecuta `SCCore.dll`.

## Alcance actual

- exige los cuatro archivos y sus tamaños exactos;
- valida el marcador y fecha de cada segmento de 1 MiB;
- calcula hashes e identifica exactamente el corpus 1.1.2 analizado;
- mantiene separados los segmentos porque cada uno posee su propia tabla de
  escalas;
- decodifica rangos FCE-DPCM a PCM para investigación;
- valida las tablas de control 1.1.2 por tamaño y SHA-256;
- resuelve un tono y nota MIDI a sus dos parciales, mapas de onda y
  descriptores de muestra.

Todavía no sintetiza una voz completa. La selección de muestras ya está
implementada; falta decodificar los campos de dirección, loop y afinación de
cada descriptor y reproducir las envolventes/filtros.

## Archivos locales

Los bancos se instalan fuera de Git:

```text
/home/kalex/artupy/share/scva/
├── wave_1994_ver200_8mib.bin
├── wave_1996_rom_make_a_8mib.bin
├── wave_1996_rom_make_b_4mib.bin
├── wave_1999_sc8820_4mib.bin
└── control-v1/
    ├── sample-descriptors.bin
    ├── wave-maps.bin
    └── tones.bin
```

## Compilación en la Raspberry

```bash
cd /home/kalex/artupy/current/engines/scva-arm64
sh ./build.sh
/home/kalex/artupy/bin/artupy-scva-bank \
  inspect /home/kalex/artupy/share/scva
```

Para resolver `Piano 1` (tono 0) y C4 (nota MIDI 60):

```bash
artupy-scva-bank resolve \
  /home/kalex/artupy/share/scva/control-v1 0 60
```

El render offline sigue esa resolución, decodifica los rangos completos,
aplica la afinación raíz observada en el descriptor, remuestrea linealmente y
mezcla los parciales:

```bash
artupy-scva-bank render-tone \
  /home/kalex/artupy/share/scva \
  /home/kalex/artupy/share/scva/control-v1 \
  0 60 /tmp/piano1-c4-preview.wav
```

Es un preview de investigación, no el sintetizador terminado: aún no reproduce
loops, filtros, envolventes, modulación ni la curva de volumen de SCCore.

Para decodificar un rango conocido:

```bash
artupy-scva-bank decode /home/kalex/artupy/share/scva \
  sc88-rev200 0 0x8000 0x10000 /tmp/candidate.wav
```

La salida WAV de diagnóstico se normaliza para escucharla; no implica que el
rango corresponda a una muestra completa ni que 32 kHz sea su afinación real.

## Validación cruzada

El mismo rango (`sc88-rev200`, segmento 0, `0x8000..0x18000`) fue decodificado
en Windows x86-64 y Debian ARM64. Ambos WAV produjeron:

```text
SHA-256 52da487847336bb7839b82cfd9e349b49e9e1367015d3ded496c0013cc044958
peak    302076
```

Esto verifica la lectura y el algoritmo DPCM entre arquitecturas. No verifica
todavía la selección de una muestra musical completa.
