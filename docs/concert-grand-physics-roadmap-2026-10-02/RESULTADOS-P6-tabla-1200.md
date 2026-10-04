# P6: carga acústica para 59 modos de tabla bajo 1200 Hz

Fecha: 2026-10-04. Base: `d35cb4a`, que conserva los contactos compactos
frío/reiterado de 35 modos. Esta etapa amplía la base mecánica del laboratorio
y vuelve a calcular su carga acústica. No modifica el plugin 0.172.0.

## Construcción

El mismo asset P2 de placa ortótropa, costillas y puente contiene 256 modos
hasta 2965,08 Hz. Se seleccionan los primeros **59**, hasta 1193,51 Hz, frente
a los 35 anteriores bajo 800 Hz. Su geometría y pérdidas siguen siendo las
hipótesis congeladas del piloto: no se identifican aquí desde el Steinway.

Se recalculan el factor de radiación, la masa estática y la resistencia límite
en la base completa. No se rellenan con ceros las matrices de carga de 35 modos.
El factor usa celdas 128×96, 64 kHz y polar/azimut 96×96: 326 taps originales,
327 tras el promedio adyacente, 9216 salidas angulares y **19293 estados**.
La relajación conserva residuos estrictamente positivos; no recorta eigenvalores.

Los primeros 35 canales del factor y el bloque principal de masa estática
coinciden exactamente con sus controles congelados. La suma DC relativa del
factor es 1,64e−16. La impedancia completada de ese bloque cambia hasta
0,003888%, porque la relajación matricial se calcula con la base ampliada.
Conservar el factor anterior no exige conservar idéntica esa aproximación.

## Transferencia y referencias independientes

La referencia geométrica integra la memoria de Rayleigh de las mismas celdas,
sin ajustar polos ni samples. Se contrasta con cuadratura espacial Duffy,
refinamiento temporal 128→256 kHz y una segunda grilla espacial 192×144.
Los nueve puntos independientes incluyen 60/261,63/1200/2000/4500/6000 Hz y
tres puntos de Laplace con parte real positiva. El resumen guarda los errores
globales y los máximos individuales de esos controles.

Los umbrales se fijan antes de medir: 2% en Z/R/X, Laplace y transferencia
alta; 5% en velocidad cargada; 2% RMS y 1% de pico tardío en fuerza; 1% de
cambio de referencia espacial; 1e−5 entre integrales independientes;
1e−9 en el balance energético. Son controles de este modelo geométrico.

| Medición | Resultado | Límite |
| --- | ---: | ---: |
| Máximo Z / R / X en 210 puntos de 60–1200 Hz | 0,673577% / 0,595975% / 0,764700% | 2% |
| Máximo complejo en tres puntos de Laplace | 0,616638% | 2% |
| Máximo complejo en 61 puntos de 1–6 kHz | 0,989030%, a 6 kHz | 2% |
| Máximo complejo en seis puntos contra grilla 192×144 | 1,321738% | 2% |
| Cambio máximo de referencia entre las dos grillas, seis puntos | 0,788156% | 1% |
| Velocidad modal cargada, proyección puntual de C4 | 0,234164% | 5% |
| Masa estática / resistencia límite completadas | 3,07e−16 / 2,93e−15 | 1e−10 |

El barrido bajo incluye cada resonancia estructural seleccionada. La velocidad
cargada compara la solución mecánica completa de 59 coordenadas con ambos
puertos; usa el punto de puente de C4 y la pérdida modal congelada 0,046.
No sustituye un banco de notas ni un ensayo de contacto no lineal.

Los 6 kHz son frecuencias de evaluación de la **carga del aire para estas
59 formas**, no una tabla con modos mecánicos hasta 6 kHz.

## Memoria y cobertura de la excitación

Se prescriben velocidades durante 4 ms a 128 kHz y después un puerto quieto
durante 60 ms. La fuerza se mide a 32 kHz. Se conserva la parada exacta y
no se recorta la salida ni se oculta la cola con un umbral.

- `FrozenC4`: trayectoria de 35 coordenadas del contacto C4/124 anterior,
  con las 24 entradas nuevas nulas. Se calcula la fuerza en las 59 salidas.
  Su normalizador es el baño original congelado del ensayo anterior.
- `C4Projected`: excitación modal amortiguada bajo la proyección del punto
  del puente de C4, con envolvente sin² de extremos nulos. La simetría hace
  casi nulas 29 proyecciones: activa 30 modos, 11 nuevos. No se presenta como
  cobertura de los 59 modos.
- `Distributed`: amplitud modal igual en todas las coordenadas, conservando
  la norma de amplitudes del ensayo anterior, con la misma envolvente y las
  frecuencias físicas de cada modo. Comprueba también los modos nodales del
  punto de C4. Es un puerto prescrito, no un martillo tocando un piano.

Para los dos ensayos sintéticos, el normalizador se obtiene de la referencia
geométrica antes de comparar el candidato. La segunda grilla utiliza los
mismos normalizadores. Las trayectorias y errores completos se conservan en
`memory.npz` y `resumen-P6-tabla-1200.json`.

El almacenamiento se verifica con estados acústicos arbitrarios vivos y
pasos de 1/48, 1/192 y 1/768 kHz. La identidad usa energía independiente,
trabajo de puerto y potencia radiada no negativa; no sólo el ajuste de Z.

| Fuerza: error RMS relativo | Referencia 128×96 | Referencia 192×144 | Cambio de referencia |
| --- | ---: | ---: | ---: |
| FrozenC4 | 0,561674% | 0,612466% | 0,159101% |
| C4Projected | 0,348864% | 0,641053% | 0,330733% |
| Distributed, 59 modos / 24 nuevos activos | 0,641665% | 1,089959% | 0,509426% |

El máximo pico de error después de 10 ms es 1,30e−16 relativo al pico inicial
de referencia, en ambas grillas. En los tres controles de estados arbitrarios,
el mayor error de presupuesto es **3,16e−16** y la potencia radiada es positiva.
Los **11 gates pasan**: subespacio, límites, referencia, transferencia baja,
Laplace, transferencia alta, velocidad cargada, cobertura de excitación,
memoria, refinamiento espacial y energía. La comparación geométrica independiente
por punto tiene máximo 7,38e−8, muy por debajo del umbral 1e−5.

La grilla más fina mueve la transferencia hasta 0,788156% y la fuerza hasta
0,509426%; ese cambio queda registrado. Se valida este escalón de resolución,
no una convergencia espacial del piano completo ni toda frecuencia continua.

## Archivos y reproducción

Informe y resumen: única copia en el repo. Los únicos nuevos datos externos
están en `C:/SHARED/Fisica-pendiente-2026-10-04/P6-tabla-1200`:
factor/masa/límite, `transfer.npz` y `memory.npz`. No hay CSV de las mismas
trayectorias, ZIP de fuentes ni informes duplicados en SHARED.

El SHA-256 del coeficiente es
`d7b4c0274f0b94498307076869a50ee47c8c57895e2f44b90b8ddc91d6245e5e`.
El resumen conserva huellas de entradas, fuentes y mediciones.
Pasaron 16 tests dirigidos: cinco del kernel geométrico, siete del aire
racional y cuatro de su completado limitado.

```powershell
$p6BoardArgs = @(
  '--asset', 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz',
  '--control', 'C:/SHARED/Fisica-pendiente-2026-10-04/P6-resolucion-aire/Polar96/Wave64000-Cells128x96-Angle96x96-Sparse.npz',
  '--mass-control', 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos/spectrum.npz',
  '--contact', 'C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto/T72-384000-Cold.npz',
  '--memory-control', 'C:/SHARED/Fisica-pendiente-2026-10-04/P6-kernel-geometrico/Cells128x96-Rate128000-Q16.npz',
  '--output', 'C:/SHARED/Fisica-pendiente-2026-10-04/P6-tabla-1200'
)
python tools/audit-concert-grand-extended-board.py --phase build @p6BoardArgs
python tools/audit-concert-grand-extended-board.py --phase verify @p6BoardArgs
```

`build` rechaza reemplazar coeficientes o resumen existentes. Para reproducir
desde cero, elegir destinos nuevos con `--output` y `--report`; `verify`
recalcula sólo las mediciones de esa construcción congelada. Usar el mismo
Python/NumPy/SciPy y un hilo BLAS; el resumen registra versiones. El informe
por defecto queda en el repo, cualquiera sea la ubicación del output NPZ.

## Alcance y siguiente paso

Esta etapa verifica el puerto acústico ampliado. Los contactos SI de
384/768 kHz ya medidos pertenecen a 35 modos; no se trasladan automáticamente
sus resultados a 59. Sigue acoplar el nuevo puerto al contacto frío/reiterado,
refinar tasas y verificar balances, luego observación SI, banco de notas e
intensidades y escucha con control. También quedan pérdidas identificadas,
ambas caras/borde, unísono, P4, graves y acción/pedal. No se afirma todavía
una mejora audible del cuerpo, el brillo o el ataque del Steinway.
