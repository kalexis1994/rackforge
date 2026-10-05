# P6: 90 modos y diagnóstico de resolución del aire

Construcción inicial: 2026-10-04. Verificación y diagnóstico: 2026-10-05.
Base: `68a508a`, comparación de presión del ataque con 35/59 modos.

## Hipótesis y resultado inicial

Ampliar la tabla hipotética P2 a 1500 Hz selecciona 90 modos, hasta
1491,645680 Hz. La geometría, frecuencias, pérdidas y formas intrínsecas
son las mismas; se añaden 31 coordenadas respecto de los 59 modos anteriores.
El ensayo inicial conserva celdas 128×96, factor temporal a 64 kHz y
cuadratura polar/azimutal 96×96. La carga tiene 29430 estados acústicos.

La construcción conserva exactamente el factor y el bloque de masa de los
35 modos archivados. Los límites completos de masa y resistencia tienen
errores relativos de 4,28e−16 y 4,14e−15. Eso no garantiza precisión en toda
la banda: **pasan nueve de los once controles; fallan transferencia alta y
refinamiento espacial**. Se conserva el ensayo rechazado y sus umbrales.

| Medición inicial | Resultado | Límite anterior |
| --- | ---: | ---: |
| Máximo Z/R/X, 241 puntos de 60–1500 Hz | 1,133934% / 1,012025% / 1,309842% | 2% |
| Máximo complejo, tres puntos de Laplace | 1,024073% | 2% |
| Máximo complejo, 61 puntos de 1–6 kHz | **2,243874% a 2416,667 Hz** | 2% |
| Velocidad cargada en la proyección de C4 | 0,301850% | 5% |
| Cambio máximo de referencia, seis puntos entre grillas | **1,180121%** | 1% |
| Error complejo contra grilla 192×144, seis puntos | **2,472209%** | 2% |
| Fuerza distribuida RMS, grilla primaria / fina | 0,943629% / 1,548853% | 2% |

El puerto distribuido activa las 90 coordenadas; el punto central de C4
activa 47. La referencia geométrica usa la integral de celdas con
singularidad tratada independientemente; su comprobación por punto alcanza
9,93e−8 frente a la integral Duffy y 1,43e−7 al refinar tiempo. La fuerza
prescrita incluye 60 ms de puerto quieto; no es un sostenido no lineal de piano.

La banda hasta 6 kHz evalúa la carga para estas 90 formas; no afirma modos
mecánicos hasta 6 kHz. No se ejecutan contactos con la carga rechazada.

## Separar fuentes de error

El diagnóstico reproduce la frecuencia que falló y 6 kHz mediante integrales
geométricas en 128×96, 192×144 y 256×192, y un Gram hemisférico de celdas
exactas con cuadraturas 96×96 y 192×192. Sus números están en
[resumen-P6-discretizacion-1500.json](resumen-P6-discretizacion-1500.json).

| Diagnóstico | 2416,667 Hz | 6000 Hz |
| --- | ---: | ---: |
| Cambio espacial 128×96 → 192×144 | 1,369258% | 0,980985% |
| Cambio espacial 192×144 → 256×192 | 0,483489% | 0,345260% |
| Cambio angular 96×96 → 192×192 | 9,01e−13 relativo | 0,005816% |
| Gram angular fino frente a resistencia Duffy | 1,16e−13 relativo | 3,38e−10 relativo |
| Error real de resistencia del candidato | 1,866292% | 0,489970% |
| Error real de reactancia del candidato | 3,966714% | 592,827748% |

El último porcentaje tiene una referencia de reactancia muy pequeña a 6 kHz:
el error **complejo total** allí es sólo 0,901533%. No usar la división por
esa componente débil como un indicador de volumen o de percepción.

La conversión Cayley de los retardos usa
`theta = 2*atan(omega/(2*rate))`. El factor de derivada, después del promedio
de taps adyacentes, es `rate*sin(theta)`. Por eso se compara la resistencia
geométrica a la frecuencia `rate*theta/(2*pi)`, multiplicada por
`(sin(theta)/theta)^2`. Es una predicción del Gram de ondas, **no una nueva
carga completa aceptada**: deja fuera el histograma y el completado de masa.

A 2416,667 Hz, el factor de 64 kHz usa una frecuencia efectiva de
2405,425 Hz. La predicción de resistencia de ondas cambia 2,465994% frente
al objetivo; a 96 y 128 kHz cambia 1,102330% y 0,621281%. La onda real del
candidato difiere 0,463660% de la predicción a 64 kHz. La relajación que
completa la masa cambia nuevamente la resistencia. A 6 kHz también se
compensan errores: no se puede atribuir todo el error complejo a una sola
fuente ni aceptar otra tasa sólo por esta predicción.

Estos controles señalan discretización espacial y temporal como prioridades
antes de ampliar la cuadratura angular. Son dos frecuencias diagnósticas,
no una demostración de convergencia en toda frecuencia continua.

## Ensayo refinado

Se construye el mismo soporte de 90 modos con celdas 192×144 y factor a
128 kHz; se compara con integral geométrica 256×192. Se conservan todos los
umbrales. La lista espacial incluye desde antes de medir el punto fallido
2416,667 Hz y el corte 1500 Hz, además de los seis puntos anteriores.

Cambiar celdas/tasa modifica la representación acústica de los modos comunes.
El control de subespacio se regenera a la resolución del candidato y se
etiqueta explícitamente: no se afirma que el factor discretizado anterior
permanezca idéntico. La diferencia de masa respecto del archivo anterior se
registra por separado. La estructura intrínseca sí conserva su base física.

El candidato refinado **pasa los once controles** con los mismos umbrales.
Tiene 58320 estados acústicos; los límites de masa/resistencia cierran a
3,62e−16 / 4,50e−15. El bloque de masa de los 35 modos cambia 0,127797%
frente al archivo de celdas antiguas, y coincide exactamente con su
construcción independiente a la nueva resolución.

| Medición refinada | Resultado | Límite |
| --- | ---: | ---: |
| Máximo Z/R/X, mismos 241 puntos de 60–1500 Hz | 0,285996% / 0,256458% / 0,328458% | 2% |
| Máximo complejo en Laplace | 0,258436% | 2% |
| Máximo complejo, mismos 61 puntos de 1–6 kHz | 0,571678%, a 2416,667 Hz | 2% |
| Velocidad cargada, proyección C4 | 0,075614% | 5% |
| Cambio máximo de referencia espacial, ocho puntos | 0,485578% | 1% |
| Error complejo contra grilla 256×192, ocho puntos | 0,861880% | 2% |
| Comprobación de referencia Duffy / tiempo, máximo por punto | 9,48e−8 / 1,48e−7 relativo | 1e−5 |
| Presupuesto de energía en estados vivos, máximo | 2,14e−16 relativo | 1e−9 |

| Fuerza RMS: error relativo | Grilla 192×144 | Grilla 256×192 | Cambio de referencia |
| --- | ---: | ---: | ---: |
| FrozenC4 | 1,691295% | 1,692054% | 0,057943% |
| C4Projected, 47 modos activos | 0,157326% | 0,307975% | 0,177064% |
| Distributed, 90 modos activos | 0,237616% | 0,455971% | 0,254526% |

El control FrozenC4 conserva el normalizador original y su trayectoria,
incluida la parada prescrita; no todos los errores de fuerza bajan al refinar.
Es el control más cercano al umbral del 2%. Se conserva también su resultado
en ambas grillas. El mayor pico relativo después de 10 ms es 1,21e−16.
La positividad de las potencias y el balance se prueban con estados
arbitrarios a pasos de 1/48, 1/192 y 1/768 kHz.

La verificación refinada tarda 345,375 s, además de construcción/compresión.
Pasaron los 34 tests dirigidos de observación, contacto, kernel geométrico,
aire racional y completado limitado. El factor refinado ocupa 2,515 GB y
su SHA-256 es
`586070dcdb91b14bad6d715a7ae4832586b610309773f9c6a3a57b53b96c9942`.
El resumen completo es
[resumen-P6-tabla-1500-refinada.json](resumen-P6-tabla-1500-refinada.json).

Esto valida este puerto acústico ampliado dentro de los controles declarados.
No demuestra convergencia de la tabla completa ni convierte 58320 estados en
un diseño viable para el hilo de audio. Mecánica no lineal, observación,
escucha y reducción con control de coste siguen siendo etapas distintas.

## Archivos, reproducción y alcance

Los informes tienen única copia en este directorio. Sólo coeficientes y
arrays numéricos únicos quedan en SHARED:

- `C:/SHARED/Fisica-pendiente-2026-10-04/P6-tabla-1500`: construcción rechazada,
  transferencia y memoria, sin modificar el paquete instalado.
- `C:/SHARED/Fisica-pendiente-2026-10-04/P6-discretizacion-1500`: matrices nuevas
  de los diagnósticos, sin duplicar trayectorias completas ni informes.
- `C:/SHARED/Fisica-pendiente-2026-10-05/P6-tabla-1500-refinada`: construcción
  de mayor resolución y sus controles independientes.

Para repetir, usar las entradas del resumen y destinos nuevos. La construcción
inicial usa `audit-concert-grand-extended-board.py --cutoff 1500` en fases
`build` y `verify`. El refinamiento agrega `--cells 192 144 --spatial-cells
256 192 --factor-rate 128000`, con `--report` y `--output` diferentes. El
diagnóstico usa `audit-concert-grand-air-discretization.py`, el asset, los
coeficientes iniciales y su resumen mediante `--load-report`. Todos registran
SHA-256 de entradas/fuentes/mediciones; Python 3.12.14, NumPy 2.3.5,
SciPy 1.18.1, un hilo BLAS. No se generan WAV ni copias de fuentes en SHARED.

La versión del verificador del primer ensayo es la de `68a508a` con dos
cambios: agregar `--baseline-commit` con default `d35cb4a` y usar
`args.baseline_commit` en el reporte. Su huella se reconstruyó y verificó
desde ese commit; conservaba los seis puntos espaciales originales. La
versión refinada, con grillas/tasa parametrizadas y ocho puntos espaciales,
registra la huella del archivo actual. Se verificaron 31 huellas de entradas,
fuentes, mediciones y paquete durante el cierre de la etapa.

Antes de la siguiente comparación de contacto/presión, ambas bases 59 y 90
deben usar la resolución acústica validada. Comparar directamente contactos
de 59 modos/celdas antiguas con 90 modos/celdas nuevas mezclaría truncamiento
estructural y refinamiento numérico. Continúan pendientes sostenido, banco de
notas/intensidades, pérdidas/geometría identificadas, ambas caras/borde,
unísono, P4, graves y acción/pedal. El paquete 0.172.0 conserva su huella;
estos controles no afirman acercamiento audible al Steinway.
