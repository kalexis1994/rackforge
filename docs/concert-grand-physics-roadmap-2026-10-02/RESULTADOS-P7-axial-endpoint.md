# P7 — Eliminar incógnitas axiales sin eliminar su dinámica

2026-10-13. Continuación de optimización autorizada. Base **ac8d2dda**;
coro precedente **928cd20f**, fuera del paquete de fábrica.

## Identidad utilizada

En el potencial finito SI, el gradiente no lineal axial es
`g_u = EA D_uᵀ W η(v,w)`, con `η=(v_x²+w_x²)/2`. No depende de `u`.
Para un extremo transversal propuesto se puede evaluar `g_u` directamente,
componer todos los portadores axiales con ese extremo y volver a evaluar
las fuerzas transversales con la posición axial actualizada. Se conserva
`q0/v0`, masa, rigidez, fases rápidas y todos los modos 96/192 por cuerda.
No es una aproximación axial estática ni una reducción del espacio físico.

El trabajo axial y su desplazamiento ponderado se sustituyen también por
los de la nueva trayectoria. La referencia `--micro-reference` reproduce
esos mismos portadores paso a paso. `--uncondensed` conserva la iteración
axial completa. Tolerancias: fuerza/corrección 1e−11, trabajo del bloque
1e−13 de su energía; estado/fuerza temporal ≤1%, ledger ≤1e−9.

## Primer candidato: mejora de costo, equivalencia rechazada

[Primer costo](medidas-P7-cuerdas-nativas-2026-10-13/axial16-primer-costo.json):
C4 limpia, intensidad 84, 128 kHz/16, 128 buffers. Máximo de iteraciones
10→4; media **2,0725 ms**, P99 **3,1743 ms**, máximo/ataque **4,7852 ms**
frente a 2 ms. Cero asignaciones. Estado corto frente al monolítico
0,100696%, ledger 1,69e−14. Frente al anterior avance por bloques, cambio
de estado 7,23e−8 y de fuerza 1,12e−9.

[250 ms con reiteración](medidas-P7-cuerdas-nativas-2026-10-13/axial16-reiteracion250ms.json):
estado **0,324165%**, ledger **2,64e−12**, ambos pasan. Gauss compuesto
independiente confirma la fuerza axial a partir de las posiciones
transversales finales para 4/12/96 modos.

Eso no basta para aprobar equivalencia de solver. El control con ambos
campos perturbados y seed 83 da una diferencia de **1,4616e−5** en 4 modos
y **0,00607482** en 96: ambos fallan el límite adicional 1e−6, aun cerrando
trabajo. [Rechazo conservado](medidas-P7-cuerdas-nativas-2026-10-13/axial16-correction-ambiguity.json).
La prueba unitaria de equivalencia falla y este candidato no se selecciona.

La corrección anterior acumula incrementos en direcciones de desplazamiento
distintas durante las iteraciones. Al converger, el ledger impone un producto
escalar, pero deja indeterminado el componente perpendicular de esa fuerza.
Cambiar el camino de iteración puede cambiar ese componente y la trayectoria.
Sigue definir una corrección paralela al desplazamiento de las cuerdas para
que ambas rutas resuelvan un paso determinado, conservando el rechazo y el
control monolítico. El contacto y la cabeza quedan fuera de esa corrección.

## Corrección determinada

Se fija `c = lambda (q_end - q_start)` sobre las cuerdas. La corrección se
proyecta completa en esa dirección durante la solución, en lugar de
acumular incrementos que dejan una componente perpendicular indeterminada.
Las rutas con/sin eliminación axial vuelven a pasar la equivalencia ≤1e−6
en ambos controles perturbados, manteniendo los límites de fuerza/trabajo.
La referencia por micro-pasos y Gauss independiente también pasan.
`--legacy-correction` conserva el algoritmo rechazado para diagnóstico.
La comparación de equivalencia es entre las dos rutas del nuevo paso
determinado; no se presenta como identidad con el paso anterior ambiguo.

[Primer costo con dirección determinada](medidas-P7-cuerdas-nativas-2026-10-13/axial16-canonical-vector.json)
conserva el ataque y el fallo de presupuesto de 2 ms. Todavía se itera un
vector de corrección completo. Sigue aprovechar su único coeficiente
escalar en el vuelo certificado, sin dejar de verificar el contacto fino.

## Vuelo con una corrección escalar

En ausencia certificada de contacto, el mapa lineal permite resolver
`q_end = (q_free - Qa lambda q_start)/(1 - Qa lambda)` y `c=lambda Δq`
directamente para las coordenadas transversales. Después se determina el
extremo axial con la misma identidad, conservando su posición/velocidad.
Sólo se iteran 576 fuerzas transversales y un coeficiente de trabajo: **577**
incógnitas frente a 2306 en la ruta vectorial, con **1153 estados físicos**.
El escalado del coeficiente es fijo por bloque y tiene unidades de fuerza.
Si una propuesta no certifica separación o no tiene denominadores seguros,
se vuelve al solver determinado completo desde el estado físico inicial.
`--vector-flight` conserva ese control.

[Primer costo escalar](medidas-P7-cuerdas-nativas-2026-10-13/axial16-scalar-flight.json):
128 kHz/16, 128 buffers. Media **2,0709 ms**, P99 **2,7140 ms**, máximo/ataque
**4,3844 ms** frente a 2 ms. Cero asignaciones; 2033 vuelos escalares y 15
retornos a la ruta completa. Estado corto cambia 3,37e−13 respecto a la ruta
sin eliminación axial del mismo paso determinado. La prueba con ambos
campos excitados también pasa ≤1e−6 contra la corrección vectorial.
No se afirma mejora de tiempo respecto al primer costo vectorial; sigue
calcular/escalar el vuelo sin trabajo redundante y reducir el costo de
resolver nuevamente el contacto durante el ataque.

## Contacto reutilizado y cierre de la etapa

Se guarda la solución de las tres fuerzas de contacto en cada microinstante
de la iteración no lineal anterior. Sólo se usa como punto inicial de Newton:
se vuelven a resolver fuerza y calor con las mismas tolerancias. La traza
se vacía al iniciar cada bloque; si falla una solución iniciada con esa
traza, se reintenta desde cero antes de aceptar estado físico. `--cold-felt`
conserva el control. El vuelo escalar usa ahora la norma del desplazamiento
previsto de las cuerdas como escala; la cabeza queda fuera de esa corrección.

Mediciones consecutivas, sin ejecutar las pruebas simultáneamente: C4,
intensidad 84, tres cuerdas 96/192, 256 micro-pasos a 128 kHz, bloques de 16.
El plazo por buffer es **2 ms**. Se incluyen el primer ataque y todos los
buffers; no se descuentan los retornos al solver completo.

| Ruta | Buffers | Media ms | P99 ms | Máximo ms | Primer ataque ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| [Anterior](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-final.json) | 512 | 4,7170 | 5,7415 | 10,3108 | 10,3108 |
| [Final, escalar y contacto reutilizado](medidas-P7-cuerdas-nativas-2026-10-13/axial16-final.json) | 512 | 2,2314 | 3,7293 | 3,9336 | 3,7293 |
| [Control vectorial](medidas-P7-cuerdas-nativas-2026-10-13/axial16-vector-control.json) | 512 | 2,1953 | 3,0317 | 3,9194 | 3,9194 |
| [Contacto desde cero](medidas-P7-cuerdas-nativas-2026-10-13/axial16-cold-felt-control.json) | 128 | 2,2171 | 3,6298 | 4,3500 | 4,3500 |
| [Portable](medidas-P7-cuerdas-nativas-2026-10-13/axial16-final-portable.json) | 128 | 2,3830 | 3,8126 | 4,1625 | 4,1625 |

La media final baja **52,7%** frente al precedente, y el primer ataque
**63,8%**. Aun así, todas las rutas **fallan el plazo de 2 ms**. El máximo
final ocurre en el buffer 239, después del ataque. La ruta escalar no
demuestra ventaja de tiempo sobre el control vectorial: su media difiere
aproximadamente 1,6%, y los tiempos tienen variación entre corridas.

Contacto reutilizado: **1750** iteraciones de Newton frente a **2543** desde
cero, reducción **31,2%**, con 538 puntos iniciales reutilizados y cero
reintentos. Ambas corridas contienen un único buffer con contacto, aunque
tengan distinta duración total. Esta reducción de iteraciones no demuestra
por sí sola una mejora del tiempo completo del callback. La ruta final
acepta 8177 vuelos escalares de 8192 bloques y usa 15 retornos al solver
completo; máximo de cuatro iteraciones externas y **cero asignaciones**.

Pasan **18 pruebas** de cuerdas y presión. Incluyen fuerzas axiales contra
Gauss independiente, equivalencia determinada con/sin eliminación axial,
vuelo escalar/vectorial con ambos campos excitados, trayectoria/fuerza del
contacto reutilizado contra inicio desde cero, rechazo de vuelos con impacto
futuro y conservación del cache entre bloques. Los límites de equivalencia
siguen en 1e−6 y el control de contacto en 1e−8.

[Control final de 250 ms con reiteración viva a 125 ms](medidas-P7-cuerdas-nativas-2026-10-13/axial16-final-reiteracion250ms.json):
error de estado **0,3241655%**, ledger máximo **2,6324e−12**, ambos pasan.
El control corto da **0,1006955%** de estado, **0,0004888%** de fuerza y
ledger **1,4955e−14**. La fuerza registrada cubre los primeros 6 ms; estado
y trabajo cubren los 250 ms. Se reutiliza la referencia monolítica anterior
autenticada, con los mismos coeficientes y eventos; no se vuelve a integrar.
Informes conservan hashes de fuentes, ejecutable, coeficientes y referencia.

Se mantienen las **1153 coordenadas mecánicas**, sin retirar modos ni
cambiar el potencial constitutivo. La dirección de la corrección de trabajo
sí determina un paso numérico nuevo, documentado arriba: no se afirma
identidad con el paso ambiguo anterior. El primer candidato rechazado queda
conservado. Son ensayos de un coro a puente fijo: cuerpo móvil, presión,
pedales y polifonía aún deben integrarse y medirse. No prueban tiempo real
del piano ni duración de render de La Campanella. Sigue reducir el costo
de evaluación de campos y validar cualquier aumento del bloque temporal
antes del cierre de puertos con la inercia completa. Sin WAV, aprobación
global ni actualización del paquete de fábrica.
