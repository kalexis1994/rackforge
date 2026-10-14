# P7 — Prueba acotada de viabilidad para tiempo real

Fecha: 2026-10-14. Control físico: **1748aa97**. Esta prueba cambia
explícitamente la resolución del modelo experimental y el reloj de integración.
El modelo completo y la fábrica mantienen sus parámetros por defecto.

**Resultado:** ninguna configuración ensayada pasa a la vez los controles
provisionales de ambas intensidades y el plazo observado por bloque.
Un modelo de ocho modos por dirección transversal cabe para una nota en el
caso suave, con pérdida importante de banda. No queda demostrada la viabilidad
de un piano completo, fiel y polifónico. Tampoco es una prueba de imposibilidad
para otras arquitecturas o reducciones.

Fuentes y datos reproducibles:

- [Barrido principal: 98 ejecuciones](medidas-P7-cuerdas-nativas-2026-10-14/modal-viability.json).
- [Orden temporal: 32 ejecuciones adicionales](medidas-P7-cuerdas-nativas-2026-10-14/modal-orders.json).
- [Auditoría de los 130 resultados y decisiones](medidas-P7-cuerdas-nativas-2026-10-14/modal-viability-audit.json).
- [Productor principal](../../tools/measure-concert-grand-modal-viability.py),
  [productor de órdenes](../../tools/measure-concert-grand-modal-orders.py) y
  [auditor](../../tools/audit-concert-grand-modal-viability.py).

## Qué se comparó

Se usan las tres cuerdas de C4, puente fijo y **96 ms de sostenido**.
La condición inicial es la respuesta **lineal** a un pulso prescrito de
fuerza semisenoidal de 1 ms, compartido entre las cuerdas, con picos totales
de **10 y 50 N**. Las energías iniciales son **0,00202848 y 0,05090202 J**.
La cuadratura temporal de esa inicialización se contrasta con 256/512 puntos.

El pulso define q/v iniciales; **no simula contacto con el fieltro**. Inicializa
el movimiento vertical, con posición/velocidad axiales y horizontales nulas.
La dinámica posterior conserva el acoplamiento no lineal del modelo elegido.
No hay martillo activo, cuerpo, aire, pérdidas, pedales ni polifonía.
Los parámetros son los del prototipo; estas fuerzas no son una identificación
de dos velocidades MIDI de un piano medido.

El modelo completo usa 96 modos por dirección transversal y 192 axiales por
cuerda. Cada reducción toma un **prefijo de coordenadas del mismo estado**,
sin generar nuevas muestras aleatorias ni renormalizar amplitudes o energía.
Conserva masa, tensiones, rigidez, frecuencias de los modos retenidos y
potencial restringido. Reducir los modos cambia el modelo físico.

El barrido prueba 4/8/12/16/24/32/48 modos transversales, manteniendo dos
veces esa cantidad de axiales, y relojes de 16/32/64 kHz. También ensaya
96/192 a 16/64 kHz para comprobar el efecto de bajar sólo el reloj.
El orden principal es 8. Cada caso se contrasta con el **mismo modelo reducido**
a doble reloj y orden 16; el complemento prueba órdenes 16/24 y 8 kHz
en los modelos pequeños.

## Observación y criterios, sin aprobar calidad de piano

`--observation-trace=...` guarda velocidades verticales en **nueve sondas
mecánicas**, tres por cuerda, a fracciones de longitud 0,125/0,31/0,73.
Cada sonda tiene una huella suave de anchura fraccional 0,02. El registro se
preasigna y no altera q/v; su escritura ocurre después del procesamiento.
Se prohíbe activar este registro en el modo benchmark.

Las señales se remuestrean a 48 kHz mediante filtrado polifásico. Se comparan
amplitud y fase, sin ajustar ganancias o alinear artificialmente los tiempos.
Se informa el error relativo RMS máximo entre las nueve sondas, tanto
incluyendo los bordes como excluyendo 2 ms en cada extremo por el filtro.
También se compara el espectro complejo con ventana Hann en cuatro bandas.
Las sondas **no son presión acústica** ni una evaluación de audibilidad.

Los tamices se fijan antes de medir: error mecánico máximo del **5 %**,
refinamiento temporal de señal y estado del **0,1 %**, y máximo medido
de CPU **≤0,125 ms por bloque**. El 5 % es un criterio exploratorio de esta
prueba, no un umbral perceptual ni una aprobación solicitada por el usuario.
Además se mantienen los gates energéticos 1e−9 y cero asignaciones.

La referencia completa a 512 kHz se contrasta entre órdenes 8 y 16.
El cambio de estado es **5,918e−14 / 1,303e−10** para los pulsos 10/50 N;
el cambio de sondas es **5,004e−15 / 1,815e−13**. Es un refinamiento numérico
de estas nuevas condiciones iniciales, no la referencia de 16,384 MHz del
estado aleatorio anterior ni una validación acústica del instrumento.

## Resultados decisivos

La tabla muestra el caso fuerte, 50 N. La media y el máximo son los peores
entre tres benchmarks: inicio, repetición e inicio desde el estado final de
los 96 ms. Cada benchmark tiene 64 bloques de 0,125 ms, 8 ms físicos.

| Modos transversales / axiales | Reloj / orden | Error mecánico | Media por bloque | Máximo | Refinamiento temporal |
|---|---:|---:|---:|---:|---|
| 8 / 16 | 16 kHz / 8 | 4,53 % | 0,0687 ms | 0,1158 ms | Falla |
| 8 / 16 | 16 kHz / 16 | 4,51 % | 0,1231 ms | 0,1550 ms | Pasa |
| 12 / 24 | 16 kHz / 16 | 1,75 % | 0,1924 ms | 0,2334 ms | Pasa |
| 32 / 64 | 64 kHz / 8 | 0,098 % | 0,9542 ms | 1,1998 ms | Pasa |
| 96 / 192 | 64 kHz / 8 | 98,61 % | 3,8651 ms | 5,2086 ms | Falla |

El caso suave de ocho modos, 16 kHz/orden 8, es el único candidato
provisional del primer barrido: error **4,52 %**, máximo **0,1186 ms** y
refinamiento temporal aprobado. Ese mismo esquema falla el refinamiento del
caso fuerte: cambio de estado **1,01 %** al refinar, frente al límite 0,1 %.
Subir a orden 16 lo corrige, pero excede el plazo medido.

El RMS global oculta una pérdida de banda relevante. En la versión de ocho
modos, el error espectral relativo en **2–6 kHz ronda el 100 %**, también en
bandas superiores. Esa banda contiene aproximadamente el **0,15 %** de la
energía de las sondas de referencia, mientras 20 Hz–2 kHz contiene el 99,85 %.
Por eso un error global cercano al 4,5 % no demuestra conservación del timbre.
El cuerpo acústico podría ponderar esas bandas de otra forma; no se midió.

Conservar 96/192 y bajar sólo el reloj a 16/64 kHz da errores mecánicos
**72–73 %** en el pulso suave y **98–99 %** en el fuerte. Los cierres de
energía pasan: **conservar energía no certifica trayectoria ni señal**.

El modelo completo a 512 kHz/orden 8 cuesta **13,83/13,49 ms** de media en
estos pulsos. Es una condición inicial distinta, con canales inicialmente
vacíos, de la que costaba 29,3 ms en el estado mixto aleatorio. Esa diferencia
no es una aceleración del motor ni debe sumarse a las optimizaciones previas.

## Margen para el resto y generalización

Incluso el caso fuerte de ocho modos/orden 16 consume **0,1231 ms medios**
para una sola nota de tres cuerdas: casi todo el bloque antes de añadir el
resto del instrumento. Sumar linealmente ocho costos medios da **7,88 veces**
el presupuesto de un bloque. En orden 8 da 4,40 veces, pero falla el
refinamiento temporal fuerte. Son extrapolaciones secuenciales de costos,
**no benchmarks polifónicos** ni cotas de una implementación paralela.

Los máximos se miden bajo Windows general y carga variable; no se establece
una garantía de latencia ni un límite inferior del algoritmo. Se informa el
peor de las tandas, sin eliminar la primera ejecución ni picos desfavorables.

La instrumentación reproduce exactamente el control mixto de 6 ms de
1748aa97. Aplicar a ese estado la reducción 12/24 a 16 kHz da **100,03 %**
de error de estado completo y **102,10 %** en los modos retenidos. La
proyección omite el **98,76 % de la energía lineal** del estado final fino.
La reducción no es una sustitución general del modelo anterior.

## Decisión

No se aprueba un motor de piano completo para tiempo real ni se cambia la
resolución por defecto. El recorte simple demuestra una ruta hacia un
**componente monofónico de banda limitada**, con restricciones de intensidad,
precisión temporal y margen. No demuestra el objetivo de Concert Grand.

La siguiente hipótesis arquitectónica útil sería conservar los modos lineales
agudos y reducir sólo el costo de la respuesta no lineal. Eso requeriría
otra prueba de costo, banda, contacto, registros y polifonía; no está implementado
ni se promete su rendimiento. Estos datos no justifican seguir tratando las
microoptimizaciones del solucionador completo como un camino asegurado.

## Integridad y pruebas

La auditoría verifica **130 ejecuciones nativas**, entradas, resultados,
trazas y fuentes. Una escritura interrumpida del checkpoint dejó los
resultados nativos intactos. Se recuperaron **72 ejecuciones** comprobando
el ejecutable, todas las fuentes de cálculo, las entradas serializadas y los
hashes de resultados/trazas. El informe conserva el productor original como
texto y hash, el manifiesto anterior y la procedencia de cada resultado
reutilizado. El productor actual guarda checkpoints atómicos locales con
reintentos y publica el informe final una sola vez. No se repitieron esas
integraciones ni se descartaron sus tiempos.

Pasan **32 pruebas actuales** de fases, trayectoria y diagnóstico modal,
incluyendo la proyección exacta de coordenadas/frecuencias, convergencia del
pulso prescrito, transparencia y preasignación de las sondas, y una señal
conocida que verifica remuestreo, fase y detección de pérdida de banda.
Suite acumulada **58 pruebas**; cuerdas/presión normales no cambian.
Las historias grandes quedan sólo en `local/concert-grand-physics/`.
