# Cuerpo y sostenido sobre la release publicada

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

## Candidato: separación mínima del unísono agudo

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

El candidato queda en la rama de trabajo. No se publicó ni se instaló un
paquete; la selección por escucha sigue pendiente.
