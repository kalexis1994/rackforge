# P6: resolución de tabla y presión hasta 6 kHz

2026-10-05. Seguimiento del rechazo de la frase SI por falta de definición,
brillo y apagado de tecla. Se estudia la cobertura estructural antes de volver
a renderizar. Los audios recientes se eliminan por pedido del usuario;
[manifiesto](limpieza-2026-10-05/shared-manifest.json). Los datos nuevos se
guardan sólo en `local/concert-grand-physics/`, los informes y fuentes en el
repo. No se cambia la calibración ni el paquete 0.172.0.

## Alcance del control

Se mantiene la geometría **supuesta** P2: placa ortótropa de 1,4 × 1 × 0,008 m,
nueve costillas paralelas a y y puente recto paralelo a x sobre y = 0,5 m.
Bordes empotrados. No se identifica la tabla real del Steinway ni se añaden
curvatura, pretensión, torsión, excentricidad de vigas o elasticidad del aro.
La pérdida viscosa modal sigue siendo `0.046 * omega`, sin identificar.

Los cuatro puertos verticales corresponden a C4/E4/G4/C5, sobre el mismo
puente. Dos micrófonos en (0,55; 0,5; 0,66) y (0,85; 0,5; 0,66) m. Se calcula
admitancia velocidad/fuerza en los puertos y presión/fuerza mediante la
integral retardada de la misma superficie. **Sin realimentación de carga del
aire, cuerda ni martillo**: es un requisito estructural, no un piano audible.

Se comparan respuestas complejas en 1616 frecuencias entre 60 y 6000 Hz.
La métrica integra el cuadrado del error y de la referencia por banda,
sumando todos los puertos/cápsulas, y toma la raíz de su cociente. El límite
de refinamiento es 1% en cada banda: 60–1200, 1200–3000 y 3000–6000 Hz.
Es una norma integrada; no exige 1% en cada frecuencia o cada nota individual.
No certifica frecuencias mayores de 6 kHz ni equivalencia perceptual.

El observador separable `concert_grand_basis_readout.py` proyecta el mismo
histograma de celdas con dos contracciones tensoriales. Dos pruebas verifican
equivalencia con el observador modal anterior e independencia de ampliar la
base. No se introduce un filtro tonal ni se cambia distancia/ganancia.

## Control completo y truncamiento

[Medición completa](medidas-P6-banda-alta/estructura.json). Se resuelven todos
los modos de bases 52×32, 60×40, 68×48 y 76×56. El último sistema tiene 4256
coordenadas, 584 resonancias por debajo de 6 kHz; el archivo P2 anterior sólo
guardaba 256 modos hasta 2965,08 Hz de una base 52×32.

| Cambio 68×48 → 76×56 | 60–1200 Hz | 1200–3000 Hz | 3000–6000 Hz |
| --- | ---: | ---: | ---: |
| Admitancia | 0,0579% | 0,3875% | 1,3738% |
| Presión/fuerza | 0,0773% | 0,9172% | 4,7128% |

La estructura **no pasa**. El cambio de celdas 192×144 → 256×192 sí pasa:
presión 0,0062% / 0,1134% / 0,4663%. Positividad de masa/rigidez,
normalización, residual modal, reciprocidad y disipación también pasan.

La ablación de corte modal compara contra los 4256 modos del mismo sistema
76×56; esa referencia tampoco está convergida espacialmente en banda alta.

| Corte | Modos | Error admitancia 3–6 kHz | Error presión 3–6 kHz |
| --- | ---: | ---: | ---: |
| 6 kHz | 584 | 25,8316% | 19,0061% |
| 9 kHz | 927 | 8,9215% | 1,8921% |
| 12 kHz | 1290 | 5,1700% | 0,5296% |
| 18 kHz | 1870 | 2,6820% | 0,1530% |

Ningún corte ensayado pasa el 1% simultáneo. Los modos altos aportan
flexibilidad fuera de resonancia al puente; su frecuencia propia por encima
de la banda no permite descartarlos sin controlar esa contribución. Esto
no establece que sus frecuencias extremas representen físicamente una tabla
real: pertenecen a la discretización de esta idealización de placa/vigas.

## Hipótesis de ancho de costillas

[Medición separada](medidas-P6-banda-alta/ancho-finito.json). Las vigas P2
declaran ancho, pero su masa/rigidez se concentran en una línea. Se ensaya
distribuir uniformemente la misma masa lineal y EI sobre ese ancho. Es una
idealización de fibras de flexión longitudinal, no un sólido tridimensional
ni una corrección calibrada de las costillas reales.

Tres pruebas verifican el límite de ancho cero exactamente igual al control,
cuadratura independiente de energía y masa/rigidez positivas con admitancia
pasiva. El refinamiento 68×48 → 76×56 cambia la presión alta un **5,9275%** y
la admitancia alta un **6,7265%**. No resuelve la convergencia; no se selecciona
como nueva geometría. Se conserva separado para no atribuir a resolución lo
que también cambia el mecanismo físico.

## Simetría exacta del piloto vertical

[Medición de simetría y refinamiento](medidas-P6-banda-alta/simetria.json).
La geometría supuesta, fuerzas verticales y micrófonos tienen simetría
respecto de y = 0,5 m. Con estado inicial antisimétrico cero, las coordenadas
antisimétricas no se excitan. Se conserva el bloque invariante simétrico y
**todas** sus frecuencias; no se hace un corte tonal.

En la base empotrada `sin(m*pi*y/W)*sin(pi*y/W)`, los índices m impares son
simétricos. `concert_grand_symmetric_plate.py` ensambla directamente ese
bloque, calcula sus modos y los vuelve a expresar en las coordenadas Ritz
completas para observar la superficie. No construye las matrices del bloque
antisimétrico. Tres pruebas comprueban bloque exacto, desacoplamiento de
paridades, respuesta contra el sistema completo y rechazo de puente asimétrico.

En 76×56 se pasa de 4256 a 2128 coordenadas. Frente al cálculo completo
congelado, el máximo error integrado de las seis métricas es **2,6521e−10**
(fracción relativa). La etapa solve/controles/respuestas/guardado tarda
9,328 s frente a 28,797 s del control completo en esta ejecución. No es una
medición de ejecución en tiempo real ni un benchmark repetido aislado.

Esta selección **no es válida** para fuerzas/momentos laterales con componente
antisimétrica, puentes o geometrías asimétricos, ni un estado inicial de esa
paridad. Un micrófono fuera del centro puede observar el campo del sector
simétrico; por sí solo no excita el sector mecánico omitido. El eventual
modelo completo debe conservar ambos sectores cuando los mecanismos lo
requieran. La simetría de este piloto no es una propiedad medida del Steinway.

El refinamiento se detiene en 132×112, con 7392 coordenadas activas y 296
resonancias activas por debajo de 6 kHz. Esas 296 no reemplazan al sistema:
las transferencias conservan todos sus modos, incluidos los de alta frecuencia.

| Cambio 124×104 → 132×112 | 60–1200 Hz | 1200–3000 Hz | 3000–6000 Hz |
| --- | ---: | ---: | ---: |
| Admitancia | 0,00395% | 0,03546% | 0,09488% |
| Presión/fuerza | 0,00637% | 0,08726% | 0,43455% |

Las seis métricas pasan el límite del 1%. Normalización de masa 2,822e−13,
residual modal 1,783e−9: pasan sus límites de 1e−8. El control anterior
108×88 → 124×104 aún fallaba presión alta (1,03049%), por lo que no se
había aceptado. La verificación espacial del observador 192×144 → 256×192
también pasa: 0,00620% / 0,11307% / 0,46502%. Los tres controles finales
son positivos: equivalencia de simetría, refinamiento mecánico y observador.
La ejecución completa tarda 541,719 s. Se conserva el activo estructural
con SHA256 `ecee61e1c1f9503dca79a67ff49cdd93abc4d8beda9fdc1458d7044856205af9`.
No se genera audio con la respuesta sin carga para presentarlo como piano.

## Pendientes antes de otra escucha

1. Comprimir la contribución modal alta preservando flexibilidad, masa y
   disipación positivas, con control contra la respuesta completa. Una
   proyección Galerkin con enriquecimiento de flexibilidad es una hipótesis
   futura, **todavía no implementada ni aprobada**. Debe conservar la matriz
   de pérdidas proyectada, no reemplazarla silenciosamente por su diagonal.
2. Construir y verificar carga acústica recíproca para esa misma superficie
   y proyección. Los factores antiguos de 59/90 no corresponden a la nueva
   base; no se pueden reutilizar como si fueran su carga.
3. Repetir contactos/sostenidos/apagado de tecla, refinamiento y energía;
   después la misma frase con ganancia común, conservando un control.

El marco de proyección estructural con disipación se revisa en
[este trabajo sobre reducción de sistemas mecánicos](https://arxiv.org/abs/2010.06331).
La pasividad de una proyección no garantiza que conserve timbre o precisión:
hay que medir las transferencias y el sistema acoplado. Siguen abiertos
identificación, unísono, ambas caras/borde, P4, acción/pedal y coste runtime.

## Reproducción y datos

Fuentes principales:
`tools/audit-concert-grand-high-band-board.py`,
`tools/audit-concert-grand-strip-band.py`,
`tools/audit-concert-grand-symmetric-band.py`, y sus módulos/pruebas.
Los JSON registran hashes de fuentes y entradas, criterios, métricas y
hashes de las salidas. No se modifican módulos congelados durante el cálculo.

Desde el repo, con Python/NumPy/SciPy y BLAS de un hilo:

```powershell
python tools/audit-concert-grand-high-band-board.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --output local/concert-grand-physics/P6-banda-alta-repeticion --report local/concert-grand-physics/P6-banda-alta-repeticion.json --max-grids 4
python tools/audit-concert-grand-strip-band.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --output local/concert-grand-physics/P6-ancho-finito-repeticion --report local/concert-grand-physics/P6-ancho-finito-repeticion.json --max-grids 4
python tools/audit-concert-grand-symmetric-band.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --parent-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-alta/estructura.json --output local/concert-grand-physics/P6-simetria-repeticion --report local/concert-grand-physics/P6-simetria-repeticion.json --max-grids 5
```

Cada comando exige rutas nuevas y se niega a sobrescribir resultados. La
tercera reproducción necesita el control `Transfer-76x56.npz` referido por
el informe padre, con su hash comprobado. Los datos de esta etapa tienen
una única ubicación bajo `local/concert-grand-physics/`:

- `P6-banda-alta-2026-10-05/`: control completo y cuatro transferencias.
- `P6-ancho-finito-2026-10-05/`: hipótesis distinta y cuatro transferencias.
- `P6-simetria-banda-alta-2026-10-05/`: refinamientos del bloque invariante.

Los tres sistemas estructurales son ensayos distintos. No son copias del
factor acústico anterior ni nuevos paquetes instalables.

Pasan ocho pruebas nuevas (dos de observador, tres de ancho finito y tres
de simetría). Reproducción dirigida:

```powershell
python -m pytest -q tools/tests/test_concert_basis_readout.py tools/tests/test_concert_finite_strip_plate.py tools/tests/test_concert_symmetric_plate.py
```
