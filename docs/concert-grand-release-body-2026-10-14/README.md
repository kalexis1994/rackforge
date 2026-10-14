# Cuerpo y sostenido sobre la release publicada

**Estado actual:** el usuario pide retirar el ajuste local del fundamental
en MIDI 58–62 y trabajar en una corrección general del acoplamiento. Se elimina
por completo esa selección de notas y se restaura la base aceptada con el
residuo agudo. Los candidatos de cuerpo y sus audios siguientes quedan como
historial de ensayos, no como el modelo que se continúa. La retirada recupera
exactamente las huellas `e74df30116b39a5a` y `9b9bd2cbb32452f0`.
Se implementa después una [corrección general del puerto de acoplamiento](general-coupling.md).
Las pruebas físicas y numéricas pasan; el efecto acústico medido es mixto.
No se presenta como una mejora ya conseguida de cuerpo en todo el teclado.
El usuario escucha esta corrección y responde «Suena bien, sigamos»:
`7d7546e5` pasa a ser la base aceptada para las comparaciones siguientes.

2026-10-14. El usuario confirma que quiere continuar desde el piano incluido
en la release y prioriza **cuerpo y sostenido**. Base: RackForge v0.1.32,
Concert Grand **0.171.35**, commit `91eb43f88495f0485e7a7ed61718280ddce23a8d`.
La rama 0.172.0 y el prototipo SI se conservan como investigación.

## Decisión de trabajo

La última escucha del prototipo mejoró algo los ticks, pero sigue muy lejos
del sonido de la release según el usuario. La nueva base es el instrumento
publicado. Se mantienen su ataque, captación, programas y funcionamiento;
se investiga una debilidad concreta por vez y se compara su audio con esa base.

El error de proceso fue llegar tarde a esa comparación musical. Conservar
energía, superar pruebas numéricas y encontrar similares dos reducciones de
un fragmento corto no demostraba una mejora sobre el instrumento aceptado.
Se conservan los controles, solvers y aprendizajes de la investigación, sin
trasladar sus supuestos geométricos ni su calibración completa al piano actual.

La revisión completa se conserva en la rama `codex/concert-grand-restrike`,
commit **790335e9**, documento
`docs/concert-grand-physics-roadmap-2026-10-02/REVISION-release-y-aprendizajes-2026-10-14.md`.
Allí también están la identidad del snapshot publicado, seis pruebas sobre
él, las dos huellas distintas de 0.172.0 y el historial de las escuchas rechazadas.

## Qué se mide ahora

Se compila como biblioteca el **código de producción sin modificar** de
0.171.35, con su SDK del mismo tag. Un consumidor externo llama a sus métodos
públicos. No sustituye el motor por el nuevo solver ni por notas muestreadas.
El programa Concert se carga explícitamente; 44,1 kHz, bloques de 256,
200 ms de preparación silenciosa, tecla mantenida dos segundos y salida
de cuatro segundos. Doce casos: C3/A3/C4/D#4/C5/C6, velocidades 60 y 110.

Se vuelve a calcular también el modelo local 0.172.0 con su ejecutable
congelado autenticado. Se verifican las grabaciones originales y los metadatos
del corpus Steinway ORTF ya utilizado: nota raíz exacta, sin transposición.
Este corpus conocido no es una validación nueva ni ciega, y sus envolventes
no identifican por sí solas materiales, pérdidas o fase del instrumento real.

Se mide presión estéreo completa, usando ambas cápsulas para evitar
cancelación mono. La alineación por inicio se aplica sólo al análisis;
los archivos crudos conservan todas sus muestras, niveles y retardos.
No se ajusta ganancia por nota ni se ecualiza. La comparación de caída es
relativa al ataque de cada señal: no supone que ambas versiones o la
grabación tengan la misma calibración absoluta.

## Primera localización del problema

Caída del fundamental entre el nivel medio de 100–200 ms y el de
500–1400 ms después del inicio acústico. Valores en dB, velocidad **60**:

| Nota | Release 0.171.35 | Investigación 0.172.0 | Referencia |
| --- | ---: | ---: | ---: |
| C3 | −16,46 | −9,95 | −10,80 |
| A3 | −3,76 | −10,35 | −19,46 |
| C4 | −16,09 | −21,69 | −5,78 |
| D#4 | −22,20 | −13,34 | −16,87 |
| C5 | −14,01 | −10,34 | −13,20 |
| C6 | −19,76 | −21,61 | −16,85 |

La velocidad 110 conserva casi el mismo patrón en el registro medio.
El déficit de sostenido de C4 **sí aparece en la release**, pero es menor
que en 0.172.0. A3 presenta el problema opuesto frente a esta referencia,
y C5 tiene una caída total parecida. No corresponde alargar todo el piano.
Estos datos describen H1, no un porcentaje de calidad audible ni todo el cuerpo.

En C4/60, las pendientes locales de la release son −16,70 / −19,07 / −32,33
dB/s en 150–400 / 450–800 / 900–1400 ms. La referencia da −9,62 / −6,59 /
−7,21 dB/s. El dato tardío aún no separa pérdidas y cancelación del unísono.
Otras notas muestran recuperaciones de nivel: D#4 y C5 tienen pendientes
positivas en la última ventana. No convertir esas envolventes automáticamente
en constantes de pérdida ni afirmar que el modelo genera energía.

## Primer control: tabla de caída inicial

La release usa `PROMPT_MEASURED_POWER=1`, que hace seguir al fundamental una
tabla empírica de caída inicial. Se prueba **sólo** pasar ese valor a cero,
manteniendo contacto, detune, pérdidas, captación y niveles restantes.
Es una ablación de diagnóstico, no una calibración seleccionada.

| Nota, velocidad 60 | Caída base | Tabla desactivada | Referencia |
| --- | ---: | ---: | ---: |
| C3 | −16,46 | −3,88 | −10,80 |
| A3 | −3,76 | −7,93 | −19,46 |
| C4 | −16,09 | −17,73 | −5,78 |
| D#4 | −22,20 | −22,27 | −16,87 |
| C5 | −14,01 | −28,53 | −13,20 |
| C6 | −19,76 | −19,76 | −16,85 |

**Se rechaza quitar globalmente la tabla:** no recupera el sostenido de C4
y empeora mucho C5. La tabla tampoco explica por sí sola el déficit de C4.
No se modifica ningún coeficiente de fábrica, no se instala un paquete y
no se presenta el control como una mejora audible.

## Verificación y siguiente paso

El consumidor de producción reproduce exactamente la huella publicada
`e74df30116b39a5a` al usar la inicialización del control de CI. Para ese control
se omiten explícitamente la recarga de preset y los 200 ms de preparación
diagnóstica: comparar las dos inicializaciones sin igualarlas había dado una
huella distinta. No se alteró la biblioteca ni el consumidor que generó las
doce mediciones para superar la comprobación.

El medidor de envolventes pasa un control analítico: tono exponencial de
440 Hz, cápsulas en oposición de fase y pendiente conocida −6,080123 dB/s.
El error máximo es **7,99e−7 dB/s**. Todos los renders nuevos son finitos;
fuentes, eventos, binarios, referencias y audios crudos quedan autenticados.
El banco comparado completo tarda **9,73 s** en esta máquina, incluida la
ejecución de ambas versiones y el análisis; la ablación tarda unos cinco
segundos. Estos tiempos no son pruebas de reserva de CPU del plugin WASM.

El siguiente ensayo conserva la release y separa en C4 las magnitudes de
los estados del unísono y su suma proyectada, antes y después del receptor.
Esas magnitudes heredadas no son julios. Se busca distinguir pérdida real
de amplitud y cancelación, con C3/A3/D#4/C5 como controles, antes de elegir
un cambio local de la transición hacia el sostenido. El ataque seguirá
comparándose por separado y la selección final será por escucha.

- [Medición de la base y procedencia](baseline.json).
- [Control rechazado de tabla desactivada](prompt-table-off.json).
- [Identidad del consumidor con el piano publicado](probe-verification.json).
- `tools/concert-grand-release-probe.rs`: consumidor del motor original.
- `tools/measure-concert-grand-release-body.py`: banco y control analítico.
- `tools/audit-concert-grand-release-prompt.py`: ablación de un coeficiente.
- `tools/verify-concert-grand-release-probe.py`: comprobación de la huella.

## La Campanella completa y limpieza de ensayos anteriores

A pedido del usuario se eliminaron **14.987 archivos generados**, que sumaban
33.605.638.126 bytes (**31,30 GiB**): audios de ensayos anteriores, cachés
numéricas y binarios de diagnóstico. Se conservaron código, MIDI, informes,
mediciones CSV, grabaciones originales en `local/reference-audio` y el entorno
Python. Los audios crudos y referencias derivadas de los bancos anteriores ya
no están disponibles; sus informes y hashes quedan como registro histórico.
El [resumen de limpieza](cleanup-2026-10-14.json) identifica las extensiones,
el espacio liberado y el inventario completo conservado en `local/`.

Se generó un nuevo audio de **referencia de la release 0.171.35**, sin aplicar
todavía una mejora de cuerpo/sostenido. El MIDI completo dura 275,159 s y el
WAV añade cinco segundos de cola: **280,159 s**. El proceso nativo tardó
**25,85 s** en esta máquina, unas 10,84 veces más rápido que tiempo real.
Esto mide el render nativo offline, no el margen de CPU del plugin WASM.

Audio para escuchar:
`C:/SHARED/Piano-Campanella-release-2026-10-14/Campanella-release-0.171.35.wav`.

El nuevo render procesa las 4.249 pulsaciones y sus 4.249 liberaciones en una
sola instancia continua del piano de producción. Conserva tempo, orden,
velocidades y valores continuos de pedal del MIDI original; el programa 0
queda seleccionado mediante el preset Concert. Salida estéreo float32 a
44.100 Hz, bloques de 256 muestras, ganancia de exportación 1, sin
normalización adicional. Pico **0,92546**; RMS de los últimos 100 ms
**6,13e−12**, por lo que la cola final ya se extinguió.

Se comprobó la igualdad exacta entre muestras crudas y WAV, ausencia de
valores no finitos y saturación, fuentes de producción idénticas a `v0.1.32`,
y reproducción de la huella publicada `e74df30116b39a5a` con la inicialización
de CI. El lector directo del MIDI coincide en los 11.382 eventos con el plan
anterior; la diferencia temporal máxima de 1,302 microsegundos corresponde a
la cuantización a 256 kHz de aquel plan. El render nuevo usa tiempos racionales
del MIDI original y redondea una sola vez a muestras de audio.

- [Procedencia, tiempos y comprobaciones del render](campanella-baseline.json).
- [Huella de la biblioteca recompilada para este render](campanella-probe-verification.json).
- `tools/render-concert-grand-release-midi.py`: lector MIDI y render completo.
- Nuevos datos de trabajo en `local/concert-grand-release-campanella-2026-10-14/`.

Este WAV será la versión A para comparar futuros cambios locales de cuerpo
y sostenido con el mismo MIDI, preset y nivel de exportación.

## Separación mínima del unísono agudo: aceptada por escucha

El usuario propone conservar pequeñas diferencias de afinación entre las
cuerdas. La release ya las tiene en el registro medio, pero las reduce a cero
en la última octava para evitar una oscilación que molestaba. Se conserva esa
reducción y se añade un residuo nominal de **0,02 cents**, con la misma
variación fija por nota/parcial y los controles Unison y de laboratorio de
detune. Es un candidato conservador para escuchar, no una tolerancia de
afinación medida en un piano real ni una mejora audible ya confirmada.

El residuo entra progresivamente con `top_octave`, desde la zona de G6 hacia
C7. No agrega cuerdas, parciales, estados ni operaciones por muestra: sólo
ajusta las frecuencias al crear la nota. El registro medio sigue usando las
frecuencias de la release. Las pruebas leen las rotaciones de los osciladores:
la separación entre extremos del fundamental es 0,0498 / 0,0205 / 0,0232 /
0,0342 Hz en C7/E7/G7/C8. Con Unison en cero, la separación vuelve a cero.

La Campanella completa B conserva exactamente los eventos, preset, duración
y ganancia de exportación de A. Tarda **26,12 s**, dura **280,159 s** y alcanza
un pico de **0,92524**. Salida:
`C:/SHARED/Piano-Campanella-unisonos-2026-10-14/escucha/Campanella-B-unisonos-agudos.wav`.
Los WAV `Notas-A-release.wav` y `Notas-B-unisonos-agudos.wav` de esa misma
carpeta permiten escuchar notas aisladas en orden MIDI
60/84/90/91/96/100/103/108, velocidad 96, tres segundos por nota y 250 ms de
silencio entre ellas. Los primeros tres casos coinciden bit a bit; los demás
cambian. En las ventanas medidas de 50 ms a 2 s, el mayor cambio absoluto de
nivel de estas notas es **0,117 dB**. Esto no demuestra una mejora de timbre.

Una comparación alternada de tres ejecuciones por versión, con un guion
nativo de 14 s, da medianas de **0,6956 s** para A y **0,7021 s** para B
(+0,93%). Los rangos se solapan; no se aprecia un costo relevante en esta
prueba. Incluye escritura del audio crudo y no mide plazos de audio WASM.

Pasan ocho comprobaciones: separación real y control a cero, dos huellas de
audio, ausencia de apagadores agudos, dos controles de medio pedal,
repetición sobre la misma cuerda y desfase del unísono. La huella del guion
grave/medio permanece `e74df30116b39a5a`. La del guion con pedales, que incluye
notas agudas, cambia intencionalmente a `9b9bd2cbb32452f0`; poner sólo el nuevo
residuo a cero en una fuente temporal restaura exactamente ambas huellas de
la release, incluida `8d20f899b0509bd0` con pedales.

- [Audios, comparación por nota, tiempos y procedencia](top-unison-comparison.json).
- [Ocho comprobaciones del candidato](top-unison-tests.json).
- [Control de reversión a las huellas publicadas](top-unison-zero-control.json).
- `tools/compare-concert-grand-top-unison.py`: comparación reproducible.

El usuario escucha la Campanella B y confirma: «No se nota mucho, pero si no
agrega carga dejemoslo porque es mas real. Yo lo escucho muy bien». Se acepta
conservar el ajuste en la rama de trabajo, commit `d55f8f50`. La justificación
de costo es que no agrega osciladores ni trabajo por muestra y los tiempos
medidos se solapan; no se afirma un costo total exactamente nulo. La escucha
no establece una mejora perceptual grande ni valida una tolerancia física
real. No se publicó ni se instaló un paquete.

## Diagnóstico del cuerpo de C4: pérdida y cancelación

Se añade una exportación nativa `cfg(test)` que observa los estados de H1
después de cada bloque, sin cambiar el procesamiento. En veinte casos
(C3/A3/A#3/B3/C4/C#4/D4/D#4/C5/C6, velocidades 60 y 110), y seis controles de
afinación, el audio coincide bit a bit con el consumidor nativo independiente
de la base aceptada. Se recuperan también las doce medidas históricas de
la release en las notas comunes. Los cuadrados de magnitudes internas usan
unidades arbitrarias del modelo: **no son julios**.

En C4/v60, entre 100–200 y 500–1400 ms, la norma cuadrática conjunta vertical
y horizontal cae **7,74 dB**, su suma proyectada cae **15,99 dB**, y H1 en el
audio estéreo final cae **16,09 dB**. La suma de las tres verticales cae
20,22 dB: 7,95 dB corresponden a su norma conjunta y 12,27 dB al cambio de
coherencia. La horizontal atenúa parte de esa cancelación en la proyección
completa. La lectura aislada de una envolvente acústica no podía separar esto.

Se varía únicamente la separación del unísono en procesos independientes:

| Separación respecto de fábrica, C4/v60 | Caída de norma vertical + horizontal | Caída de H1 en salida |
| --- | ---: | ---: |
| 70% | −8,72 dB | −13,32 dB |
| 85% | −8,21 dB | −14,47 dB |
| 100%, base | −7,74 dB | −16,09 dB |
| 115% | −7,31 dB | −18,16 dB |

Reducir la separación conserva **más nivel acústico aun con una norma interna
que cae más**. Eso identifica una contribución de cancelación a esta caída;
no demuestra que toda cancelación sea un error ni que falte sólo energía.
El patrón se repite a velocidad 110. La variante al 70% cambia el nivel RMS
de los primeros 100 ms sólo +0,063 dB y recupera unos 2,76 dB en la caída de H1,
pero todavía queda lejos de los −5,78 dB de la referencia. No se selecciona
una reducción global del unísono.

- [Trazas, audio independiente, descomposición y controles](body-state-diagnosis.json).
- `tools/audit-concert-grand-release-body-states.py`: reproducción y análisis.
- `tests::export_release_body_trace`: exportador nativo ignorado por defecto.

El siguiente candidato aplica una reducción suave y local de separación
alrededor de C4, con A3 y D#4 fuera del ajuste. Se conserva la excitación,
la captación y las leyes de pérdida; se comprobarán ataque, notas vecinas,
controles fuera de la zona y La Campanella antes de seleccionar por escucha.

## Candidato de cuerpo: corrección limitada al fundamental

La primera prueba redujo la separación de **todos** los parciales de las
notas afectadas. Aunque recuperaba H1, empeoraba la caída de H2 en C4 unos
6,1 dB frente a la base; **se rechaza**. Sus medidas y la diferencia exacta
de fuente respecto del commit de diagnóstico quedan en
[body-all-partials-rejected.json](body-all-partials-rejected.json).
Que sus pruebas numéricas pasaran no bastaba para seleccionarla.

El candidato final usa la afinación independiente por parcial que ya tiene
el motor y estrecha **sólo el grupo del fundamental**, en las notas MIDI
58/59/60/61/62 (A#3/B3/C4/C#4/D4), con factores 0,9/0,8/0,7/0,8/0,9.
Fuera de esa zona se conserva la base aceptada, incluido el nuevo residuo
agudo. Es un ajuste local de voicing dentro de la representación existente,
no una identificación de la tensión o de la geometría de un piano real.
No se alarga globalmente ninguna ley de pérdida ni se cambia el martillo,
el receptor, las ganancias o las rutas auxiliares.

| C4, caída de H1 entre 100–200 y 500–1400 ms | Base aceptada | Candidato | Referencia conocida |
| --- | ---: | ---: | ---: |
| Velocidad 60 | −16,09 dB | −13,32 dB | −5,78 dB |
| Velocidad 110 | −16,11 dB | −13,33 dB | −6,64 dB |

Se recuperan **2,76–2,78 dB** en esta medida de caída del fundamental; es un
avance parcial y no corrige todo el déficit. En esas dos notas H2/H3 cambian
menos de 0,0002 dB en la misma medida. El mayor cambio de nivel RMS de los
primeros 100 ms, sobre veinte casos, es **0,0076 dB**. Los diez controles de
C3/A3/D#4/C5/C6, velocidades 60 y 110, son idénticos muestra por muestra.
Las cinco alturas modificadas se conservan en el informe para revisar sus
vecinas; la mejora frente a la referencia se exige en C4 a ambas intensidades.

Se renderizó La Campanella completa contra la base aceptada del unísono
agudo, con los mismos eventos, preset Concert y ganancia de exportación 1.
El nuevo WAV dura **280,159 s** y se renderizó en **25,97 s**:
`C:/SHARED/Piano-Campanella-cuerpo-medio-2026-10-14/fundamental/Campanella-B-cuerpo-medio.wav`.

En esa carpeta, `Do-central-A-B.wav` dura 13 s: base y candidato a velocidad
60, luego base y candidato a 110, con tres segundos por toma y 250 ms de
silencio después de cada una. `Notas-A-base-aceptada.wav` y
`Notas-B-cuerpo-medio.wav` permiten escuchar A3/B3/C4/C#4/D#4/C5 primero a
velocidad 60 y luego a 110. Se conserva la misma ganancia, sin normalización
individual ni EQ de exportación. Los audios de la variante rechazada están
en la carpeta superior y no son los WAV de `fundamental/`.

Pasan **diez comprobaciones nativas** (las ocho anteriores, más caída en dos
etapas y caída inicial del fundamental) y la compilación WASM con SIMD128 y
pila de 8 MiB. No se ejecutó aquí una prueba de combustible o reparto entre
instancias WASM. Las nuevas huellas son `24e62f0990d18637` y
`7e537f277662ea34`, porque ambos guiones incluyen la zona modificada. Un
control temporal que pone sólo la nueva corrección a cero reproduce las
dos huellas de la base aceptada, conservando el residuo agudo. Las otras dos
sustituciones del control son únicamente los valores esperados de los asserts.

No se agregan osciladores ni trabajo por muestra: el ajuste se calcula al
crear los parciales de una nota. Tres ejecuciones alternadas de un guion
nativo de 14 s dan medianas de **0,7338 s** para la base y **0,7318 s** para
el candidato; la diferencia queda dentro de la variación de las mediciones.

- [Comparación final, audios y procedencia](body-candidate-comparison.json).
- [Diez pruebas nativas](body-candidate-tests.json).
- [Compilación WASM](body-candidate-wasm.json).
- [Reversión de las huellas con la corrección en cero](body-fundamental-zero-control.json).
- `tools/compare-concert-grand-body-candidate.py`: reproducción del candidato.

**Pendiente de escucha del usuario.** La rama contiene el candidato, con el
diagnóstico previo separado en `29ea9c5e`. No se publica ni instala un paquete.

## Nocturno de Chopin para escucha

Se renderiza completo `chopin-nocturno-op9-2.mid` con el mismo candidato de
cuerpo del fundamental y el residuo agudo aceptado, sin cambiar el DSP.
Son 1.498 pulsaciones y 1.498 liberaciones; se conservan tempo, velocidades,
pedal continuo, una corda y el mensaje inicial de sostenuto. Los 6.028 eventos
coinciden con el plan histórico; la diferencia temporal máxima de 1,302
microsegundos corresponde a su anterior cuantización a 256 kHz. El render
nuevo usa directamente los tiempos racionales del MIDI.

La música dura 224,564 s y se añaden ocho segundos de cola, conservando el
estado final original del pedal (CC64=80), sin eventos artificiales ni fade.
El WAV completo dura **232,564 s (3:53)** y el proceso tarda **24,72 s**.
Pico **0,33999**; RMS final de 100 ms **1,33e−11**. Se verifica salida finita,
igualdad exacta WAV/crudo y hashes del ejecutable y las fuentes del candidato.
Ganancia de exportación 1, sin normalización ni EQ adicional.

Audio: `C:/SHARED/Piano-Nocturno-2026-10-14/Chopin-Nocturno-op9-2-cuerpo-medio.wav`.
[Procedencia y comprobaciones](nocturno-render.json).
Herramienta reutilizable: `tools/render-concert-grand-midi.py`.
