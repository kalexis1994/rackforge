# Roland Sound Canvas VA plugin

Primer plugin de instrumento para ArtuPy. Reproduce un banco multimuestreado
generado por una instalación legítima de Sound Canvas VA sin incluir audio ni
datos propietarios en Git.

## Recurso requerido

El manifiesto declara el recurso externo `rendered-bank` como directorio
obligatorio. ArtuPy Core valida su existencia antes de cargar código del
plugin y lo entrega mediante la API de host 1.1.

Formato provisional:

```text
rendered-bank/
├── bank.json
├── note-036.wav
├── …
└── note-096.wav
```

Cada WAV puede ser float32 o PCM16, mono o multicanal. El plugin reduce los
canales a mono y adapta la frecuencia de muestreo durante la reproducción.

## Capacidades iniciales

- MIDI Note On/Off;
- velocity;
- pedal sustain CC64;
- All Sound Off CC120 y All Notes Off CC123;
- 16 voces con robo determinista;
- automatización sample-accurate;
- estado persistente;
- preset `Sound Canvas VA / Piano 1`;
- parámetros `Volume`, `Attack` y `Release`.

## Prueba

```powershell
cd raspberry\runtime
cargo build -p artupy-core -p artupy-roland-scva
cargo run -p artupy-core -- `
  smoke plugins/roland-scva/package `
  --library target/debug/artupy_roland_scva.dll `
  --resource "rendered-bank=C:\ruta\artupy-rendered-piano"
```

En ARM64 la biblioteca es `target/debug/libartupy_roland_scva.so`.

## Limitaciones provisionales

- solo está declarado Piano 1;
- las muestras duran cuatro segundos y todavía no tienen loops de sustain;
- Attack agrega una envolvente sobre el ataque nativo ya renderizado;
- todavía no hay pitch bend, chorus ni reverb;
- no reemplaza automáticamente al daemon de audio estable.

