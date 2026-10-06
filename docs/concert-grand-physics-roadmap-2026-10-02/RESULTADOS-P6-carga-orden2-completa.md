# P6: condición global y contacto con retardo de segundo orden

2026-10-06. La corrección temporal, sondeo de 30 coordenadas y rechazo de
masa a 96×256 quedan congelados en **e11fe6d**, por pedido del usuario.
Esta etapa conserva la tabla P2 supuesta, 606 coordenadas, pérdidas completas,
superficie 256×192, tasa del factor 128 kHz y operador de cuarto orden.

## Separación del refinamiento angular

Se repite el control legado 32×64 y luego se refina sólo la cuadratura a
128×384. La condición exige que la masa continua menos la masa de ondas sea
estrictamente positiva en todas las coordenadas, con el mismo margen relativo
1e−13. No se recortan autovalores ni se ajusta una ganancia.

El [informe de masa](medidas-P6-aire-bloques/masa-cuarto128.json) registra:

| Cuadratura | Mínimo del residuo | Máximo del residuo | Condición |
| --- | ---: | ---: | --- |
| 96×256 (etapa anterior) | −2,7593417e−6 | 0,0007889262 | Rechazada |
| 128×384 | **+9,9769870e−6** | **0,0006321346** | **Aprobada** |

El control legado 32×64 reproduce sus extremos propios frente a las
correlaciones congeladas con error relativo 1,98333e−14. Refina únicamente
la cuadratura angular; el cambio basta para pasar esta condición global del
operador temporal nuevo. No certifica convergencia angular para toda la banda.

961,313 s para la masa refinada y 1006,234 s totales con control/preparación.
Working set máximo 927.993.856 bytes (0,864 GiB). Hay 49152 canales lógicos,
24576 evaluados por la simetría y 1536 bloques; se evita reservar ~5,79 GB
de Gram/correlaciones durante esta condición previa.

El auditor de transferencia requiere una fila aprobada de ese informe y
verifica sus fuentes, padres y datos antes de reservar correlaciones.

## Recurrencia rápida de dos estados

`PadeMidpoint` conserva los estados vivos `x1,x2` y evalúa las mismas ecuaciones
de midpoint que `PadeAir`. Para `h=dt*rate/2`:

```text
d = 1 + 6h + 12h²
t = (1 - 6h + 12h²) / d
b_j = (144h*x1_j - 12*x2_j) / d
u_(j+1) = t*u_j + b_j
```

Los offsets se calculan como una recurrencia vectorizada sobre las secciones
acústicas. Este eje **no es el tiempo del audio**. Las potencias de t propagan
la velocidad de entrada; las soluciones centrales de las dos coordenadas
actualizan el estado persistente. No se restablecen colas al golpear otra vez.

La fuerza constante contrae las correlaciones directamente, sin crear una
segunda copia de la matriz de tamaño `modos×(secciones*modos)`. Esto evita esa
copia; todavía se lee el tensor completo al evaluar la fuerza. No se afirma
coste de tiempo real ni aceleración medida de la tabla completa.

Se adaptan `FastBoundedContact` y la fase acústica de `ExponentialCoast`.
El contacto AVF, potencial no lineal, pérdida de fieltro, pérdida material,
eliminación Schur y separación mecánica/acústica siguen las mismas ecuaciones.
La ruta previa de Cayley conserva su recurrencia específica.

## Verificación temporal pequeña

Pasan tres pruebas nuevas:

- Cascada activa contra una resolución matricial independiente de la ODE
  continua discretizada con midpoint, a pasos `0,1/rate`, `1/rate` y `10/rate`.
  Estados, salidas y fuerza concuerdan dentro de 2e−11 relativo / 2e−12 absoluto
  para estados y 2e−11 absoluto para fuerza. Balance de trabajo < 2e−11;
  radiación no negativa.
- Contacto no lineal con las dos coordenadas acústicas activas, a 128, 384 y
  768 kHz de integración: 96 pasos por tasa contra el contacto genérico,
  error de estado < 2e−9 y balance independiente < 1e−9. Relanzamiento
  prescrito del martillo conserva ambos estados de onda y la relajación;
  trabajo externo contabilizado. Radiación diferida concuerda con el factor.
- Sostenido separado del martillo: 96 pasos a 32 kHz, balance total y del
  puerto acústico < 1e−9. Comparación con DOP853 durante 0,5 ms: errores
  decrecientes de segundo orden entre 64/128/256 kHz y error final < 0,1%.

Pasan además las nueve pruebas previas de contacto rápido y coast con Cayley.
Estos límites son los criterios de las pruebas, no una medida de timbre.
Los ensayos usan un sistema pequeño y factores sintéticos pasivos. Falta
validar contacto/repetición/refinamiento con la carga completa, release,
observadores y contexto musical; no constituyen identificación del Steinway.

## Auditor de la tabla completa

`audit-concert-grand-pade-full.py` usa el Gram real por bloques y el límite
correcto de Padé, `T(infinito)=+1`. Comprueba la masa contra la suma acumulada
independiente de factores, y masa/límite realizados después del completado.

Primero mide 60, 1881,960712 y 6000 Hz. Exige error Z/R/X < 2%, movilidad
y presión por puerto < 1%, y disipación relativa ≥ −1e−10. Si esas puertas
pasan, recorre las 83 frecuencias congeladas del padre y los tres puntos
`100+4000i`, `1000+20000i`, `5000+40000i` s⁻¹ con error complejo < 2%.

Una única copia del informe se actualiza al terminar cada punto. Los factores
y correlaciones se mantienen en memoria; no se guarda otro activo grande.
La puerta general permanece falsa: aunque la grilla armónica pase, no habrá
aprobación temporal, musical ni de ejecución del plugin.

### Controles completos iniciales

El [informe de carga](medidas-P6-aire-bloques/carga128-pade2.json) reproduce
la condición de masa por correlaciones: diferencia de extremos respecto de
la suma de factores **7,61524e−14**. El completado de las 606 coordenadas
pasa sin reparación. Masa realizada: error 2,97115e−16; límite de resistencia:
5,54126e−15. Tiempos de relajación: 1,81599–5,35655 microsegundos.

| Hz | Error Z | Error R | Error X | Movilidad máxima | Presión máxima |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 60 | 0,003328% | 0,013101% | 0,000078% | 0,000135% | 0,000136% |
| 1881,960712 | 0,128320% | 0,177351% | 0,079975% | 0,006993% | 0,029929% |
| 6000 | **0,667445%** | **0,494951%** | **0,921198%** | **0,019284%** | **0,055024%** |

Pasan los seis gates del control de tres frecuencias. Frente al candidato
legado con Cayley/derivada original y cuadratura 96×256, el error a 6 kHz
era Z 10,012998%, R 8,618405%, X 12,349082% y presión máxima 1,058836%.
La comparación incluye ambas correcciones temporales y el refinamiento
angular; no atribuye toda la mejora a uno solo. La ablación del sondeo
anterior separa las dos correcciones temporales.

Se conservan **786588 estados de onda y 606 de relajación**, 787194 en total.
Son estados de este prototipo completo; no una configuración de ejecución
aceptada para el plugin.

### Grilla congelada y semiplano derecho

Se completan las **83 frecuencias**, entre 60 y 6000 Hz, con los once puertos
y ambos micrófonos. Se mantienen las frecuencias próximas a resonancias y
los puntos de excitación reservados, sin ajustar ganancias ni pérdidas.

| Magnitud | Error máximo | Frecuencia / punto |
| --- | ---: | --- |
| Impedancia compleja Z | **0,667445%** | 6000 Hz |
| Resistencia R | **0,494951%** | 6000 Hz |
| Reactancia X | **0,921198%** | 6000 Hz |
| Movilidad por puerto | **0,020731%** | 3470,730353 Hz, nota MIDI 36 |
| Presión por puerto | **0,123714%** | 4909,246600 Hz, nota MIDI 96 |

| Punto Laplace (s⁻¹) | Error complejo |
| --- | ---: |
| 100+4000i | 0,040734% |
| 1000+20000i | 0,259792% |
| 5000+40000i | 0,682496% |

El mínimo autovalor disipativo relativo es +3,47661e−8 en los 86 puntos.
Pasan los **nueve gates armónicos/Laplace**: completado finito, equivalencia
de masa, límites, Z/R/X, puertos cargados, pasividad muestreada, grilla
completa, precisión Laplace y pasividad Laplace. Las pruebas algebraicas del
factor/allpass dan el certificado de pasividad, separado de esta precisión.

1654,188 s (27 min 34 s), con working set máximo 4.338 GiB. Se conserva
únicamente el informe de medidas; no un segundo activo grande. Este tiempo
incluye construcción de correlaciones y todas las comparaciones; no es el
coste de renderizar una nota ni un benchmark del driver.

`harmonic_gates_passed=true`, **`all_gates_passed=false`**. La precisión en
esta grilla no certifica toda frecuencia intermedia, el transitorio completo,
el coste de ejecución o el timbre de un piano real. La geometría y pérdidas
P2 siguen siendo supuestas, no identificadas a partir del Steinway.

## Siguiente etapa

1. Contacto frío y repetido con los 606 modos, las pérdidas completas y los
   dos estados acústicos persistentes; reacción, energía y refinamiento de paso.
2. Reducir el coste del certificado por bloques conservando exactamente el
   mismo almacenamiento/radiación; medir coste del contacto y coast completos.
3. Release con apagador y observación de presión en el tiempo antes de nueva
   frase musical. El observer de presión debe usar la misma base física.
4. Después de esa validación, escucha e identificación de propiedades reales,
   unísono y mecanismos restantes del plan. Esta etapa no selecciona un timbre.

## Reproducción

Python/NumPy/SciPy, BLAS de un hilo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_pade_midpoint.py -v
python -m unittest discover -s tools/tests -p test_concert_pade_contact.py -v
python -m unittest discover -s tools/tests -p test_concert_fast_contact.py -v
python -m unittest discover -s tools/tests -p test_concert_exponential_coast.py -v
python tools/audit-concert-grand-wave-mass.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --comparison-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/refinamiento32.json --case 128000 32 64 --case 128000 128 384 --fourth-case 128000 128 384 --fold-y --report NUEVA_MASA.json
python tools/audit-concert-grand-pade-full.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --mass-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/masa-cuarto128.json --report NUEVA_CARGA.json
```

Los informes nuevos requieren rutas libres. Se conserva el paquete de fábrica
0.172.0; no se generan WAVs ni copias de informes en `C:\SHARED`.
