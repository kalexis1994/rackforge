# P6: reacción directa del aire sobre la tabla proyectada

2026-10-05. La reducción mecánica anterior queda en **379cc51**, por pedido
del usuario. Se continúa con la carga recíproca del aire sobre las mismas
formas de la tabla padre de 7392 coordenadas y su proyección de 606.
Datos nuevos únicos en `local/`; informes y fuentes únicos en el repo.

## Qué se implementó

Se usa la integral de Rayleigh del
[control de impedancia de superficie](RESULTADOS-P6-impedancia-tabla.md),
incluida la integración local singular por Duffy. Aire con ρ = 1,2 kg/m³ y
c = 343 m/s; una cara plana en un baffle rígido infinito. La geometría de
placa/costillas/puente sigue siendo supuesta, no una tabla medida del Steinway.
La referencia y el candidato reciben ambos la reacción distribuida del aire.
La carga no se usa sólo como observador de un movimiento seco.

Con la convención `s = i*omega`, se resuelve:

```text
(K + s*C + s²*M + s*Z(s))*q = B
v = s*q
F_aire sobre la madera = -Z(s)*v
```

`Z(s)` conserva todos los términos mutuos de la integral saliente. La misma
transformación de modos a superficie se usa con su transpuesta para devolver
la fuerza. Los promedios analíticos de las funciones Ritz se mantienen en
celdas constantes; refinar las celdas refina esta aproximación de superficie.
No se introducen resistencias por nota ni modos acústicos ajustados a muestras.

`concert_grand_matrixfree_air.py` calcula el movimiento por contracciones
separables en x/y, aplica la convolución de celdas con FFT y proyecta la fuerza
de vuelta. No ensambla una matriz modal de carga de 7392² ni una matriz de
interacción entre todas las celdas. La cola mecánica conserva su matriz de
pérdidas completa, igual que en la proyección validada.

## Solución en bloque y controles numéricos

Se resuelven juntos los once puertos C4/E4/G4/C5 y los siete reservados de
la etapa previa: MIDI 36/48/53/65/69/84/96. Block GMRES comparte operaciones
de superficie entre fuerzas distintas, con Arnoldi y dos pasadas de
ortogonalización. El precondicionador es la dinámica mecánica seca completa:
una ayuda para resolver las ecuaciones, no una eliminación de la carga.

La estimación de mínimos cuadrados no alcanza para aceptar una solución.
Se evalúa de nuevo el residual de **las fuerzas originales** y se exige
máximo relativo < 2e−10. Si la iteración agota su presupuesto, se rechaza.
No se recortan autovalores para reparar resistencia o rigidez.

Para fuentes armónicas se comprueba por puerto:

```text
Potencia media de entrada = 0,5*Re(v* B)
Calor viscoso = 0,5*Re(v* C v)
Potencia enviada al aire = 0,5*Re(v* Z(s) v)
Entrada = calor + aire
```

Este balance es de potencia armónica. No es todavía un balance de energía
de estados acústicos vivos en el tiempo.

Cinco pruebas nuevas pasan:

- Mapa separable y transpuesta contra formas de celdas explícitas; carga
  contra matriz modal densa en eje imaginario y semiplano derecho.
- Solución cargada contra solve denso y SciPy GMRES independiente, potencia
  angular y presión de micrófono con cuadratura independiente.
- Dinámica de la cola con pérdidas completas contra el sistema matricial.
- Rechazo del intento de omitir una paridad no nula.
- Solver en bloque contra un sistema complejo no normal; agotamiento de
  iteraciones rechazado. Cambiar el tamaño del bloque angular conserva potencia.

## Preflight

[Tres puntos iniciales](medidas-P6-carga-directa/preflight.json): 60;
1881,960712; 6000 Hz, en las once notas. Pasan transferencias cargadas,
residual, balance de potencia, reciprocidad y puertos disipativos. A 6 kHz
el error máximo de admitancia es 0,25524%, y el de presión 0,39955%.
Tarda 33,375 s y alcanza 2.455.916.544 bytes de working set de proceso
(2,29 GiB). Este preflight **no aprueba** las verificaciones espaciales y
angulares: esos controles no se ejecutaron en él.

## Corrida ampliada

[Mediciones](medidas-P6-carga-directa/carga.json). Se declaran 83 controles
armónicos entre 60 y 6000 Hz: grilla geométrica, resonancias seleccionadas de
la tabla completa y ±1%, fundamentales de las once notas y puntos de
refinamiento. Son controles discretos; no constituyen un barrido denso ni
una cota para todas las frecuencias intermedias.

Criterios establecidos antes de la corrida:

- Máximo error complejo por puerto de entrada, en cada frecuencia: 1% para
  admitancia (once puertos observados) y presión (dos cápsulas).
- Refinamiento espacial 192×144 → 256×192: 1%, en ocho puntos declarados.
- Potencia del aire frente a integral angular independiente: 1%, en cinco
  puntos. Hemisferio 96×256; referencia angular 128×320 con cambio < 1%.
- Cuadratura de carga 8→12 y de micrófonos 4→8: 0,1%, en tres puntos.
- Residual de fuerza 2e−10; balance de potencia 1e−8; reciprocidad 1e−8;
  mínimo relativo de disipación de puertos ≥ −1e−10 (sólo redondeo).

La presión se obtiene del Green retardado continuo de las mismas celdas,
sin discretización temporal del kernel de audio. La integral angular evalúa
exactamente la apertura rectangular de cada celda mediante sinc y sumas
separables. Se aplica al **movimiento complejo cargado**, no sólo a una forma
real aislada. Los bloques angulares acotan memoria temporal sin quitar direcciones.

La corrida completa de 192×144 **no se aprueba**: pasa nueve de diez
controles, pero falla el refinamiento espacial de presión a 6 kHz.

| Control | Resultado |
| --- | ---: |
| Proyección cargada, máximo de admitancia por puerto/frecuencia | 0,255239% |
| Proyección cargada, máximo de presión | 0,618522% |
| Refinamiento espacial 192×144 → 256×192, presión a 6 kHz | **1,018371%**, límite 1% |
| Residual de fuerzas originales, máximo | 9,973e−11 |
| Balance armónico de potencia, máximo | 1,615e−10 |
| Integral angular y su refinamiento | pasan; diferencias de orden 1e−14 |
| Cuadratura de carga y micrófonos | pasa |

La ejecución tarda 850,516 s; peak working set de proceso 2.933.538.816
bytes (2,73 GiB). Se conserva el ensayo fallido sin cambiar su umbral ni
presentarlo como referencia espacial aprobada. Transferencias pequeñas con
SHA256 `5edd88d12eeab8fdf0ab561522d913b33da1c6f71ff09f219a6693329c8acbea`.
El siguiente refinamiento parte de 256×192 y lo contrasta con 320×240.

## Datos y reproducción

Se guardan únicamente transferencias complejas y metadatos. No se genera
un factor angular FIR grande ni se duplican los factores anteriores 59/90.
La referencia de una cara, sin aro/sala/segunda cara, sigue siendo el mismo
alcance físico que el control P6 de Rayleigh.

Desde el repo, con Python/NumPy/SciPy y BLAS de un hilo; usar rutas nuevas:

```powershell
python -m unittest tools/tests/test_concert_matrixfree_air.py
python tools/audit-concert-grand-loaded-projection.py --mechanical-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-alta/simetria.json --projection-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-proyeccion-pasiva/espacial.json --state-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-proyeccion-pasiva/estado.json --output local/concert-grand-physics/P6-carga-directa-repeticion --report local/concert-grand-physics/P6-carga-directa-repeticion.json
```

`--limit-frequencies 3` ejecuta sólo un preflight y lo marca como tal; no
permite aprobar la corrida completa. Cada informe conserva hashes de
entradas/fuentes, los eventos de solver, potencias y errores por puerto.
Los controles de esta etapa tienen una única ubicación en
`local/concert-grand-physics/P6-carga-directa[-preflight]-2026-10-05/`.

## Qué sigue

La herramienta actual es una referencia **continua en frecuencia**. No
tiene estados causales de aire ni ejecuta notas en tiempo real. Antes de
otra escucha deben construirse una realización temporal pasiva verificable
y la integración del contacto/sostenido/apagador con las pérdidas completas
de cola. Hay que medir masa/resistencia/reactancia, transitorios y energía
frente a esta referencia, sin reutilizar una carga de la base antigua.

Punto concreto para la integración siguiente: `MovingBridge` aún recibe
`Support.loss` diagonal. Sus componentes, `BridgeHammer.interface` y
`BoundedContact.interface` calculan fuerza/calor con `core.viscous` por
componente. No alcanza con cambiar sólo la matriz dinámica del integrador:
los diagnósticos también deben usar `C_board @ v` y `v.T @ C_board @ v`.
El coast exponencial ya opera con una matriz `Db`, pero la construcción del
soporte y esos balances deben generalizarse antes de aceptar la nueva tabla.

Se conservarán estos datos pequeños y los factores necesarios una sola vez,
con presupuesto de memoria/disco antes de construir un factor grande.
Siguen abiertos ambas caras/borde, identificación del instrumento, unísono,
P4, acción/pedal y ejecución en tiempo real. No se produce otro WAV ni se
cambia el paquete 0.172.0 a partir de una medición armónica.
