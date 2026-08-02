# Roland Sound Canvas VA plugin

Primer plugin de instrumento para RackForge. Reproduce un banco multimuestreado
generado por una instalación legítima de Sound Canvas VA sin incluir audio ni
datos propietarios en Git.

## Recurso requerido

El manifiesto declara el recurso externo `rendered-bank` como directorio
obligatorio. RackForge Core valida su existencia antes de cargar código del
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
- envolvente ADSR por instrumento, con liberación continua aun si la tecla se
  suelta durante Attack o Decay;
- parámetros `Volume`, `Attack`, `Decay`, `Sustain` y `Release`.

## Programas

El modelo inicial admite hasta dos capas (`A` y `B`), aunque Piano 1 empieza
solamente con `A`. Cada capa declara:

- `sound_id`;
- activación, gain y pan;
- octava, transposición y afinación fina;
- rangos de tecla y velocity;
- ADSR propia.

El documento común pertenece a RackForge y el payload pertenece a Roland. El
ejemplo validado está en `programs/factory.piano-1.json`. La estructura de
carpetas que Roland elija dentro de su raíz privada no forma parte del contrato
global y puede evolucionar con migraciones del plugin.

## Prueba

```powershell
cd C:\ruta\a\rackforge
cargo build -p rackforge-core -p rackforge-roland-scva
cargo run -p rackforge-core -- `
  smoke plugins/roland-scva/package `
  --library target/debug/rackforge_roland_scva.dll `
  --resource "rendered-bank=C:\ruta\rackforge-rendered-piano"
```

En ARM64 la biblioteca es `target/debug/librackforge_roland_scva.so`.

## Limitaciones provisionales

- solo está declarado Piano 1;
- las muestras duran cuatro segundos y todavía no tienen loops de sustain;
- la ADSR agrega una envolvente sobre la dinámica nativa ya renderizada;
- todavía no hay pitch bend, chorus ni reverb;
- no reemplaza automáticamente al daemon de audio estable.

## Frontera y futura extracción

El plugin permanece dentro del monorepo mientras se estabilizan DSP, parámetros,
presets, recursos y UI. Su directorio es deliberadamente autocontenido para
poder moverlo más adelante a un repositorio propio.

Reglas de esa frontera:

- el código del plugin puede depender de `rackforge-plugin-api` y bibliotecas de
  propósito general, pero nunca de módulos privados de `rackforge-core`;
- manifiesto, implementación, presets propios y pruebas viven dentro de
  `plugins/roland-scva/`;
- ROMs, WAV renderizados, bancos y cualquier material propietario son recursos
  externos y nunca forman parte del repositorio;
- `platforms/raspberry-pi/engines/nuked-sc55/` es tooling/experimentación independiente, no
  una dependencia del plugin en tiempo de ejecución;
- RackForge debe cargar el paquete por la ABI pública igual que cargaría un plugin
  de terceros.

Cuando la ABI y el formato de paquete sean estables, la extracción consistirá
en mover este directorio, sustituir la dependencia `path` de la API por una
versión etiquetada del SDK, agregar CI propio y hacer que RackForge consuma un
paquete publicado. Hasta entonces mantenerlo aquí permite depurar host y plugin
en una sola revisión sin debilitar la separación.
