# P6 — Contacto no lineal con tabla y aire recíprocos

2026-10-03. La referencia de estados acústicos anterior quedó en **e244450**,
por pedido del usuario. Esta etapa conecta esa referencia al contacto SI
de una cuerda y un martillo. Es un control físico offline, sin nuevo refit
ni selección de sonido de fábrica.

## Coordenadas, fuerzas y trabajo

Se conservan la cuerda extensible Euler, su huella de contacto distribuida,
el fieltro no lineal y el puente vertical móvil del control P3/P5. El banco
estructural P2 mantiene sus 35 modos hasta 800 Hz. No se ajustan geometría,
rigidez de fieltro, afinación ni coeficientes para mejorar una comparación.

La inercia de la cuerda contiene términos cruzados con la tabla y una masa
de levantamiento del extremo. En las coordenadas normalizadas de ese sistema,
`C` lleva la velocidad mecánica a los modos físicos de la tabla. La masa
del nuevo sistema es `M = I + Cᵀ M_tail C`: la cola acústica modifica la
inercia compartida, en lugar de añadirse a una fuerza alimentada externamente.
No hay columna de martillo en C; su velocidad incidente conserva las unidades SI.

Con `a,p` como posición/velocidad de los canales del aire y `g` como gradiente
del potencial mecánico compartido:

```text
q' = v
M v' = −g(q,v) − D v − Cᵀ Gᵀ p
a' = p
p' = G C v − Ω² a − 2γ p
E = ½ vᵀ M v + V(q) + ½ |p|² + ½ |Ωa|²
E' = −calor_fieltro − vᵀ D v − Σ 2γ p²
```

El mismo desplazamiento de contacto paga el trabajo del fieltro y de la
cuerda. El gradiente AVF integra el potencial cuártico de la cuerda y el
secante exacto del fieltro; el aire y la pérdida material usan punto medio.
La eliminación de los canales del aire deja una resolución mecánica pequeña
por paso. La actualización de momento evita restar dos desplazamientos casi
iguales y dividirlos por un paso muy corto.

Se contabilizan tres presupuestos: global; aire almacenado + absorción
numérica = trabajo de puerto; tabla intrínseca + calor material + trabajo
al aire = trabajo recibido de la cuerda. El puerto incluye tanto fuerza
oscilatoria como fuerza de la masa de cola. Puede devolver energía a la tabla.
La absorción numérica γ sigue siendo un término separado de pérdidas físicas.

## Criterios y alcance

Antes de ejecutar se fijan: balance relativo <1e−9; cambios de energías y
trabajos <0,2%; duración de contacto <0,1%; fuerza/impulso <0,2%; RMS de
posición/velocidad del puente y del banco de tabla <1%. Las trayectorias se
comparan en una grilla común de 128 kHz, sin normalización individual.

Se usa C4/MIDI 60 a velocidad 124, una cuerda y un martillo equivalente,
con 6,4089556 m/s incidentes. El fieltro conserva su ley elástica; una prueba
unitaria separada comprueba también el calor de contacto dependiente de velocidad.
`D=0,046·ω` es una hipótesis material del ensayo, sin identificación de su
partición con la radiación del instrumento real. No se aplica al plugin.

La matriz del aire conserva todos los términos entre modos; 176013 canales
representan 352026 estados. La geometría supone una cara en bafle. La precisión
de transferencia heredada corresponde a 60–800 Hz; el contacto es una fuente
de banda ancha, de modo que no se declara completa su radiación fuera de esa
banda. El corte espectral de 6000 Hz y su masa de cola tampoco identifican
el ataque acústico de un Steinway.

## Pruebas independientes

Las seis pruebas verifican:

1. Masa de cola contra una construcción en coordenadas físicas con inercia
   cruzada de cuerda/tabla y energía cinética positiva.
2. Contacto no lineal contra DOP853 en coordenadas físicas, con masas y
   fuerzas transformadas independientemente; refinamiento temporal de orden dos.
3. Cierre global y por interfaz, con aire inicial no nulo y calor de fieltro.
4. Desacoplamiento del aire contra el integrador de contacto previo.
5. Conservación exacta de estados vivos al rearmar un martillo ya separado;
   la extracción/inyección de su energía se cuenta como trabajo externo.
6. Rechazo de mapeos y pasos inválidos y de rearmado durante compresión.

Todas pasan. La referencia ODE independiente es pequeña y prueba las
ecuaciones; no se confunde con la convergencia de la geometría P2 completa.

## Convergencia del primer contacto

| Refinamiento | RMS velocidad puente | Cambio duración | Mayor cambio energía/trabajo |
| --- | ---: | ---: | ---: |
| T48-384000-Air → T48-768000-Air | 0.01112% | 0.000460% | 0.00585% |
| T72-384000-Air → T72-768000-Air | 0.01112% | 0.000458% | 0.00585% |
| T48-384000-Air → T72-384000-Air | 0.00827% | 0.002022% | 0.00650% |
| T48-768000-Air → T72-768000-Air | 0.00827% | 0.002019% | 0.00650% |

Todos los criterios del primer contacto pasan. La comparación espacial usa
48→72 modos transversales y 96→144 longitudinales. La temporal usa
384→768 kHz a cada resolución. Esto verifica las magnitudes declaradas;
no prueba la fase de cada modo longitudinal alto por separado.

El control sin aire a 768 kHz se compara con el estado DOP853 congelado
del P3/P5 de 48 modos: error RMS final mecánico de posición
0.00439% y de velocidad
0.04511%, excluyendo el martillo.
Ambos pasan el límite de 0,1%; no se exige igualdad bit a bit entre
integradores distintos.

## Presupuesto fino y repetición

Primer golpe, 72/144 modos a 768 kHz, 4 ms:

| Término | μJ |
| --- | ---: |
| Energía inicial incidente | 46933.719044 |
| Cuerda final | 28153.323309 |
| Martillo final | 18608.257842 |
| Tabla final | 139.675355 |
| Aire y cola finales | 9.761996 |
| Calor material | 22.657671 |
| Absorción numérica | 0.042871 |

El fieltro elástico termina sin compresión y no genera calor en este caso.
Duración del primer contacto: **1.816846 ms**;
fuerza máxima 25.555378 N. Hay
210 pasos con trabajo negativo hacia el aire:
el puerto devuelve parte de la energía almacenada.

A los 4 ms se prescribe otra velocidad incidente del martillo, después de
su separación. Todos los estados vivos de cuerda, tabla y aire se conservan
exactamente. La posición del martillo se lleva al primer contacto y su
energía retirada/inyectada se cuenta como **28.325461 mJ**
de trabajo externo. Esto es una condición de entrada para el ensayo, no un
modelo del escape, backcheck o trayectoria de tecla.

Se simulan otros 4 ms con aire inicial
9.761996 μJ y tabla inicial
139.675355 μJ. El segundo contacto dura
2.249334 ms. Presupuestos combinados de los dos golpes,
referidos a la energía inicial del primero: global **8.160e-13**,
aire **4.815e-17**, tabla **4.181e-16**. También cierran por
separado en cada trayectoria. El segundo golpe se verifica a una resolución:
su refinamiento de precisión sigue pendiente.

## Qué cambia respecto del control sin aire

La comparación con y sin carga a 48 modos/768 kHz conserva eventos,
velocidad incidente y parámetros mecánicos. A los 4 ms, la energía de tabla
cambia **-5.66307%**,
el calor material **-4.33803%**
y el trabajo recibido por la tabla
**0.22876%**.
La energía de cuerda cambia apenas
-0.001395%.
Duración, fuerza e impulso del primer contacto son prácticamente iguales.

Como interpretación de este caso: la velocidad transversal sin dispersión
`sqrt(T/μ)` es ≈324 m/s. Con la longitud y el punto de golpe congelados, el
ida/vuelta golpe→puente→golpe es ≈3,36 ms, más que el contacto de ≈1,82 ms.
Esto es compatible con que la carga cambie la tabla después del golpe sin
modificar apreciablemente ese primer contacto. Es una estimación del caso;
Euler dispersivo y otros registros impiden convertirla en una regla universal.
No se presenta esta conexión como una solución demostrada al ataque fino
o al cuerpo que falta frente al Steinway.

## Siguiente paso

Conservar este banco completo como referencia. Reducir estados preservando
pasividad, términos entre modos, transferencia y presupuestos; comprobar
contactos repetidos y horizontes largos contra él. Después extender banda,
registros, unísono, observación SI, acción y pedal para un control audible con
ganancia común. Separar pérdidas materiales de radiación antes de promover
una carga al plugin. P4/Timoshenko y demás pendientes siguen en el plan.

La referencia requiere cientos de miles de estados y tasas de integración
muy por encima de la tasa de audio. Los tiempos guardados son wall time de
diagnóstico con un hilo BLAS por proceso; parte del control seco y de T72
se ejecutó simultáneamente. No constituyen un benchmark de tiempo real.
No se generó un nuevo paquete ni se seleccionó una calibración de fábrica.

## Datos y reproducción

En `C:\SHARED\Fisica-pendiente-2026-10-03\P6-contacto-aire` quedan seis
trayectorias NPZ/CSV (cuatro cargadas, una sin aire y una reiterada), estados
finales completos de cuerda/tabla/aire, métricas, criterios, hashes, log de
pruebas y fuentes con sus dependencias. Los CSV de la reiteración usan tiempo
local desde el segundo golpe; su inicio absoluto es 4 ms.

SHA256 de `source-contact-air.zip`:
`ec2fb150754476a799f18c9925b860aae8026cb69079bbd3d018f61a6f0b0347`.

Paquete 0.172.0 preservado:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

Con NumPy/SciPy y un hilo BLAS:

```powershell
$inputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input'
$asset = 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz'
$bath = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos/bath-fine.npz'
$outputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire'
$reference = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-puente-movil/p2-fine/60-124-P2-low-band-T48.npz'
python -m unittest discover -s tools/tests -p test_concert_bath_contact.py -v
python tools/audit-concert-grand-bath-contact.py --input $inputDir --asset $asset --bath $bath --output $outputDir --transverse 48 --rates 384000 768000
python tools/audit-concert-grand-bath-contact.py --input $inputDir --asset $asset --bath $bath --output $outputDir --transverse 72 --rates 384000 768000 --relaunch
python tools/audit-concert-grand-bath-contact.py --input $inputDir --asset $asset --bath $bath --output $outputDir --transverse 48 --rates 768000 --dry
python tools/audit-concert-grand-bath-contact.py --phase summary --input $inputDir --asset $asset --bath $bath --output $outputDir --dry-reference $reference
```
