# P7 — Modos lineales completos y respuesta no lineal reducida

Fecha: 2026-10-14. Continuación de la
[viabilidad por recorte modal](RESULTADOS-P7-viabilidad-tiempo-real.md).
Fuentes y referencias anteriores autenticadas contra **5a4c4d64**.

Se implementó y midió la hipótesis pedida: mantener los 96 modos
transversales por dirección y 192 axiales por cuerda, calculando la
respuesta no lineal sólo en un prefijo. **La banda aguda vuelve a tener
energía, pero su evolución todavía difiere mucho con la excitación fuerte.**
Ninguna configuración medida cumple el plazo de 0,125 ms para el
solucionador; la salida de audio tampoco está implementada en tiempo real.
El experimento queda disponible por opción explícita, sin cambiar el modelo
completo por defecto ni la fábrica.

## Modelo que se probó

Se define un Hamiltoniano aproximado:

`H_híbrido(q,v) = H_lineal(q,v) + V_no_lineal(P_bajo q)`.

Todas las 1152 coordenadas de las tres cuerdas sobreviven en el estado.
`P_bajo` conserva, por cuerda, N modos transversales en cada dirección y
2N axiales. El subsistema retenido usa la colocación y proyección de
energía independiente por cuerda existentes. Los demás modos avanzan con
sus portadores lineales exactos, con las frecuencias y amplitudes iniciales
originales. No se normalizan energías, ganancias o fases.

La aproximación retira los acoplamientos no lineales con los modos altos,
incluyendo su reacción sobre los bajos. También retira la posibilidad de
generar inicialmente desde cero esos modos mediante la no linealidad.
Por eso conservar sus coordenadas y energía lineal inicial no garantiza
reproducir su trayectoria en el sistema completo.

La proyección energética sólo toca el subsistema bajo. En caso de fallo
del solucionador no se publica ninguna coordenada, incluidos los portadores
altos. No se reservan matrices de colocación sobre las 1152 coordenadas.
El cálculo conserva el Hamiltoniano aproximado; la energía del Hamiltoniano
original se audita por separado y puede cambiar.

## Comparación y observación

Se reutilizan las referencias completas de 96 ms de C4, tres cuerdas,
512 kHz/orden 16, de la prueba anterior, con pulsos prescritos de 1 ms y
picos totales de 10/50 N. Los estados iniciales se reconstruyen de la
misma integral lineal del pulso. No se simula el contacto de fieltro.
Las referencias habían pasado su refinamiento de orden 8 a 16.

Tres tamaños no lineales: 8/16, 12/24 y 24/48. Sus relojes son 16, 16 y
32 kHz, respectivamente, a orden 16. Cada caso se contrasta con el doble
de reloj y de orden, manteniendo el mismo Hamiltoniano aproximado.
Los seis pasan el umbral provisional de 0,1 % tanto en estado completo
ponderado por energía lineal como en las nueve sondas de velocidad.

Los portadores altos no se observan al reloj bajo: eso introduciría aliasing.
La traza nativa contiene sólo la señal de los modos no lineales. Para el
diagnóstico offline se interpola esa traza a 512 kHz, se suman los portadores
altos evaluados analíticamente a 512 kHz y se filtra/remuestrea el conjunto
a 48 kHz, igual que la referencia. Se informa el error con y sin los 2 ms
de borde del filtro. No hay ajuste de fase, tiempo ni ganancia.

Esta reconstrucción es **una herramienta offline de comparación**. El costo
del mezclador de audio, su filtro y una implementación causal de interpolación
no están incluidos en los benchmarks. Los tiempos sí incluyen avanzar todas
las coordenadas altas en cada paso del solucionador, publicar el estado
completo y monitorizar la energía híbrida.

Se mantienen el puente fijo, la cabeza inmóvil y las cuerdas sin pérdidas.
No se han integrado cuerpo, presión, pedales, contacto ni polifonía. Las
sondas son mecánicas; no certifican timbre, audibilidad o presión acústica.

## Resultados

Los costos son el peor promedio y el peor máximo de tres tandas de 64
bloques de 0,125 ms: inicio, repetición y continuación desde el estado
final de 96 ms. Se incluyen el primer bloque y los picos medidos bajo
Windows general. Son mediciones de una nota de tres cuerdas, sin garantía
de latencia.

| Pulso | Modos no lineales T/A | Reloj / orden | Error de sondas | Error con bordes | Media por bloque | Máximo |
|---|---:|---:|---:|---:|---:|---:|
| 10 N | 8/16 | 16 kHz / 16 | 0,376 % | 1,708 % | 0,1120 ms | 0,2019 ms |
| 10 N | 12/24 | 16 kHz / 16 | 0,209 % | 1,688 % | 0,1851 ms | 0,3620 ms |
| 10 N | 24/48 | 32 kHz / 16 | 0,085 % | 0,900 % | 0,6054 ms | 0,7491 ms |
| 50 N | 8/16 | 16 kHz / 16 | 6,556 % | 6,565 % | 0,1318 ms | 0,1451 ms |
| 50 N | 12/24 | 16 kHz / 16 | 2,705 % | 2,805 % | 0,2011 ms | 0,3188 ms |
| 50 N | 24/48 | 32 kHz / 16 | 0,429 % | 0,877 % | 0,6796 ms | 0,9458 ms |

Con ocho modos no lineales, el caso suave mejora respecto del recorte
simple anterior: aproximadamente **4,52 % → 0,376 %** de error mecánico.
En el fuerte, el recorte simple a orden 16 tenía **4,507 %** y esta variante
da **6,556 %**. Sumar modos altos con una evolución inexacta puede aumentar
el error aunque evite el vaciamiento de banda.

El error de estado completo en el fuerte es **7,974 / 2,522 / 0,421 %**
para N=8/12/24. El mayor cambio de estado al refinar los seis casos es
**0,01592 %**; el mayor cambio de sondas es **0,07291 %**. Subir reloj y
orden resuelve el Hamiltoniano elegido, pero no recupera el acoplamiento
físico que se quitó.

## Qué ocurre con los agudos

Error complejo de espectro en el caso fuerte, sin descartar la fase:

| Modos no lineales T/A | 20 Hz–2 kHz | 2–6 kHz | 6–12 kHz | 12–20 kHz |
|---|---:|---:|---:|---:|
| 8/16 | 0,122 % | 148,14 % | 183,68 % | 157,68 % |
| 12/24 | 0,105 % | 59,28 % | 183,68 % | 157,70 % |
| 24/48 | 0,054 % | 0,062 % | 69,58 % | 152,90 % |

Un error complejo superior al 100 % es posible con señales de energía
parecida y distinta fase o distribución de frecuencias. La auditoría también
compara magnitudes y RMS por banda: en N=8 fuerte, el cociente de RMS
candidato/referencia en 2–6 kHz es **1,0010**, y el error de magnitud
espectral es **44,84 %**. En 6–12 kHz son **1,0021** y **74,15 %**.
Esto distingue el resultado de la desaparición de banda del recorte simple:
hay energía aguda, con evolución espectral incorrecta. La contribución exacta
de cada acoplamiento eliminado aún requiere una ablación específica.

En el caso suave de N=8, los errores complejos en 2–6/6–12/12–20 kHz
son **7,11 / 12,39 / 42,67 %**: también hay diferencias que el RMS global
pequeño oculta. N=24 recupera bien hasta 6 kHz en ambos pulsos; las bandas
superiores siguen discrepando. El cuerpo acústico podría ponderarlas de
otra forma; no se evaluó esa ponderación.

## Energía, generalización y decisión

La auditoría independiente del Hamiltoniano híbrido por cuerda da un máximo
de **2,381e−12** relativo entre las 31 ejecuciones. La comparación de los
portadores altos con la solución lineal analítica da **1,039e−12** como
máximo. Todas terminan con cero asignaciones durante el procesamiento.

En los pulsos, el cambio de energía del Hamiltoniano original al final de
96 ms llega a **2,440e−6** relativo. Es pequeño, aunque la señal pueda
tener errores grandes. No se usa ese ledger como certificación de fidelidad.
El estado mixto aleatorio original de 6 ms sigue siendo un contraejemplo:
N=8, 512 kHz/orden 8 conserva todas las coordenadas y falla con **130,97 %**
de error de estado y **0,565 %** de cambio de energía original. El error
ya no puede atribuirse a energía omitida: no se descarta ninguna coordenada.

La hipótesis confirma que el costo de transportar modos lineales completos
es compatible con un solucionador pequeño. Sin embargo, la variante más
barata medida ya usa **0,1318 ms medios / 0,1451 ms máximos** en el pulso
fuerte, antes de producir audio. N=24 mejora considerablemente la señal y
consume **0,6796 ms medios**, 5,44 veces el plazo de un bloque para una sola
nota. No se ha medido polifonía ni margen para el resto del instrumento.

No se selecciona esta variante como reemplazo general. La siguiente
hipótesis concreta sería reducir la evaluación de los acoplamientos y
mantener alguna reacción de los modos altos a los bajos, con una energía
recíproca bien definida. El resultado presente no demuestra que baste una
modulación escalar de tensión; habría que medir esa aproximación por separado.

## Reproducción e integridad

- [Medición de 31 ejecuciones](medidas-P7-cuerdas-nativas-2026-10-14/hybrid-linear-highs.json).
- [Auditoría y diagnósticos espectrales adicionales](medidas-P7-cuerdas-nativas-2026-10-14/hybrid-linear-highs-audit.json).
- Núcleo opcional: `tools/concert-grand-hybrid.rs`, seleccionado mediante
  `--collocation --linear-highs=N` en el ejecutable de trayectoria.
- Productor: `tools/measure-concert-grand-hybrid.py`; auditor independiente:
  `tools/audit-concert-grand-hybrid.py`. Ambos exigen rutas nuevas para informes.
- Las fuentes medidas quedan congeladas por SHA256; las referencias viejas
  se autentican con sus fuentes en 5a4c4d64 y los hashes de sus artefactos.
  Entradas, ejecutable y trazas grandes permanecen sólo en
  `local/concert-grand-physics/P7-hybrid-linear-highs-2026-10-14/`.

Pasan **36 pruebas actuales** de trayectoria, fuerzas, reducción y señales.
Las cuatro nuevas comprueban identidad exacta al retener toda la no linealidad,
coincidencia del subsistema bajo y portadores altos analíticos, publicación
atómica ante fallo y observación modal densa independiente. La suite acumulada
contiene 62 pruebas, contando las 26 de cuerdas/presión sin cambios, ejecutadas
en las etapas anteriores. No se modificó ni instaló ningún paquete.
