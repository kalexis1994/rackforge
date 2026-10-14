# Concert Grand completo en Raspberry Pi — 2026-10-14

Se instala Concert Grand **0.171.36** en la Raspberry Pi 4 del usuario. El
DSP es el de los renders aceptados: fuente `cb6d553c638f06fb43010984b6321ebcb9fa6f137832e755ba47eb9fa5e16b77`,
preset **Concert**, `MERGE_RAMP_S = 0.004`, 44.100 Hz, bloques de 256 muestras.
La versión cambia los metadatos del paquete; no cambia las ecuaciones ni las
constantes compiladas respecto de `6b403c5c`.

## Identidad de audio

Se compilan SDK, piano y probe directamente en ARM64 con `rustc -O`, sin
aritmética aproximada adicional, desde las mismas fuentes congeladas del render
de Windows. Ambos MIDI se vuelven a renderizar completos en la Pi, desde el
principio y con la misma preparación de 200 ms, eventos, pedales y colas.

| Pieza | Duración del audio | Tiempo ARM64 | SHA256 del crudo estéreo f32, idéntico a Windows |
| --- | ---: | ---: | --- |
| Nocturno | 232,564 s | 118,663 s | `940e6fc7e951004b7dd4097fa9ecfcac362c044169de7e6908be249886985355` |
| Campanella | 280,159 s | 122,512 s | `7a4a81813117303279b5d61a1274d5e2d42e05766961474cdbccc13eeb837955` |

La igualdad abarca **todas las muestras** de cada pieza. Estos tiempos son
promedios de un render sin reloj; por sí solos no prueban que cada bloque
llegue a tiempo durante la reproducción.

`rackforge-core compare-native --seconds 4` comprueba además los 16 programas:
WASM y biblioteca ARM64, ejecución serial y repartida entre cuatro instancias,
audio y estado iguales. Esa prueba usa los programas de fábrica con sus valores
compilados, sin el ajuste de contacto de 4 ms; la igualdad de las piezas completas
anteriores sí incluye ese ajuste.

- [Renderizados completos y compilación ARM64](arm64-proof.json).
- [Carga y equivalencia del runtime, 16 programas](runtime-proof.json).
- [Validación del paquete y hash del archivo](package-proof.json).

## Instalación y perfil

Conexión verificada: `ssh rackforge-pi` (también `ssh ssh-raspberry`).
Raíz de la instalación: `/home/kalex/rackforge`.
Paquete nuevo: `plugin-store/packages/org.rackforge.concert-grand/0.171.36`.
El paquete contiene WASM y `native/linux-aarch64/librackforge_concert_grand.so`;
se instala mediante `rackforge-store install-local ... --official` y el journal
confirma `PLUGIN_NATIVE_BUILD ... version=0.171.36 platform=linux-aarch64`.

Core y Web conservan la release `91eb43f`; se actualiza el plugin. La biblioteca
portable nativa no acepta el presupuesto de combustible del runtime WASM:
`InstanceBackend::Library(_) => false` en
`crates/rackforge-plugin-runtime/src/portable.rs`. Por tanto esta instalación
ejecuta el modelo completo, con cuatro workers, sin reducción adaptativa de
parciales, voces o respuesta no lineal. No se modifica el host para lograrlo.

Se selecciona Concert en lugar del anterior Concert 280. Los **200 parámetros
públicos**, incluido el preamplificador en el índice 200 (199 no existe), son
numéricamente idénticos al perfil del render. Se guarda también el preset de
usuario **Concert renders 4ms**. El ajuste se conserva en el checkpoint de
parámetros y se verifica tras reiniciar el servicio de audio. El writer tarda
1,5 s de inactividad en volcarlo: la comprobación espera ese volcado antes del
reinicio. No se cambia el valor compilado de `MERGE_RAMP_S`, porque también
define la interpretación de estados normalizados existentes.

Salida: Scarlett Solo USB, estéreo S32LE, **44.100 Hz / 256 / 768**; período
de 5,805 ms y cola nominal de 17,415 ms. La cola nominal no es una medición de
latencia completa desde el teclado al conversor. Master 1000, panorama central,
modo PLAY y Concert seleccionado.

Los respaldos previos están en
`/home/kalex/rackforge-deploy/concert-grand-20261014-4ms/backup`:
configuración de audio, sesión, parámetros persistentes, activación del store,
presupuesto previo y datos de usuario del piano si existían. El paquete 0.171.35
permanece instalado. El archivo nuevo y las fuentes congeladas se conservan en
el directorio de despliegue; no se publica una release ni se hace push.

- [Archivos instalados, configuración y salud](installation-proof.json).
- [Perfil del render, 200 parámetros](render-profile.json).
- [Igualdad exacta tras reiniciar](profile-persistence-proof.json).

## Pruebas con reloj de audio

`tools/measure-appliance-midi.py` reproduce el plan completo de eventos del
render mediante un cliente MIDI virtual del Core, conservando los tiempos y
velocidades. Mide el journal desde antes del primer evento hasta después de la
cola y libera sólo su propio cliente al terminar. Verifica el perfil antes y
después. Registra bloques fuera de plazo, fallos, xruns, clipping, eventos
descartados y cualquier cambio de presupuesto de calidad.

Esta prueba incluye la planificación y el IPC del MIDI virtual; sus tiempos
pueden diferir de la cuantización de eventos de un render offline. La igualdad
binaria demostrada arriba corresponde al DSP, no a una captura analógica de
la Scarlett ni a la latencia física del teclado.

| Pieza completa | Eventos aceptados | Bloques medidos | Media por bloque | Máximo | Fuera de plazo |
| --- | ---: | ---: | ---: | ---: | ---: |
| Campanella | 11.381 / 11.381 | 48.792 | 1,676 ms | 5,337 ms | 0 |
| Nocturno | 6.027 / 6.027 | 40.517 | 1,783 ms | 3,724 ms | 0 |

En ambas pruebas: **cero xruns, clipping, eventos descartados, fallos de
procesamiento y cambios de presupuesto de calidad**. Se revisan los journals
completos, incluidos fallos de unidad, cuarentenas y ataques iniciales. Los
parámetros siguen exactamente iguales al terminar. La tardanza p99 al enviar
MIDI es 1,597 ms en Campanella y 1,163 ms en Nocturno; sus máximos son 7,298 ms
y 3,926 ms respectivamente. Esta variación de envío explica por qué no se
atribuye igualdad de muestras a la reproducción con MIDI virtual.

El contador `midi_events` del medidor observa sólo el bloque vigente una vez
por segundo; no cuenta todos los eventos recibidos. Se informan por separado
los eventos aceptados por el socket y los descartes del motor. Los bloques de
telemetría incluyen el margen previo y posterior al playback. Los journals
íntegros quedan en el directorio remoto del despliegue y en
`local/concert-grand-pi-2026-10-14` en Windows.

Son pruebas de estas dos piezas en esta máquina. El pico más alto de
Campanella deja aproximadamente 0,468 ms antes del plazo; no equivale a una
garantía para cualquier carga adicional. El búfer sigue en 256/768 y el modelo
conserva la misma física que los renders.

- [Campanella completa en vivo](campanella-live-256.json).
- [Nocturno completo en vivo](nocturno-live-256.json).
- [Estado final listo para tocar](ready-proof.json).
