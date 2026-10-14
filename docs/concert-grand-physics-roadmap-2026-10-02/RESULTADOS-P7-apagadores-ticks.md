# Apagadores y notas cortadas — Concert Grand

Fecha: 2026-10-14.

El usuario rechaza la escucha de la primera mitad de La Campanella: «el 90%
se escucha como ticks cortados». Esa valoración sustituye cualquier aceptación
implícita por haber conservado eventos o superado controles de archivo. El
WAV anterior se conserva como control del fallo, en su ruta original y con
sus fuentes exactas en **8cff4497** / **d736c9c9**.

## Fallo reproducido y corrección

La voz calculaba el levantamiento del apagador durante los 30 ms posteriores
al note-on. Pero ese note-on ya lanza el martillo contra las cuerdas: el
levantamiento por la tecla debe estar completado al golpear. Con pedal bajo,
la cuerda recién excitada quedaba frenada durante sus primeros milisegundos.
La dependencia de la masa modal hace especialmente fuerte el efecto en las
cuerdas cortas.

Una prueba agrega 40 ms de silencio frío antes de tocar exactamente la misma
nota mantenida, con pedal en cero. El código anterior da una fuerza distinta
porque sólo trataba como asentado el apagador del ataque en el frame cero.
La prueba falla antes del arreglo y pasa después: la fuerza de los primeros
100 ms, alineada por el ataque, coincide con tolerancia 1e−12 relativa y
1e−14 absoluta. No se cambian la velocidad ni el tiempo de la nota.

Se completa el levantamiento en el golpe; el retorno del apagador y los
cambios del pedal conservan su rampa de 30 ms. Las posiciones y velocidades
de las cuerdas no se reinician en las reiteraciones.

El prototipo también aplicaba apagadores hasta la última tecla. La ausencia
de apagadores en el extremo agudo está descrita por
[Yamaha](https://hub.yamaha.com/pianos/p-acoustic/how-does-a-piano-work/) y
[Steinway](https://www.steinway.com/steinway.com-japan/en/dam/jcr%3Ac94babcc-fa76-4b8c-b1a6-8e8dac935aac/Steinway%20%26%20Sons%20Buyers%20Guide.pdf).
Se incorpora un límite configurable `--undamped-from`, con **89 (F6) como
valor provisional del piloto**. Las referencias justifican el mecanismo,
no ese límite exacto, que aún no está calibrado para un instrumento concreto.
`--undamped-from 109` conserva apagadores en las 88 teclas para controles.
Las pérdidas naturales permanecen activas en todo el registro.

Una segunda prueba verifica que una cuerda sin apagador tiene exactamente
la misma trayectoria al adelantar el note-off y mover CC64. Se conservan
también las pruebas previas de contacto, reiteraciones, presión, filtros,
entrada ligera y reposo: **nueve controles Python pasan** en 14,328 s.

## Escucha de control

Se vuelve a calcular desde el estado frío el comienzo original hasta 35 s,
incluida una zona con cortes notorios en la fuerza de las voces. Se mantienen
el MIDI y sus pedales, ocho modos no lineales, todos los portadores lineales,
el reloj fino del contacto, el máximo de 64 kHz en vuelo y la misma tabla
unidireccional. Esta es una comprobación de la corrección antes de volver
a invertir en una escucha de media obra.

El WAV nuevo dura **37,1 s**: 35 s de música, dos segundos de cola y 100 ms
de silencio inicial. Incluye **310 golpes y 27 alturas**. El cálculo tarda
**272,873 s**, o **279,339 s** con preparación (4 min 39,3 s). No hay fallos
del solver ni reintentos. Los dos agudos sin apagador presentes son MIDI
92 y 99; no se eliminan voces ni se alteran eventos para obtener el resultado.

Se usa la misma ganancia uniforme de **0,25 / Pa** del control anterior.
El pico es 0,485052 y el RMS 0,026328, sin saturación. Los primeros ocho
segundos de música, más su silencio inicial, son **idénticos bit a bit** al
WAV previo: el pedal levantado ya evitaba el fallo en ese comienzo. Por eso
la escucha inicial de ocho segundos no había servido para descubrirlo.

La comparación por altura mide RMS entre 40–80 ms después de cada ataque,
dividido por RMS de los primeros 0–20 ms. Se excluyen ventanas con otra
reiteración de esa altura; en este corte entran las 310 notas. Esta razón
describe fuerza de puente, no presión, calidad audible ni porcentaje de
ticks percibidos por el usuario.

| Grupo | Notas | Mediana antes / después | Colas <1 % del ataque, antes / después |
| --- | ---: | ---: | ---: |
| Todas | 310 | 0,1403 / 0,4236 | 66 / 0 |
| CC64 <40 a los 50 ms | 179 | 0,0543 / 0,3610 | 56 / 0 |
| Registro sin apagadores | 60 | 0,00489 / 0,07436 | 35 / 0 |
| Tecla mantenida al menos 90 ms | 44 | 0,5144 / 0,9513 | 1 / 0 |

No hay ajuste de ganancia, de alineamiento ni de duración de las notas.
La mejora cuantitativa corresponde a las dos correcciones juntas; las pruebas
de invariancia aíslan sus mecanismos por separado. La decisión de escucha
del WAV corregido sigue pendiente. No se repite automáticamente la media
obra antes de evaluar este pasaje.

El WAV se guarda únicamente en
`C:/SHARED/Piano-Campanella-apagadores-2026-10-14/Campanella-hibrido-NL8.wav`.
El RMS de los últimos 100 ms es 1,07e−7 y la última muestra tiene magnitud
máxima 2,08e−7, sin fade añadido. Los estados y fuerzas de trabajo quedan
en `local/concert-grand-physics/P7-Campanella-apagadores-2026-10-14/`.

- [Render y fuentes medidas](medidas-P7-cuerdas-nativas-2026-10-14/campanella-apagadores-35s.json).
- [Verificación de MIDI, registro de apagadores y WAV](medidas-P7-cuerdas-nativas-2026-10-14/verificacion-campanella-apagadores-35s.json).
- [Comparación reproducible de fuerza y ganancia](medidas-P7-cuerdas-nativas-2026-10-14/comparacion-apagadores-35s.json).
- Herramienta de comparación: `tools/measure-concert-grand-tick-correction.py`.
