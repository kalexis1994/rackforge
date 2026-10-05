# P6: puerto de 59 modos con resolución acústica común

Fecha: 2026-10-05. Base: `90c7cff`, carga refinada de 90 modos.
Esta etapa prepara la comparación 59→90 sin mezclar expansión estructural
con cambio de discretización. El paquete 0.172.0 conserva su huella.

## Reutilización de coeficientes y selección física

Las bases de 59 y 90 modos usan las mismas celdas 192×144, factor a 128 kHz
y cuadratura angular 96×96. Los 59 modos tienen frecuencias hasta 1193,507913 Hz.
El factor archivado de 90 modos ya contiene sus 59 primeras columnas, por
lo que no se guarda otra copia grande del factor. Se seleccionan también
los bloques principales de masa estática y resistencia límite.

La selección debe coincidir exactamente con el prefijo ordenado de
frecuencias físicas del asset. Se verifican dimensiones, orden y huellas
del coeficiente y del reporte padre. Los targets seleccionados coinciden
con los calculados independientemente sobre la superficie de 59 modos:
error de masa cero, error relativo de resistencia límite 1,66e−17.
El factor de los 35 modos de control se genera independientemente a la
resolución nueva y coincide exactamente con el bloque seleccionado.

Se reconstruye `BoundedAir` para 59 coordenadas. **No se recortan los estados
de relajación de la carga de 90 modos**: el completado de masa/resistencia
se calcula de nuevo en la dimensión seleccionada. Sus límites físicos
coinciden con los bloques principales; el completado aproximado en
frecuencia no tiene por qué ser el bloque principal del completado de 90.
Por eso se verifica la nueva carga completa contra la geometría.

El puerto tiene 38232 estados, con tiempos de relajación de
4,689083–7,614008 µs. Los límites de masa/resistencia cierran a
3,21e−16 / 3,00e−15. No hay recorte de autovalores ni nuevos parámetros
de pérdidas, fieltro o afinación.

## Verificación de la carga seleccionada

Se conservan los umbrales anteriores: transferencia 2%, velocidad cargada
5%, fuerza RMS 2%, pico tardío 1%, cambio de referencia espacial 1%,
integrales geométricas independientes 1e−5 y presupuesto energético 1e−9.
El resumen completo es
[resumen-P6-tabla-1200-refinada.json](resumen-P6-tabla-1200-refinada.json).
**Pasan los once controles.**

| Medición | Resultado | Límite |
| --- | ---: | ---: |
| Máximo Z/R/X, 210 puntos de 60–1200 Hz | 0,169288% / 0,150340% / 0,191491% | 2% |
| Máximo complejo, tres puntos de Laplace | 0,155005% | 2% |
| Máximo complejo, 61 puntos de 1–6 kHz | 0,599796%, a 6 kHz | 2% |
| Velocidad cargada, proyección puntual C4 | 0,058849% | 5% |
| Cambio máximo de referencia espacial, siete puntos | 0,276164% | 1% |
| Error complejo contra grilla 256×192, siete puntos | 0,590889% | 2% |
| Referencia independiente Duffy / tiempo, máximo por punto | 8,46e−8 / 8,77e−8 relativo | 1e−5 |
| Presupuesto de estados vivos, máximo | 6,44e−16 relativo | 1e−9 |

La referencia espacial incluye 2416,667 Hz, punto que falló en el ensayo
inicial de 90 modos. La banda hasta 6 kHz evalúa la carga del aire de estas
59 formas, no una estructura mecánica con modos hasta 6 kHz.

| Fuerza RMS: error relativo | Grilla 192×144 | Grilla 256×192 | Cambio de referencia |
| --- | ---: | ---: | ---: |
| FrozenC4, trayectoria de 35 modos archivada | 1,602188% | 1,603161% | 0,056349% |
| C4Projected, 30 modos activos | 0,087510% | 0,192196% | 0,116241% |
| Distributed, 59 modos activos | 0,161068% | 0,321370% | 0,178787% |

El puerto distribuido activa los 59 modos, incluidos los 24 adicionales
respecto del control de 35. El máximo pico relativo después de 10 ms es
1,20e−16. Los tres presupuestos de estados arbitrarios tienen radiación
positiva a pasos 1/48, 1/192 y 1/768 kHz. La verificación tarda 210,594 s.
Los normalizadores y la parada prescrita permanecen iguales a los controles
anteriores; FrozenC4 sigue siendo el caso más próximo al límite del 2%.

## Banco de contacto preparado

El banco acepta `--cells` y la selección del prefijo de un archivo padre.
Comprueba grilla/tasa contra el reporte de carga antes de medir. Registra
`acoustic_discretization` y rechaza una comparación de bases con diferentes
resoluciones acústicas. Para archivos anteriores sin esa metadata, conserva
la configuración 128×96/64 kHz utilizada por los bancos compactos archivados.

La nueva fase `--phase preflight` carga y valida la configuración y calcula
la energía del estado inicial. No avanza el tiempo, no escribe trayectorias
y no crea archivos o directorios de resultados. Ambos preflights pasan:

| Base | Estados acústicos | Energía inicial | Grilla / factor |
| --- | ---: | ---: | --- |
| 59 modos | 38232 | 0,04693371904404984 J | 192×144 / 128 kHz |
| 90 modos | 58320 | 0,04693371904404984 J | 192×144 / 128 kHz |

En ambos se conserva C4/MIDI 60, velocidad 124, 72 modos transversales,
144 axiales, parámetros de cuerda/martillo, velocidad incidente
6,408955574 m/s y relanzamiento prescrito con estados vivos. Esta comprobación
de inicialización no equivale a los cuatro contactos temporales ni a su
balance acumulado. Pasan 20 tests: cuatro nuevos de selección/restricción
física, siete de aire racional, cuatro del completado y cinco de contacto.
Los nuevos tests contrastan impedancia/fuerza de un puerto con sus columnas
restantes en reposo, radiación y archivo inmutable, además del rechazo de
orden/dimensiones inconsistentes.

## Archivos y reproducción

Se reutiliza el único archivo de coeficientes
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-tabla-1500-refinada/Wave128000-Cells192x144-Angle96x96-Modes90.npz`,
con SHA-256
`586070dcdb91b14bad6d715a7ae4832586b610309773f9c6a3a57b53b96c9942`.
Los únicos arrays nuevos de esta etapa son `transfer.npz` y `memory.npz`
en `C:/SHARED/Fisica-pendiente-2026-10-05/P6-tabla-1200-refinada`.
Informes, código y tests conservan una sola copia en el repo; no hay WAV,
CSV de las mismas trayectorias, ZIP de fuentes ni otro factor almacenado.

Repetir la selección/carga usando `audit-concert-grand-extended-board.py`
en fases `build` y `verify`, las entradas del resumen y destinos nuevos:
`--cutoff 1200 --cells 192 144 --spatial-cells 256 192 --factor-rate 128000`,
`--parent-coefficients` con el archivo anterior, y `--parent-report` con
`resumen-P6-tabla-1500-refinada.json`. El archivo padre permanece inmutable.

Para repetir las inicializaciones con `audit-concert-grand-bounded-contact.py`,
usar las entradas SI de `P3-contacto-acoplado/input`, el mismo asset y el
archivo padre para `--coefficients` y `--mass`, más
`--cells 192 144 --relaunch --observer-probe-steps 64 --phase preflight`.
Cada base usa su corte 1200/1500 y su reporte de carga correspondiente.
Los argumentos `--output`, `--reports` y `--summary` deben ser destinos
nuevos y diferentes para cada banco. El preflight no escribe en ellos.
Para medir las trayectorias posteriores, reemplazar `preflight` por `run`;
el banco de 90 usará luego el de 59 como `--basis-reference/--basis-data`.

## Siguiente

Ahora ambas cargas y ambas inicializaciones comparten resolución validada.
Sigue medir los contactos frío/reiterado a 384/768 kHz de ambas bases,
balances independientes, continuidad y equivalencia del observador con
estados vivos; luego observación SI con la misma superficie 192×144 y
geometría de micrófonos. El observador anterior sigue configurado para
35/59 modos y celdas 128×96: habrá que generalizarlo y comprobar la metadata
antes de aplicarlo a los nuevos bancos.

Continúan pendientes sostenido, banco de notas/intensidades, pérdidas y
geometría identificadas, ambas caras/borde, unísono, P4, graves, acción/pedal
y reducción de coste para el plugin. No se afirma mejora audible desde
estos controles de carga e inicialización.
