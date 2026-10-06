# P6: Gram espectral real de las parejas de ondas

2026-10-06. El refinamiento angular y su condición de masa quedan en
**d2ab0c6**, por pedido del usuario. Esta etapa construye la misma onda
96×256/128 kHz, con las 606 coordenadas y la agrupación par en y ya comprobada.

## Identidad y cálculo

Las parejas temporales cumplen exactamente `H_opuesto = -H_directo[::-1]`.
Su transformada es el conjugado de la transformada directa multiplicado
por una fase común de inversión y el signo negativo. La fase y el signo
desaparecen en el Gram. Para cada frecuencia:

```text
Gram_pareja = F.H F + F_opuesto.H F_opuesto
            = 2 Re(F.H F)
            = 2 (Re(F).T Re(F) + Im(F).T Im(F))
```

Se transforma sólo un canal de cada pareja. Dos productos `dsyrk` acumulan
el triángulo inferior de la matriz real por frecuencia; se reconstruye el
superior antes de la FFT inversa. No se aproxima el rango ni se descartan
términos entre modos. La implementación comprueba inversión exacta antes
de usar esta identidad y mantiene la ruta compleja como control.

La impedancia continúa siendo **compleja**: la reactancia sale del polinomio
causal de las correlaciones y del completado por relajación. El Gram real
no elimina esa reactancia, ni cambia el mapa de Cayley o la tasa del factor.

## Pruebas de equivalencia

Dos pruebas nuevas cubren:

- Correlaciones, masa, impedancia en eje imaginario y semiplano derecho,
  fuerza midpoint, energía de estados activos y balance trabajo/pérdida.
  Se comparan contra la ruta compleja con agrupación activada/desactivada
  y con/sin promedio de taps; tolerancia relativa 1e−11.
- Rechazo de parejas que no sean inversiones exactas y presupuesto de
  memoria insuficiente antes de reservar el Gram.

Pasan **diez pruebas** dirigidas: siete de construcción/certificado y tres
de masa acumulada. El generador de energía/radiación sigue evaluando el
mismo factor físico; no se sustituye por una pérdida inferida del balance.

## Recursos y alcance

Para 96×256/128 kHz, el Gram ocupa **1.906.689.312 bytes**, la mitad del
Gram complejo. Las correlaciones ocupan 1.903.751.424 bytes. El presupuesto
estimado de arreglos es 4.343.875.872 bytes (4,046 GiB), menor que el límite
declarado de 5 GiB. El working set del proceso se mide por separado.

El factor agrupado completo ocuparía 38.602.801.152 bytes; el no agrupado,
77.205.602.304 bytes. Ninguno se almacena. Cada bloque temporal ocupa
50.264.064 bytes. Siguen presentes el coste modal denso de las correlaciones
y la regeneración de factores para certificar energía. No es una medición
de rendimiento del plugin ni de render musical.

## Preflight completo de las 606 coordenadas

[Informe](medidas-P6-aire-bloques/carga96-real.json). **Rechazado por
precisión**, con los mismos umbrales. El completado finito y la pasividad
pasan. Las pruebas iniciales de masa se reproducen desde las correlaciones
completas, pero no bastaban para predecir el error en alta frecuencia.

| Hz | Error Z | Error R | Error X | Admitancia máxima | Presión máxima |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 60 | 0,010250% | 0,040323% | 0,000432% | 0,000335% | 0,000339% |
| 1881,960712 | 0,701313% | 0,802243% | 0,624401% | 0,049952% | 0,257489% |
| 6000 | **10,012998%** | **8,618405%** | **12,349082%** | 0,231594% | **1,058836%** |

El máximo de presión que falla a 6 kHz corresponde al puerto C4; se conservan
los once valores en el informe. El error modal de impedancia es mayor que el
de los puertos cargados, pero no se cambia el umbral para aceptar esa diferencia.
Los autovalores disipativos mínimos relativos son positivos en los tres
puntos. El residual mínimo de masa sigue siendo aproximadamente 1,283e−5.

Tiempo **666,281 s** (11,10 min), working set máximo **4.611.059.712 bytes**
(4,294 GiB). El ensayo 32×64 complejo anterior alcanzaba 6,10 GiB; cambia
también la cuadratura, por lo que los tiempos no son un benchmark aislado
de la optimización. Se escribe sólo el informe pequeño; no se guarda otro
factor ni WAV ni paquete, y el instrumento instalado conserva su versión.

El siguiente diagnóstico separa la integral angular exacta de los efectos
del mapa de Cayley, la derivada/promedio de taps y los histogramas temporales.
Su contracción de sondeo conserva las once respuestas cargadas a 6 kHz y
añade ocho vectores aleatorios fijos. No reduce el candidato completo ni
sustituye su puerta de precisión.

La construcción se contrasta contra Rayleigh continuo de la misma
superficie 256×192, usando el cambio modal y las pérdidas completas de la
tabla. Los criterios del preflight permanecen: completado pasivo de ambos
límites, Z/R/X < 2%, admitancia/presión por puerto < 1% y mínimo disipativo
relativo ≥ −1e−10. Los tres puntos son 60, 1881,960712 y 6000 Hz.

## Diagnóstico de resistencia a 6 kHz

[Informe](medidas-P6-aire-bloques/resistencia-tasas.json). El banco real tiene
30 direcciones ortonormales: partes real/imaginaria de las once respuestas
cargadas continuas y ocho sondas aleatorias con semilla 61006. El error de
representar esas respuestas en el banco es 1,614e−15. Se conserva la geometría,
masa, rigidez, pérdidas y cuadratura 96×256; no se identifica ningún coeficiente.

La integral analítica de cada rectángulo y el Gram angular continuo dan
**5,758e−15** de diferencia relativa contra la resistencia Rayleigh directa
contraída. La cuadratura angular está resuelta al nivel de redondeo **en este
banco a esta frecuencia**; no se generaliza esa medida a todo el espacio modal.
Dos pruebas nuevas contrastan la integral con el hemisferio completo anterior
(con/sin agrupación), y la evaluación del factor en una sola frecuencia con
la impedancia causal completa. Pasan doce pruebas dirigidas en total.

Para tasa `r`, frecuencia física `omega` y mapa de Cayley:

```text
theta = 2 atan(omega/(2*r))
omega_efectiva = r*theta
R_predicha = (r*sin(theta)/omega_efectiva)^2 * R_angular(omega_efectiva)
```

El factor `r*sin(theta)` es la magnitud conjunta de la diferencia hacia
atrás y el promedio adyacente. La diferencia entre el factor de histogramas
y esa predicción mide la discretización de bins adicional. Se evalúan campos
de celda exactos en las frecuencias física/efectivas; no se ajusta la fórmula
al error observado.

| Tasa del factor | Frecuencia efectiva | Sólo warp | Warp + diferencia/promedio | Histograma adicional | Onda vs Rayleigh |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 128 kHz | 5957,1835 Hz | 8,184405% | 9,367808% | 0,709240% | 9,753140% |
| 256 kHz | 5989,1921 Hz | 2,159914% | 2,501091% | 0,180032% | 2,609363% |

Son normas de diferencias matriciales, no porcentajes sumables. Los errores
contra Rayleigh usan su misma norma; el adicional de histogramas usa la norma
de la predicción temporal. Estas cifras pertenecen a la **onda** contraída,
no a la carga completa con relajación del preflight de 606.

El contraste identifica el error temporal como dominante en este banco.
Reducir el desplazamiento de fase/frecuencia mejora claramente la resistencia;
la misma cuadratura exacta no produce el defecto. Aun así 256 kHz conserva
2,609% de error y no aprueba el candidato completo. Tiempo 423 s, pico
405.708.800 bytes (0,378 GiB), sin factor guardado.

Siguiente: evaluar retardo allpass de mayor orden y refinamientos de tasa en
este sondeo, con derivada/histogramas controlados. Exigir estabilidad,
certificado de almacenamiento/pérdida y residuos de masa/resistencia positivos
antes de otra construcción completa. Después repetir Z/R/X, puertos,
semiplano derecho y memoria con los límites originales. No cambiar las
pérdidas de cuerdas ni calibrar muestras para absorber un error del aire.

```powershell
python -m unittest discover -s tools/tests -p test_concert_wave_resistance.py -v
python tools/audit-concert-grand-wave-resistance.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --preflight docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga96-real.json --report NUEVO_DIAGNOSTICO.json
```

## Reproducción

Python/NumPy/SciPy con BLAS de un hilo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_streaming_air.py -v
python -m unittest discover -s tools/tests -p test_concert_wave_mass.py -v
python tools/audit-concert-grand-streaming-air.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --report NUEVO_INFORME.json --rate 128000 --polar 96 --azimuth 256 --fold-y --real-gram --max-array-gib 5
```

El preflight no certifica banda completa ni una nueva escucha. Después
corresponden los controles de banda/semiplano derecho, límites realizados y
memoria transitoria, seguidos del contacto no lineal con estados persistentes.

Actualización posterior: [retardo de segundo orden y derivada cuarta](RESULTADOS-P6-retardo-orden2.md).
El nuevo sondeo recupera precisión; su masa global requiere otro refinamiento.
Este preflight legado conserva su rechazo y sus fuentes en 0c48a73.
