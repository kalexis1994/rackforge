# P6: contacto completo en coordenadas y prefijos temporales

2026-10-06. La masa y transferencia completas, junto con las recurrencias de
dos estados, quedan congeladas en **6253412**, por pedido del usuario.
Esta etapa conecta esa carga con el contacto no lineal conservando los 606
modos de tabla, pérdida material completa y memoria acústica persistente.

Las fuentes del primer auditor y certificado se congelan en **c47e746**.
Esa primera ejecución completa los seis prefijos y los 384 bloques del
certificado, pero su escritura final falla: un criterio contiene un booleano
NumPy que JSON no admite. No se conservan ni se reconstruyen a partir del
balance sus resultados independientes. `prefijos125us.json` queda como
registro incompleto de construcción y contactos, sin aprobación del certificado.
La repetición usa un informe nuevo y la misma R, sin duplicarla.
Sus fuentes quedan congeladas en **d850ef7**. El informe de esa repetición
usa aún las historias completas de inputs y el criterio original del máximo
AVF; las mejoras posteriores del observador/almacenamiento no se atribuyen
retroactivamente a sus medidas.

La protección del proveedor de caché se endurece después, separadamente de
las ecuaciones medidas. Su vinculación sólo certifica los controles de
construcción ya terminados (masa y tres transferencias), no el contacto ni
el certificado que falló al escribir. La repetición convierte los criterios
a booleanos Python y guarda las medidas independientes antes de armar gates.
El reloj del auditor también se mejora después de esa congelación: la
primera corrida usa `GetTickCount64`, con resolución 15,625 ms. La repetición
y el preflight nuevo registran `QueryPerformanceCounter` y su resolución
(0,1 microsegundos en este entorno). No se recalculan los tiempos históricos.
Se añade además el módulo de constantes del aire al manifiesto de fuentes
de futuras corridas; la vinculación de esta caché lo verifica contra c47e746.

## Alcance del banco

Una cuerda C4 supuesta: longitud 0,62 m, masa lineal 0,0081 kg/m y tensión
850 N. Tres modos transversales por dirección y los axiales del núcleo SI.
Martillo de 2 g, velocidad incidente 5 m/s, fieltro `7,5e8`, exponente 2,6,
posición relativa 0,125, huella 0,02 y pérdida dependiente de tasa 0,1.
Son los parámetros del control mecánico anterior, no un refit del Steinway.

Se conserva exactamente el cambio de coordenadas de la proyección y su
pérdida material entre modos. La cuerda recibe movimiento/reacción del puente
durante el contacto; la tabla recibe la carga acústica viva, sin masa de aire
instantánea duplicada. El soporte y el aire usan la misma base modal.

El banco mide **prefijos de 125 microsegundos** a 384, 768 y 1536 kHz:

- `Cold`: contacto desde reposo, con estados acústicos inicialmente nulos.
- `Continued`: prolongación desde el final del prefijo frío más fino. Todas
  las tasas parten del mismo estado completo, conservando cuerda, tabla,
  martillo, ambas coordenadas de onda y relajación. No se relanza el martillo.

Así se separa el error temporal de diferencias en la condición inicial.
Esto no es una nota completa, separación natural del martillo, segunda
pulsación, apagador/release ni comparación audible. Esas etapas quedan abiertas.

## Una única caché numérica

`local/concert-grand-physics/P6-carga-causal-2026-10-06/` conserva sólo las
correlaciones `R.npy` y su manifiesto. El factor angular completo no se guarda.
La identidad incluye SHA del tablero proyectado, informe armónico aprobado,
fuentes físicas, tasa, celdas, cuadratura, derivada y simetría. El archivo R
tiene su SHA; se rechaza un cambio de identidad, dimensiones o contenido.
No se repara ni se sobrescribe automáticamente una caché existente.

La protección endurecida verifica además los bytes de las formas de
superficie realizadas (incluyen la base modal), posiciones, direcciones y
pesos angulares, así como dimensiones, densidad y velocidad del aire. Un
mismo archivo de tablero no basta si otro entorno genera una base diferente.
Cambiar el signo de una coordenada modal se rechaza aun con la identidad de
archivos intacta. La caché inicial de esta corrida se vincula explícitamente
después de sus controles de construcción medidos; R no se modifica ni se duplica.

La recarga usa una vista de sólo lectura y conserva todos los taps/modos.
La construcción usa bloques de 32 direcciones frente a 8 en la corrida
anterior. Cambia la agrupación de sumas, no las direcciones ni el operador.
Antes de usarla se exige reproducir la masa y las medidas Z/R/X de 60,
1881,960712 y 6000 Hz del informe aprobado a < 1e−10.

Esta es una caché nueva y única, necesaria para evitar reconstruir la misma
carga en cada ensayo. No duplica otra R almacenada, los informes ni archivos
de `SHARED`. Su tamaño medido es 1.909.627.328 bytes (~1,91 GB); los historiales del certificado
se mantienen en RAM y se liberan al terminar.

## Certificado independiente en una sola pasada

`certificate_batch` regenera cada bloque angular una vez y calcula:

```text
E_onda = 144 * E_base(x1) + 12 * E_base(x2)
E_base(x) = sum(norm(colas_futuras(H, x))²) / (2*rate)
P_radiada = norm(sum(H_j * u_j))² / 2
```

Los dos términos de energía, todos los estados iniciales/finales y todos
los inputs centrales se evalúan con el mismo factor positivo. No se infiere
la radiación como diferencia entre trabajo y almacenamiento. Se agrupan las
observaciones; no se truncan modos, direcciones ni colas. La matriz de
observación se prepara una vez por bloque y se reutiliza en los lotes.

Dos pruebas nuevas pasan: energía/radiación/trabajo contra el factor completo
almacenado y su certificado por convolución; caché contra transferencia y
midpoint con memoria viva, con rechazo de identidad alterada/corrupción.
La igualdad por recarga es exacta en esos controles pequeños; el certificado
por bloques exige error < 1e−11 y conserva radiación no negativa.

Una tercera prueba cubre la vinculación explícita del proveedor: una caché
sin esa prueba se rechaza al recargar; exige controles de construcción
terminados del mismo R y fuentes físicas. No requiere aprobar los contactos
posteriores y no se migra automáticamente durante la lectura.

El banco completo acumula por separado pérdida de fieltro, material,
relajación y ondas, y trabajo del puerto. Mide almacenamiento inicial/final
de forma independiente. Exige error relativo < 1e−9 en balance total y
acústico, reacción < 1e−9 N y residuo de potencia < 1e−9 W.

El refinamiento compara la misma grilla de captura y las mismas condiciones
iniciales. El par más fino exige < 1% en trayectoria mecánica/estado final de
tabla y < 0,2% en impulso/fuerza máxima. La razón de errores contra la
referencia fina debe ser < 0,35 en las dos fases. No se denomina referencia
exacta a esa última tasa: es un control de refinamiento de paso finito.

Un informe derivado separado comprueba además el refinamiento del
almacenamiento de onda/relajación, radiación, calor material/de fieltro y
trabajo acústico. Usa el mismo límite de 0,2% y denominador mínimo de
1e−14 J del banco de contactos anterior, establecido antes de terminar
esta medida. El cierre de un balance discreto por sí solo no prueba la
precisión temporal de sus términos. Este control tampoco certifica todas
las coordenadas de onda ni la presión temporal de los micrófonos.

## Presupuesto de esta corrida

El auditor declara por separado los arrays de construcción y certificado:
4.894.085.088 y 5.848.089.920 bytes, respectivamente, bajo 8 GiB. Los inputs
de todas las secciones ocupan 2.117.606.400 bytes. El working set real se mide
aparte porque el presupuesto de arrays no limita al intérprete/BLAS/SciPy.

`all_gates_passed` permanece falso incluso si pasan los controles del prefijo:
no aprueban la duración completa del contacto, repetición natural, release,
coste de audio, propiedades físicas identificadas ni timbre.

## Medidas independientes completadas

La [repetición completa](medidas-P6-contacto-orden2/prefijos125us-repetido.json)
termina y guarda las medidas independientes. Error relativo máximo de balance
total **3,251251e−13**; balance acústico **3,096184e−23**, ambos normalizados
por la energía inicial total del caso. Radiación/calores no negativos;
residual de reacción máximo **3,176174e−14 N**, potencia **1,123589e−18 W**.

| Refinamiento, 768 vs 1536 kHz | Cold | Continued |
| --- | ---: | ---: |
| Trayectoria mecánica | 0,00000589893% | 0,000100693% |
| Estado final de tabla | 0,00726612% | 0,00189815% |
| Impulso | 0,000361345% | 0,000608813% |
| Razón de errores mecánicos | 0,2000048 | 0,2000099 |
| Mayor cambio de almacenamiento/trabajo/calor | 0,0142672% | 0,00683072% |

El [refinamiento de términos y cruce de controles](medidas-P6-contacto-orden2/refinamiento-trabajo.json)
pasa en ambas fases. Los contactos de la captura compacta y los controles
de fuerza reproducen exactamente los diagnósticos de esta repetición;
sus curvas de fuerza común también coinciden exactamente. Pasan los **doce
controles enlazados del prefijo**, usando la fuerza en instantes compartidos.
El informe original mantiene su fallo de máximo AVF frío: no se altera.

En estos 125+125 microsegundos, `wave_radiation_j` está entre ~3e−30 y
~2e−26 J, muy por debajo del denominador mínimo de 1e−14 J. También la
pérdida de relajación fría es menor que ese mínimo. Sus gates son controles
absolutos frente a ese piso, **no evidencia de precisión relativa de una
cola radiada no nula**. Se validan almacenamiento y trabajo de estos prefijos;
faltan llegadas acústicas y duración completa. El banco pequeño sí prueba
el certificado con memorias arbitrarias y radiación no nula.

Corrida repetida: **1844,820 s (30 min 45 s)**; certificado **1706,065 s**;
pico propio **6,588562 GiB**. La estimación de arrays, 5.848.089.920 bytes,
no es el límite real del proceso: temporales/bibliotecas elevan ese pico.
Las medianas de paso medidas son 0,149–0,162 s, con el control de fuerza
concurrente durante los contactos; no son un benchmark aislado ni coste
de audio en tiempo real. El primer reloj cuantizado daba ~0,125 s/paso.

`linked_prefix_gates_passed=true`, **`all_gates_passed=false`**. Ninguna
de estas medidas aprueba la nota completa, el timbre o el plugin de fábrica.

## Fuerza observada en tiempos comunes

El máximo de `force_n` de AVF es una fuerza promediada sobre un intervalo.
En estos prefijos la fuerza sigue creciendo al final; los intervalos de
distinta duración no leen el mismo instante. Se conserva esa medida y su
límite original. Un control separado repite las mismas ecuaciones y observa
la ley instantánea del fieltro en la grilla común de 384 kHz, incluyendo
ambos extremos. No cambia fuerzas aplicadas, impulso, contacto ni memoria.

En [fuerza-tiempo-comun.json](medidas-P6-contacto-orden2/fuerza-tiempo-comun.json),
se reproducen **exactamente** impulso, máximo AVF, energía mecánica final,
compresión, norma de onda y energía de relajación de los seis prefijos.
Pasan sus tres gates con los mismos límites, < 0,2% para curva/máximo y
< 0,35 para la razón de refinamiento:

| Medida, 768 vs 1536 kHz | Cold | Continued |
| --- | ---: | ---: |
| Cambio del máximo AVF original | 0,635430% | 0,151107% |
| Cambio del máximo en tiempos comunes | 0,000242279% | 0,000120979% |
| Cambio de la curva en tiempos comunes | 0,000205656% | 0,000173473% |
| Razón de errores de esa curva | 0,2000108 | 0,2000029 |

El pico común más fino es 5,515813 N frío y 21,791650 N continuado. Son
máximos de esa grilla y esos prefijos, no del golpe completo. El fallo del
máximo AVF frío se conserva; se localiza en la observación sobre intervalos
distintos, no en una modificación de la ecuación del fieltro. Las futuras
corridas del auditor añaden esta curva común y conservan el máximo AVF
original como diagnóstico. El certificado energético sigue siendo independiente.

## Certificado compacto preparado para la duración completa

`certificate_ports` guarda cada velocidad central del puerto y los estados
acústicos completos de los extremos. Para cada bloque angular reconstruye
los inputs de todas las secciones con la misma recurrencia midpoint, en
lotes limitados. Después evalúa directamente el campo y su norma positiva.
No obtiene la radiación como diferencia de energías ni elimina coordenadas.
Exige que la reconstrucción coincida con el estado final registrado a
< 1e−11. La memoria inicial no se pone a cero.

Dos pruebas nuevas pasan contra un factor completo almacenado, fuerza/inputs
de la recurrencia escalar y su certificado por convolución. Cubren memoria
fría y viva a dos tasas, almacenamiento/radiación/trabajo, dos tamaños de
lote y rechazo de un extremo incompatible. Esto valida el banco pequeño,
**no el certificado compacto completo de 606** ni un ahorro de tiempo medido.

En los seis prefijos presentes, las historias centrales ocuparían
3.257.856 bytes frente a 2.117.606.400 bytes de inputs completos (650 veces
menos). Un lote de 16 añade 50.419.200 bytes de trabajo; estados, correlaciones
y factores permanecen. El presupuesto de arrays del certificado sería
3.784.160.576 bytes, separado del working set real. Se habilita como opción
`--compact-certificate` para la siguiente medida; no sustituye esta repetición.

El [preflight de captura/reconstrucción real](medidas-P6-contacto-orden2/captura-puertos.json)
sí completa los seis contactos con las 606 coordenadas: la reconstrucción
desde puertos da **error cero** en los seis estados acústicos finales.
Historias capturadas: 3.257.856 bytes; pico propio 2.698925 GiB y 146,390 s.
Se ejecuta junto a la repetición del certificado anterior, con BLAS de un
hilo por proceso: no es un benchmark aislado ni una medida de velocidad de
render. Se detiene antes de almacenamiento/radiación independiente, mantiene
`all_gates_passed=false` y no guarda estados/historias en disco.

## Siguiente etapa

1. Certificado compacto independiente con las 606 coordenadas y duración
   suficiente para separación del martillo y radiación no nula; medir su
   coste real y precisión, no sólo el cierre del balance.
2. Segunda pulsación natural desde el sostenido, sin resetear tabla/aire;
   apagador y release con sus reacciones y pérdidas.
3. Presión temporal en ambos micrófonos usando la misma base física y
   controles de ancho de banda/paso antes de otra escucha musical.
4. Identificar propiedades reales y completar unísono, caras/borde y los
   mecanismos restantes del plan. La tabla y cuerda de este banco son supuestas.

## Reproducción

Python/NumPy/SciPy, BLAS de un hilo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_pade_certificate.py -v
python -m unittest discover -s tools/tests -p test_concert_pade_port_certificate.py -v
python tools/audit-concert-grand-pade-contact-full.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVO_INFORME.json
python tools/audit-concert-grand-pade-contact-full.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --compact-certificate --replay-preflight-only --report NUEVA_CAPTURA.json
python tools/audit-concert-grand-prefix-work-refinement.py --prefix-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-orden2/prefijos125us-repetido.json --force-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-orden2/fuerza-tiempo-comun.json --capture-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-orden2/captura-puertos.json --report NUEVO_REFINAMIENTO.json
```

Se requiere una ruta libre para cada nuevo informe. La caché compatible se
reutiliza; su contenido se autentica. Los resultados permanecen en el repo,
sin WAVs ni paquete nuevo en `C:\SHARED`.
