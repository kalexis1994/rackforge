# P7 — Refinamiento acotado y restauración del bloque

2026-10-13. Continuación autorizada desde **95c7d076**. Se corrige el fallo
del solver a 254,875 ms conservado en la etapa anterior, sin aprobar por ello
precisión temporal, tiempo real ni el modelo instrumental completo.

## Implementación

El paso habitual sigue intentando el bloque solicitado, con las mismas
32 iteraciones máximas por solver, tolerancias de fuerza/corrección 1e−11
y trabajo del bloque 1e−13 de la energía. Si converge, devuelve el resultado
directamente: no copia las reservas de restauración.

Un intento rechazado restaura posición y velocidad y no publica su extremo
en el cache. Sólo entonces se guarda el estado inicial en tres vectores
preparados: posición, velocidad y fuerza del cache, unos **27 KiB por coro**.
Se reintenta con la mitad de pasos. Si vuelve a fallar, se reduce otra vez
hasta un micro-paso. El tamaño no aumenta dentro de la recuperación del
mismo bloque, por lo que el trabajo está acotado: a lo sumo 64 hijos
aceptados y las reducciones hasta un paso para la solicitud máxima de 64.

Los hijos aceptados cubren exactamente el intervalo solicitado. Se suman
sus calores, se conservan los máximos de fuerza/iteraciones y se ensambla
la traza de cada micro-paso. El último extremo aceptado queda disponible
para el próximo bloque. Los límites de eventos del llamador siguen intactos.

Si también falla un paso, se restaura **todo** el estado anterior a la
solicitud: posición, velocidad, fuerza/potencial/validez del cache y los
contadores de vuelos aceptados. Se descarta la traza parcial y no se devuelve
calor aceptado. Los contadores de trabajo intentado conservan los rechazos.
`--no-ramp-refinement` mantiene el control que falla. No se retiran modos,
se reinician vibraciones ni se relajan las tolerancias.

## Regresión del fallo y restauración

Pasan **24 pruebas** de cuerdas y presión. Los tres controles nuevos verifican:

- El caso sostenido con 96/192 modos, seed 83 y ambos ejes excitados falla
  sin recuperación. Con recuperación resuelve el bloque que comienza en
  el micro-paso **32624** mediante **dos hijos de 8**. Posición, velocidad,
  fuerza, calor y trabajo inyectado coinciden exactamente con la subdivisión
  manual, desactivando la recuperación e imponiendo límites en 32632/32640.
- Un diagnóstico restringido a una iteración hace aceptar un prefijo en
  vuelo y luego falla al llegar al contacto. Se restauran exactamente estado
  y energía, tanto con cache válido como sin cache. El contador de vuelos
  aceptados queda intacto y la traza parcial se descarta. El límite normal
  permanece en 32; este modo sólo prueba la ruta terminal de error.
- Sin rechazos, la trayectoria es exactamente igual a la ruta sin
  recuperación, incluso con una reiteración no alineada. Se mantienen todos
  los controles anteriores de Gauss, referencia por micro-pasos y puertos.

[Verificador y evidencias del caso sostenido](medidas-P7-cuerdas-nativas-2026-10-13/adaptive16-recovery.json).
El ejecutable congelado y el nuevo con recuperación desactivada conservan
su fallo. El recuperado termina 256 ms, con **un rechazo**, una solicitud
refinada, dos hijos de 8 y tres intentos totales. Ledger **4,1698e−12** pasa.

## Costo del componente

C4 limpia, intensidad 84, 256 micro-pasos por buffer a 128 kHz, bloques de 16;
el plazo es **2 ms**. Corridas consecutivas después de las pruebas.

| Ruta | Buffers | Media ms | P99 ms | Máximo/ataque ms |
| --- | ---: | ---: | ---: | ---: |
| [Recuperación disponible](medidas-P7-cuerdas-nativas-2026-10-13/adaptive16-final.json) | 512 | 1,9533 | 2,8390 | 3,7407 |
| [Recuperación desactivada](medidas-P7-cuerdas-nativas-2026-10-13/adaptive16-disabled-control.json) | 512 | 1,9043 | 2,2674 | 3,7195 |
| [Portable](medidas-P7-cuerdas-nativas-2026-10-13/adaptive16-portable.json) | 128 | 2,1042 | 2,3210 | 4,3176 |

No hay refinamientos en estas tres corridas. La diferencia de tiempos no
demuestra una mejora de costo: esta etapa agrega recuperación, y P99/ataque
**siguen fallando**. Todas miden **cero asignaciones** durante procesamiento.
El [control habitual de 250 ms con reiteración](medidas-P7-cuerdas-nativas-2026-10-13/adaptive16-reiteracion250ms.json)
conserva el estado **0,3241655%**, ledger **2,6324e−12**, sin refinamientos.

Con ambos ejes perturbados, el costo es mucho mayor. El benchmark recuperado
de 128 buffers termina: media **7,1966 ms**, P99 **11,0856 ms**, máximo/ataque
**11,3257 ms**. Un buffer contiene recuperación y cuesta **11,0856 ms**.
El de 512 buffers (**1,024 s**) también termina: media **11,0319 ms**, P99
**17,2666 ms**, máximo **20,4251 ms** en el buffer 431. Hay 164 rechazos,
164 solicitudes refinadas, 328 hijos de 8 y 112 buffers con recuperación.
Ninguna solicitud necesita más de tres intentos en ese ensayo. Ambas corridas
miden cero asignaciones y **fallan ampliamente** el plazo de 2 ms.

## Precisión temporal aún sin aprobar

Terminar y cerrar energía no valida la trayectoria. En el caso perturbado
sostenido de 256 ms, la diferencia frente al AAVF monolítico a 128 kHz es
**136,9885% de estado** y **2,4689% de fuerza durante los primeros 6 ms**:
se conserva el fallo del límite 1%. El monolítico también falla su límite
de ledger 1e−9: **8,5007e−8**. Por tanto, ese control no constituye una
referencia global aprobada ni permite atribuir por sí solo toda la diferencia
a la aproximación por bloques.

[Diagnóstico con pasos más finos](medidas-P7-cuerdas-nativas-2026-10-13/adaptive16-mixed-clock-diagnostic.json):
frente al mismo monolítico, N8 da **137,2576%** de estado, N4 **138,3658%**
y N1 **142,2300%**. N4/N1 cierran ledger en **1,9606e−12 / 1,8106e−11**;
fuerza inicial **1,0286 / 0,6645%**. Acortar el bloque hasta un paso no elimina
la discrepancia de estado. No se acepta N8 como solución de precisión ni se
amplía la aprobación del control limpio a estados arbitrarios perturbados.

Se resuelve la recuperación local de convergencia y su transacción, mientras
queda pendiente una referencia temporal refinada y el estudio de las fuerzas
no lineales/corrección de trabajo en estados con ambos campos excitados.
Esto tiene prioridad sobre más cambios de CPU o el cierre del cuerpo móvil.
Se conservan las 1153 coordenadas. Cuerpo, presión, pedales y polifonía todavía
no están integrados; no hay WAV, render de La Campanella, aprobación global
ni modificación de fábrica.
