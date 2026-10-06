# P6: retardo de segundo orden y derivada de cuarto orden

2026-10-06. El Gram real y el diagnóstico del error temporal quedan en
**0c48a73**, por pedido del usuario. Esta etapa mantiene la geometría P2,
las pérdidas completas y la base de 606 coordenadas; ensaya una aproximación
temporal diferente, sin identificación de muestras ni ajuste de ganancia.

## Retardo y pasividad

Para `p=s/rate`, se reemplaza la sección de Cayley por:

```text
T(s) = (p² - 6p + 12) / (p² + 6p + 12)
Z_onda(s) = R0/2 + sum(Rk * T(s)^k, k >= 1)
```

Es el Padé [2/2] del retardo `exp(-s/rate)`. La documentación de
[MathWorks](https://www.mathworks.com/help/control/ug/time-delay-approximation.html)
describe el uso de Padé para retardos continuos y advierte que la aproximación
de frecuencia no garantiza por sí sola el transitorio. Aquí se verifica además
la realización con estados y su trabajo, sin discretizar los coeficientes como
si fueran un filtro digital a la tasa del driver.

Los polos son `rate*(-3 ± i*sqrt(3))`, estrictamente estables. En el eje
imaginario `|T|=1`. Para `Re(p)>0`:

```text
|p²+6p+12|² - |p²-6p+12|² = 24 Re(p) (|p|²+12) > 0
```

Por tanto T entra en el disco unidad y el polinomio positivo real del factor
continúa siendo positivo real. Esta prueba algebraica no prueba precisión
respecto de la geometría ni de un piano real.

Se mantiene `T'(0)=-1/rate`: la masa DC de una correlación dada es la misma
que con Cayley. En cambio `T(infinito)=+1`; el límite de la onda debe leerse
en **DC del polinomio**, no con la suma alternada que usaba Cayley. Se implementa
un completado específico con ese límite y los mismos residuos estrictamente
positivos; no se recortan autovalores.

## Estado vivo y certificado de energía

Cada sección usa dos coordenadas por entrada modal:

```text
x' = rate * [[0, 1], [-12, -6]] x + rate * [0, 1] u
y = u - 12*x2
P = diag(144, 12) / rate
d(x.T P x / 2)/dt = (u² - y²)/2
```

Al aplicar la identidad bilineal a todas las secciones, el Gram positivo
de las colas futuras del FIR se levanta con P. Si `E_base` es el certificado
de Cayley que ya calcula esas colas:

```text
E_orden2 = 144 * E_base(x[:,0]) + 12 * E_base(x[:,1])
```

No se infiere energía de la diferencia entre trabajo y pérdidas. La fuerza
se evalúa con las correlaciones y salidas de la cascada; la radiación con
el mismo factor FIR. Midpoint conserva la identidad cuadrática para cualquier
paso positivo dentro del rango numérico. El soporte de tabla conserva la
misma eliminación Schur `Z(2/dt)` y las pérdidas materiales entre modos.

## Derivada de cuarto orden

La diferencia/promedio anteriores tienen magnitud `rate*sin(theta)`. Se
convoluciona su factor con el filtro simétrico `[-1/6, 4/3, -1/6]`:

```text
[1/2, 0, -1/2] * [-1/6, 4/3, -1/6]
    = [-1/12, 2/3, 0, -2/3, 1/12]
```

La nueva magnitud es `rate*(4/3*sin(theta) - sin(2*theta)/6)`. Conserva cero
DC, cero Nyquist y parejas por inversión exacta, y añade dos taps. Los
coeficientes negativos no son resistencias negativas: se modifica el factor
de ondas y se construye su Gram positivo. La masa realizada sí puede cambiar;
su residuo respecto del continuo deberá volver a medirse.

La prueba de masa verifica también la identidad exacta del cambio, en la
correlación original recíproca:

```text
M_cuarto - M_original = (7*R0 - R1) / (18*rate)
```

Es positivo semidefinido: el filtro tiene magnitud ≥ 1 en todo el círculo
unidad. Por eso el residuo respecto de la masa geométrica disminuye; aprobar
el sondeo no permite dar por aprobado el residuo del espacio modal completo.

La cuadratura angular, integración de histogramas y formas de celda permanecen
iguales. No se corrigen frecuencias individuales ni se usan ganancias aprendidas.

## Pruebas

Pasan cinco pruebas nuevas:

- Transferencia del factor y pasividad en eje/semiplano derecho, estabilidad,
  fase y masa por extrapolación de puntos finitos.
- Estados arbitrarios vivos a pasos 1/16 kHz, 1/192 kHz y 3 ms: balance relativo
  < 1e−11, radiación no negativa, Schur/deferred equivalentes.
- Fuerza de arranque contra una ODE independiente construida por polinomios
  Padé y `tf2ss`; refinamiento midpoint de segundo orden.
- Factor de cuarto orden, equivalencia de construcción completa/por bloques
  y certificado vivo con ese factor.
- Completado de masa/límite, rechazo de residuo activo, balance de tabla
  acoplada con pérdida material no diagonal y aire inicialmente activo.

Son bancos sintéticos pequeños. No sustituyen masa matricial/transferencia
de las 606 coordenadas ni los refinamientos del contacto real.

## Sondeo real: separación de las dos correcciones

[Informe](medidas-P6-aire-bloques/pade2-cuarto.json). Se reconstruye el mismo
banco de 30 direcciones: once estados cargados complejos a 6 kHz y ocho sondas
aleatorias. Error de representar esos estados: 1,614e−15. Diferencia contra
el error resistivo legado congelado: 1,310e−14; se exige < 1e−10.

Con tasa 128 kHz, superficie 256×192 y cuadratura 96×256:

| Retardo | Derivada | Error de resistencia de la onda a 6 kHz |
| --- | --- | ---: |
| Cayley | Original | 9,753140% |
| Padé [2/2] | Original | 3,561562% |
| Cayley | Cuarto orden | 8,431437% |
| Padé [2/2] | Cuarto orden | **0,773470%** |

Ninguna corrección aislada basta bajo 2%; juntas recuperan precisión en ese
sondeo sin aumentar la tasa ni refinar direcciones. La matriz completa de
606 conserva sus puertas y deberá volver a construirse/medirse después de
la condición de masa.

El residuo de masa del sondeo es ya estrictamente positivo: mínimo
5,16090e−5, máximo 0,0003725078. El completado conserva ambos límites;
errores realizados de masa 3,909e−16 y resistencia límite 3,120e−15.

| Hz | Error complejo Z | Error R | Error X |
| ---: | ---: | ---: | ---: |
| 60 | 0,006715% | 0,074221% | 0,000087% |
| 1881,960712 | 0,208602% | 0,649707% | 0,086084% |
| 6000 | **0,706484%** | **0,354651%** | **0,925651%** |

Tres puntos adicionales del semiplano derecho pasan el mismo límite de 2%:
100+4000i, 1000+20000i y 5000+40000i s⁻¹; errores complejos 0,073508%,
0,287914% y 0,603487%. Todos los autovalores disipativos mínimos relativos
son positivos en los seis puntos. Los cuatro gates del sondeo pasan.

Se usan 38940 estados de onda, frente a 19470 de la misma derivada de cuarto
orden con Cayley. Los polos de sección son −384000 ± 221702,503i s⁻¹.
El Gram/correlación sigue siendo de taps×30×30; no se duplica por tener
dos estados por sección. Corrida 143,312 s; working set máximo 553.533.440
bytes (0,516 GiB). No es un benchmark de audio de fábrica.

Este resultado valida **esa contracción en esos puntos**. No mide el nuevo
contacto de las 606 coordenadas ni aprueba timbre. La tasa del factor tampoco
es la del dispositivo ASIO/Web Audio o la del paso del integrador; este
último requiere su propio refinamiento.

## Condición de masa en las 606 coordenadas

[Informe](medidas-P6-aire-bloques/masa-cuarto96.json). Se reproduce primero
el control legado 32×64/128 kHz: mínimo −0,0044861946 y máximo 0,0057726914,
con diferencia de extremos propios 1,983e−14 frente a sus correlaciones
congeladas. Ese control tarda 34,469 s y conserva su rechazo anterior.

La derivada de cuarto orden con 96×256/128 kHz da:

- Residuo mínimo **−2,75934e−6**, máximo **0,0007889262**.
- **Condición rechazada**, con el mismo margen estricto 1e−13. El defecto
  negativo está muy por encima del redondeo; no se recorta ni se reduce ganancia.
- 469,062 s para la masa; 509,906 s total con control/preparación.
- Working set máximo 925.659.136 bytes (0,862 GiB). Se evita reservar
  ~5,79 GB de Gram/correlaciones; no se guarda un factor grande.

La derivada original en esa cuadratura tenía residuo mínimo positivo
1,283e−5. El filtro nuevo cambia la masa de la onda, tal como predice la
identidad anterior. El sondeo de 30 conserva residuo positivo y precisión;
el espacio completo contiene direcciones que el sondeo no certificaba.
Por ello no se construye ni se promueve el nuevo completado de 606 a escucha.

Esto **no identifica todavía la causa única** del residuo negativo como
cuadratura angular: también hay discretización de celdas/histogramas. El
siguiente control mantiene tasa/celdas/operador y refina la cuadratura angular
(128×384 como primer ensayo), repitiendo la condición completa. Separará
ese error del cambio temporal antes de volver a construir las correlaciones.

## Pendientes de integración

1. Resolver el residuo de masa de la derivada nueva en las 606 coordenadas:
   comenzar por refinamiento angular separado de tasa/celdas/operador.
2. Construcción/transferencia completa: Z/R/X, once puertos, controles de
   banda y semiplano derecho, límites y refinamientos transitorios.
3. Contacto AVF frío y repetido, con las dos coordenadas acústicas persistentes.
   `BoundedContact` usa la interfaz genérica de aire; sus versiones rápidas
   `FastBoundedContact` y `ExponentialCoast` contienen todavía recurrencias
   específicas de Cayley/una coordenada. Requieren adaptar y comparar esas
   recurrencias antes de renderizar; no se les pasa este estado como compatible.
4. Evaluación conjunta de los dos términos de almacenamiento por bloques:
   hoy la referencia regenera el factor en cada término. Debe conservar el
   mismo certificado cuando se reduzca ese coste.
5. Contacto/release y contexto musical con la banda estructural ampliada,
   antes de identificación de pérdidas/unísono/geometría real y nueva escucha.

Los coeficientes de correlación del sondeo no se guardan como otra copia
grande. Los informes quedan sólo en el repo; no hay WAV ni paquete nuevo.

## Reproducción

Python/NumPy/SciPy con BLAS de un hilo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_pade_air.py -v
python tools/audit-concert-grand-pade-air.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --diagnostic docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/resistencia-tasas.json --report NUEVO_INFORME.json
python tools/audit-concert-grand-wave-mass.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --comparison-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/refinamiento32.json --case 128000 32 64 --case 128000 96 256 --fourth-case 128000 96 256 --fold-y --report NUEVA_MASA.json
```

El banco reproduce la contracción de las once respuestas cargadas a 6 kHz
más ocho sondas aleatorias, y contrasta las dos correcciones por separado.
El candidato completo conserva las 606 coordenadas. No se promueve a audio
por aprobar una contracción de 30.
