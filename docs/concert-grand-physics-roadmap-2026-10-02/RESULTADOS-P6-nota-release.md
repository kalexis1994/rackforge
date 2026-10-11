# P6 — Una nota completa con apagador y escucha

Fecha: 2026-10-10. La etapa de las dos fuerzas se guarda en `2d106cf8`.

## Propósito

Volver a una escucha concreta después de los controles cortos de integración.
Una C4 a velocidad MIDI 84, sostenida 160 ms, sin pedal, desde reposo.
Tres cuerdas por tecla, nueve cuerdas vivas, una tabla de 606 coordenadas y
una memoria acústica común. No es una nota sintetizada por muestras ni una
suma de renders independientes.

Se usa el splitting actual: contacto con portadoras a 256 kHz, coast a
128 kHz, observación de desplazamiento/velocidad de tabla a 128 kHz,
presión con el mismo Green retardado y WAV FLOAT32 estéreo a 48 kHz.
Ganancia fija 0,25 por Pa y silencio inicial de 100 ms. El plugin instalado
y los programas de fábrica no cambian en este ensayo.

La tecla se suelta a 160 ms. La tracción viscosa distribuida del apagador
desciende con su rampa de 30 ms dentro de las ecuaciones; los otros dos
apagadores parten cerrados. No se ponen estados a cero ni se aplica fade
de salida, EQ, normalización por nota o reverberación añadida.

## Duración de la cola

Primera trayectoria de 400 ms, con 240 ms después del note-off. Se reconstruye
presión y se compara RMS de los últimos 20 ms con RMS sostenido de 50–150 ms.
Si la cola supera el 1 % (−40 dB), se prolonga la misma trayectoria viva en
tramos de 200 ms, hasta 800 ms físicos. Ese límite y cualquier fallo de cola
se registran; no se ocultan con un corte o fade artificial.

El umbral de cola es un criterio de terminación del audio diagnóstico,
no una identificación del tiempo de decaimiento del Steinway. Los micrófonos
conservan su propio tiempo de propagación y no se eliminan del WAV.

## Continuidad y selección de solvers

Se reutiliza el productor congelado `concert_grand_integrated_phrase.trajectory`.
Contacto 256/coast 128 evita que la condición `step==1` envíe todo el sostenido
al solver de contacto. Los contadores verifican la duración integrada.

`NoteTrace` observa el estado aceptado después de la segunda mitad del
apagador. Una tentativa de coast rechazada por recontacto no avanza el reloj
ni produce checkpoint. Copias sólo en RAM a 160, 184, 192 y 400 ms; no se
guardan nuevas cápsulas al disco. Las fuentes de ensayos anteriores siguen
intactas y verificadas por SHA256.

Una prueba de integración pasa en **13,932 s**: estados y trayectoria
observados idénticos al productor original, contacto y coast utilizados,
contadores correctos, pérdidas positivas y reproducción independiente del
release desde un checkpoint aceptado. No se cambian ecuaciones para observarlas.

## Control temporal del release

Desde el estado real a 184 ms se refinan los siguientes 8 ms a 64/128/256 kHz,
cruzando el final de la rampa de apagador (190 ms). Misma geometría, rampa
absoluta, fuerzas, memoria y micrófonos. La reproducción a 128 se compara
contra el estado y la trayectoria del render completo, antes de interpretar
las diferencias con 256. Presión por bandas con ventanas rectangular y Hann,
sin alineación de fase o ganancia. No sustituye convergencia de toda la cola.

Se compara además el primer prefijo de 120 ms de nota contra el WAV físico
anterior a coast 16 kHz, antes de ambos note-offs. Es un contraste temporal
con los mismos eventos; no un ajuste ni una medida de cercanía al Steinway.

## Evidencia y resultado

Terminado y verificado. Informe primario en
`medidas-P6-nota-release-2026-10-10/nota.json`; espectros locales en
`release.npz` (478.469 bytes), cierre independiente en `verificacion.json`.
Audio únicamente en
`C:/SHARED/Piano-nota-release-2026-10-10/C4-84-sin-pedal-128k.wav`.

Trayectoria: **1908,177 s (31,80 min)**. Preparación, reconstrucción y
controles incluidos: **2193,724 s (36,56 min)**. Hay 412 pasos de contacto
y 50.994 de coast, sin reintentos por recontacto; 412 + 2×50.994 = 102.400
ticks a 256 kHz. El apagador permite terminar a 400 ms físicos, sin extensiones.

WAV: **48 kHz, FLOAT32 estéreo, 24.167 frames / 0,503479 s**, incluido el
silencio inicial y la propagación. Pico **0,269022**, finito y sin clipping.
RMS sostenido **0,104462**; últimos 20 ms **0,000696104**, cociente
**0,00666372 (aprox. −43,5 dB)**. Pasa el criterio de cola de −40 dB.
No se identifica a partir de esto el release del piano de referencia.

Trabajo externo del martillo: 0,00777164244 J. Cierre mecánico incluyendo
fieltro, material, apagador y trabajo del puerto acústico: residual
**7,84312e−15 J / 1,00920e−12 relativo**. El ledger radiativo completo sigue
pendiente. El estado mecánico final tiene 0,00367758 J; que el micrófono
quede casi silencioso no significa que toda la energía interna sea cero.

La reproducción del tramo aceptado a 128 kHz da **1,84015e−14** en estados
y **9,93451e−15** en trayectoria. Los controles desde ese mismo checkpoint:

| Tasa | Tiempo (s) | Máximo de banda rectangular | Máximo de banda Hann | Estado onda frente a 256 |
| --- | ---: | ---: | ---: | ---: |
| 64 kHz | 30,226 | 0,451853 % | 263,445 % | 0,556768 % |
| 128 kHz | 62,324 | 0,089322 % | 51,7578 % | 0,111228 % |
| 256 kHz | 111,486 | referencia | referencia | referencia |

**64 y 128 fallan el gate de 1 %**, por Hann en 6–12 kHz. En 128, la banda
2,5–6 kHz Hann da 0,461479 %; en 64 también falla con 2,33097 %.
La energía de referencia Hann 6–12 kHz es sólo **3,16977e−10** de la energía
total de esta ventana: el error absoluto alto de 128 relativo al total es
**9,21489e−6**, y el error total Hann **3,96491e−5**. Se conservan tanto el
fallo relativo como su contribución absoluta; no se declaran inaudibles.
Comparar la referencia de 256 consigo misma no valida esa tasa.

El cierre vuelve a integrar independientemente las bandas del archivo,
autentica las 17 fuentes, tabla y R, verifica formato/hash/cola del WAV y
la aritmética del ledger. Mallas espectrales anidadas 12,5/6,25 Hz:
Hann pasa (energía 0,000616103 relativo, cambio máximo del error 9,18336e−5).
Rectangular **falla** energía con **0,00124540** frente al límite 0,001;
los cambios de error son pequeños (máximo 2,23077e−7), pero no sustituyen
ese gate. El oracle Hermite independiente de esta nueva trayectoria no se
ha medido: los espectros guardados no permiten reconstruir la fuente.

Frente al primer prefijo del audio anterior a coast 16 kHz: diferencia
global de forma de onda **0,683605 %**. Las bandas 800–2500, 2500–6000 y
6000–12000 Hz difieren **2,61643 / 13,6272 / 2,23677 %**; no pasan todas el
1 %. Esto no mide cercanía al Steinway ni certifica la nota completa.

Fuentes nuevas: `concert_grand_note_trace.py`,
`render-concert-grand-note-release.py`, `verify-concert-grand-note-release.py`,
`tests/test_concert_note_trace.py`.
No duplicar informes, fuentes, R o cápsulas en SHARED.

### Limpieza solicitada durante el render

El usuario pide liberar espacio. Borrado recursivo por lotes y borrado
directo de `target/release/deps` rechazados automáticamente por política,
sin motivo más específico. No se ejecutaron esas órdenes. Se usa la
limpieza de paquetes propios de Cargo, con dry-run previo:
`cargo clean --workspace --release --offline`.

Cargo informa **1231 archivos / 393,1 MiB eliminados**. La unidad pasa de
**825.131.008** a **16.369.975.296 bytes libres** en las lecturas de esta
sesión; no se atribuye toda esa variación a los 393,1 MiB de Cargo, pues
puede incluir otras operaciones o gestión del sistema. No se borran fuentes,
grabaciones originales, la R activa ni comparaciones de audio reutilizadas.
El dry-run de Git sobre subcarpetas ignoradas proponía borrar `target/`
completo; se descarta esa vía para evitar afectar otros datos de la aplicación.

## Límites

Escucha diagnóstica. La reducción acústica espacial sigue fallando su criterio
del 5 % con 9,2021 %; los coeficientes mecánicos, el fieltro y los apagadores
siguen siendo supuestos. No se demuestra convergencia temporal de toda esta
nota, todo el teclado o una pieza polifónica. La conservación mecánica y la
pasividad no implican identificación del piano ni mayor fidelidad al Steinway.

Después de escuchar sostenido/release de esta nota: frase corta y ampliación
con F4. Mantener pendientes el control rectangular de malla y la discrepancia
Hann alta durante la rampa; no aprobar convergencia por la energía pequeña.
No encadenar otros cambios de integrador sin una ventaja concreta.
