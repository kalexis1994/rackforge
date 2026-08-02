# Lector SCVA nativo

`rackforge-scva-bank` es la primera capa nativa ARM64 del futuro motor SC-8820 de
RackForge. Lee los candidatos de Wave ROM extraídos de una copia legítima de Sound
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
  descriptores de muestra;
- reproduce parciales nativos con el mapa ROM, interpolador FIR, afinación y
  loops observados en SCCore;
- precarga las notas 36..96 y recibe MIDI directamente del KeyLab;
- entrega audio S32_LE a 48 kHz por la Scarlett.

Todavía no replica una voz completa de SCCore. Faltan sus envolventes TVF/TVA,
modulación, curvas de velocidad/volumen, efectos y todos los modos de
reproducción de descriptores. El motor nativo se mantiene separado del banco
renderizado estable mientras continúa el RE.

## Archivos locales

Los bancos se instalan fuera de Git:

```text
/home/kalex/rackforge/share/scva/
├── wave_1994_ver200_8mib.bin
├── wave_1996_rom_make_a_8mib.bin
├── wave_1996_rom_make_b_4mib.bin
├── wave_1999_sc8820_4mib.bin
└── control-v1/
    ├── sample-descriptors.bin
    ├── wave-maps.bin
    ├── tones.bin
    └── interpolation-coefficients.bin
```

## Compilación en la Raspberry

```bash
cd /home/kalex/rackforge/current/platforms/raspberry-pi/engines/scva-arm64
sh ./build.sh
/home/kalex/rackforge/bin/rackforge-scva-bank \
  inspect /home/kalex/rackforge/share/scva
```

Para resolver `Piano 1` (tono 0) y C4 (nota MIDI 60):

```bash
rackforge-scva-bank resolve \
  /home/kalex/rackforge/share/scva/control-v1 0 60
```

El render offline sigue esa resolución, aplica el mapa ROM de SCCore,
decodifica los rangos completos, afina y mezcla los parciales:

```bash
rackforge-scva-bank render-tone \
  /home/kalex/rackforge/share/scva \
  /home/kalex/rackforge/share/scva/control-v1 \
  0 60 /tmp/piano1-c4-preview.wav
```

Es un preview de investigación, no el sintetizador terminado.

Para decodificar un rango conocido:

```bash
rackforge-scva-bank decode /home/kalex/rackforge/share/scva \
  sc88-rev200 0 0x8000 0x10000 /tmp/candidate.wav
```

Para aplicar el layout utilizado por el runtime de SCCore:

```bash
rackforge-scva-bank decode-sccore /home/kalex/rackforge/share/scva \
  rom-make-a 1 0x1ff22 48286 /tmp/strings-partial.wav
```

La salida WAV de diagnóstico se normaliza para escucharla. La tasa nominal
confirmada de reproducción de la ROM es 44,1 kHz; la cabecera de 32 kHz de
algunos oráculos antiguos era sólo una etiqueta de laboratorio.

## Validación cruzada

El mismo rango (`sc88-rev200`, segmento 0, `0x8000..0x18000`) fue decodificado
en Windows x86-64 y Debian ARM64. Ambos WAV produjeron:

```text
SHA-256 52da487847336bb7839b82cfd9e349b49e9e1367015d3ded496c0013cc044958
peak    302076
```

Esto verifica la lectura y el algoritmo DPCM genérico entre arquitecturas. El
layout musical del runtime se valida por separado contra estados internos de
SCCore; está documentado en `tools/scva-inspect/RESEARCH.md`.

## Banco renderizado provisional

Mientras se porta el DSP completo de SCCore, el motor puede cargar un banco
multimuestreado generado localmente por la instalación legítima:

```powershell
.\platforms\raspberry-pi\dev\render-scva-bank.ps1 `
  -SCCorePath "C:\ruta\SCCore.dll" `
  -OutputDirectory "C:\ruta\rackforge-rendered-piano"
```

En la Raspberry:

```bash
rackforge-scva-live \
  --rendered-bank /home/kalex/rackforge/share/rendered-piano \
  --gain 1.0
```

Los WAV derivados son privados y no se guardan en Git.
