# P7 — Coro no lineal nativo, costo y rechazo del bloque largo

2026-10-13. Continuación autorizada hacia un instrumento tocable.
El avance anterior de cuerpo/presión quedó en **e50b4930** y la primera
cuerda nativa, con su rechazo de costo, en **207bfbc3**.

**Resultado: componente Rust validado a puente fijo; todavía no hay un motor
de piano integrado que pueda tocarse en tiempo real.** El bloque más rápido
falla en duración; el bloque que pasa esa comparación sigue excediendo CPU.
No se instala ni cambia el paquete de fábrica.

## Física y alcance

Tres cuerdas ligeramente desafinadas comparten una cabeza de martillo y tres
contactos independientes de fieltro. Cada cuerda conserva dos direcciones
transversales, 96 modos por dirección, y 192 axiales: **1153 coordenadas**
incluida la cabeza. Se conserva la misma tensión extensible, potencial no
lineal y ley de fuerza dividida/pérdida del maestro Python. La reiteración
conserva las coordenadas y velocidades de las tres cuerdas; sólo recoloca
la cabeza separada y registra el trabajo externo con su signo.

La cuadratura coseno de 256 nodos integra exactamente el potencial polinómico
del conjunto modal finito. La DCT usa una FFT real reordenada de tamaño N,
con permutaciones y twiddles preparados. No se eliminan modos para acelerar.
El programa conserva un avance monolítico AAVF como control; los avances por
bloques se activan explícitamente en la CLI de investigación.

El puente está fijo. Los coeficientes de masa mixta y masa del levantamiento
del extremo ya se calculan y contrastan independientemente; **no se ha
cerrado su interacción con el cuerpo móvil**. Enviar una reacción de extremo
fijo al cuerpo sin esa inercia compartida no sería el cierre instrumental.
Tampoco hay apagadores, pedales, MIDI, presión o salida de audio en este coro.

## Métodos medidos, incluidos los rechazados

El avance monolítico resuelve el gradiente AVF de las tres cuerdas y el
contacto a cada micro-paso. Su primera medida a 44,1 kHz, 256 frames, cuesta
**126,18 ms medios / 148,95 ms máximo** con 96/192 modos. Incluso 12/24 falla
el plazo. Se conserva en [primer-costo.json](medidas-P7-cuerdas-nativas-2026-10-13/primer-costo.json).

Mantener el gradiente no lineal durante 16 pasos reduce costo pero falla el
1% de estado ya a los 6 ms: **1,990% a 44,1 kHz / 1,286% a 128 kHz** para
96/192. La fuerza de ataque cambia poco y el trabajo cierra; ninguna de esas
dos propiedades permite aprobar el estado. [Bloque constante](medidas-P7-cuerdas-nativas-2026-10-13/bloques16.json).

El método siguiente interpola los gradientes de ambos extremos y resuelve
simultáneamente una corrección de trabajo sobre las cuerdas, nunca sobre la
cabeza. El contacto y los portadores lineales siguen al micro-reloj.
Anderson de profundidad tres acelera el mismo punto fijo, con almacenamiento
preparado, coeficientes acotados y retorno a iteración simple si el sistema
pequeño está mal condicionado. Se mantienen las tolerancias de fuerza
1e−11 y de trabajo por bloque 1e−13 de la energía inicial de ese bloque.

La optimización final compone exactamente los portadores axiales/horizontales
al final del bloque: no participan directamente en el contacto. Calcula
también su trabajo y desplazamiento ponderado mediante sumación discreta
por partes, sin dividir por senos modales cercanos a cero. Las coordenadas
verticales siguen avanzando a cada micro-paso; sus bucles contiguos permiten
vectorización. Newton reutiliza la evaluación aceptada de su búsqueda de
línea. Se conservan todos los estados y las mismas ecuaciones objetivo.

La composición usa tablas preparadas para 1–64 pasos, unos **5,72 MiB por
coro completo**. Su memoria y preparación todavía deben rediseñarse al
escalar a todo el teclado; el benchmark mide un solo coro.

## Resultado corto y costo de la última fuente

[Medición final aislada](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-final.json):
C4, intensidad 84, bloque 64, 128 buffers de 256 frames, un hilo; incluye
el primer impacto, sin precalentamiento con una cuerda ya sostenida.
Preparación y lectura de coeficientes quedan fuera. Son tiempos de cribado,
no una garantía de callback bajo el host/WASM o de polifonía.

| Micro-reloj | Error de estado a 6 ms | Error de fuerza de ataque | Media / P99 / máximo, ms | Plazo, ms |
| --- | ---: | ---: | ---: | ---: |
| 44,1 kHz | 0,290434% | 0,010510% | 4,5002 / 5,0280 / **5,9607** | 5,8050 |
| 48 kHz | 0,328574% | 0,009992% | 4,5340 / 6,0164 / **6,3826** | 5,3333 |
| 128 kHz | 0,163298% | 0,006869% | 4,3634 / 5,1072 / **8,6413** | 2,0000 |

Las tres tasas fallan el plazo máximo incluso sin cuerpo/presión. Se observan
**cero asignaciones** durante el avance y defecto energético máximo de
2,57e−15 / 3,68e−15 / 1,60e−14 en el control corto, comprobado sólo en las
fronteras del bloque no lineal. El método por bloques no cierra la energía
física a cada subpaso, y no se afirma que lo haga.

El error corto compara contra el monolítico **al mismo micro-reloj**, no
contra la solución continua. Tampoco valida la elección de 44,1/48 kHz para
estas cuerdas rápidas. 128 kHz es reloj interno: un buffer de audio requiere
más micro-pasos si su tasa de salida es menor; cambiar la etiqueta del plazo
no ahorra trabajo.

La corrida [con validación concurrente](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-con-validacion.json)
se conserva pero no se usa como costo aislado. Las demás etapas conservadas
son [rampa32](medidas-P7-cuerdas-nativas-2026-10-13/rampa32.json),
[Anderson64](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-Anderson.json),
[composición](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-compuesta.json),
[caché del contacto](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-contacto-cache.json),
[portadores contiguos](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-contigua.json)
y [rampa contigua completa](medidas-P7-cuerdas-nativas-2026-10-13/rampa64-contigua-completa.json).
Sus hashes identifican las variantes/executables medidos; no se interpreta
que todas usen la fuente final. Los ejecutables/coeficientes quedan en local.

## Duración: el bloque rápido queda rechazado

El cierre autenticado amplía a **250 ms**, con reiteración a 125 ms y velocidad
0,1 m/s, manteniendo el estado vivo. Cada evento termina el bloque antes de
relanzar la cabeza. El reloj nativo redondea el evento al tick más cercano:
5513 a 44,1 kHz y 16000 a 128 kHz. La referencia monolítica usa esos mismos
ticks. No se comparan directamente dos trayectorias con relojes distintos.

[Primer cierre largo](medidas-P7-cuerdas-nativas-2026-10-13/reiteracion250ms.json)
y [refinamiento autenticado](medidas-P7-cuerdas-nativas-2026-10-13/refinamiento250ms.json):

| Micro-reloj | Bloque 64 | Bloque 32 | Bloque 16 | Bloque 8 |
| --- | ---: | ---: | ---: | ---: |
| 44,1 kHz: error de estado final | **12,0129%** | **8,7270%** | **27,8117%** | **6,4740%** |
| 128 kHz: error de estado final | **5,8585%** | **1,8304%** | 0,324171% | 0,181272% |

Gate de estado/fuerza ≤1%: todos los bloques ensayados fallan a 44,1 kHz;
a 128 kHz sólo pasan 16 y 8. A 44,1 kHz ni siquiera hay refinamiento monótono.
La fuerza registrada sigue cubriendo sólo el ataque de los primeros 6 ms;
el estado y el ledger energético cubren los 250 ms y la reiteración.
El defecto energético máximo sigue por debajo de **8e−13** en los ocho casos.
La conservación del trabajo no evita la desviación de trayectoria.

Se mide por separado el candidato 16 a 128 kHz que pasa ese control de
duración: **11,3002 ms medios / 14,7005 ms máximo** por 256 microframes,
frente a **2 ms** disponibles, sin cuerpo ni presión. [Costo del bloque 16](medidas-P7-cuerdas-nativas-2026-10-13/rampa16-128k.json).
Por eso tampoco se selecciona para el instrumento. No se relaja ningún gate
ni se elige 44,1 kHz a partir de la sola comparación de 6 ms.

## Verificación y siguiente integración

Diez pruebas pasan: seis de cuerdas/contacto y cuatro del cuerpo/presión
existente. El gradiente AVF, potencial y trabajo del extremo se contrastan
contra Gauss compuesto independiente con 4/12/48/96 modos transversales.
La trayectoria monolítica, reiteración y pérdida coinciden con Python; la
trayectoria de pocos modos refina contra DOP853 de la ODE continua. La
composición del bloque se compara contra una referencia que avanza todas
las coordenadas en cada micro-paso; también se verifica una reiteración
fuera de la frontera habitual. Esas pruebas no sustituyen el fallo largo.

Productores: `tools/measure-concert-grand-native-strings.py` y
`tools/verify-concert-grand-native-strings.py`; referencias en
`tools/concert_grand_native_strings.py`, consumidor en
`tools/concert-grand-native-strings.rs`. El verificador autentica fuente,
ejecutable y padre; conserva estados finales pequeños en local para futuros
controles, sin guardar trayectorias largas. Los productores requieren rutas
nuevas y conservan los fallos. No se generan WAV ni factores acústicos nuevos.

Sigue resolver el avance temporal no lineal que cumpla a la vez duración y
costo; después cerrar cuerda/cuerpo con masa mixta, trabajo recíproco y
presión causal. Quedan apagadores/pedales, sostenidos, reiteraciones del
teclado, polifonía y el host MIDI/audio antes de entregar una prueba tocable.

## Continuación: vuelo certificado del martillo

Se compone también el avance vertical si una cota triangular, preparada
sobre todos los ticks del bloque, demuestra compresión negativa en los tres
contactos. La cota incluye la fuerza interpolada/corrección propuesta y el
movimiento futuro de la cabeza; no basta con estar separado al inicio.
Un margen de redondeo devuelve los casos cercanos al primer toque a la ruta
original. No se omite ni modifica una interacción con fuerza distinta de cero.

La referencia explícita `--micro-reference` avanza todos los portadores en
cada micro-paso, sin composición ni certificación. Ocho pruebas de cuerdas
pasan, incluidas la cabeza que se aproxima y los estados/energía de vuelos
4/12/96. Las cotas adicionales ocupan ~0,57 MiB por coro 96/192.

[Primer costo del vuelo certificado](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-primer-costo.json):
128 kHz, bloque 16, 128 buffers. La ruta se acepta en 2033 de 2048 bloques;
mantiene el error corto 0,100690% y cero asignaciones. Costo medio 9,7912 ms,
P99 15,0464 ms, máximo **16,7539 ms** frente a 2 ms. Sigue fallando CPU;
esta corrida conserva la variabilidad y no demuestra una mejora del peor caso.
Ahora se pueden atacar las transformadas/solver que quedan dominando el
sostenido sin alterar el contacto ni repetir los micro-pasos libres.

La siguiente variante prepara twiddles contiguos para mariposas AVX2 y usa
productos escalares vectorizados en el Gram de Anderson. La detección CPU
ocurre en preparación; `--portable` fuerza ambos kernels escalares. Se
preparan las diferencias de Anderson una vez, se calcula sólo el triángulo
simétrico del Gram y se actualizan las fuerzas sobre vectores contiguos.
Se mantienen profundidad, regularización, límites y tolerancias.

[Vuelo16 con SIMD](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-SIMD.json):
128 kHz/16, 128 buffers. Media **5,3284 ms**, P99 **6,1985 ms**, máximo
**10,6779 ms** frente a 2 ms, cero asignaciones. Error corto de estado
0,100690%, mismo ataque y ledger. Sigue fallando CPU; no se promueve al host.
La comparación portable/SIMD de AVF, fuerza, estado y energía pasa en 4/96.
