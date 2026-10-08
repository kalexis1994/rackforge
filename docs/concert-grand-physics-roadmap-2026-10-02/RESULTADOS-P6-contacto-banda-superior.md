# P6 — Banda de fuente y presión del contacto completo

Fecha: 2026-10-08. Continúa el diagnóstico de banda superior del coast.

## Diseño fijado antes de medir

- Autenticar informe de banda refinado, matriz dirigida y checkpoint de
  procedencia de la cápsula. Misma C4 supuesta, 96 transversales por
  dirección/192 axiales, fieltro, tabla de 606 coordenadas, R y micrófonos.
- Reconstruir **0–3 ms a 1536/3072 kHz**, velocidad incidente **5 m/s**.
  Reproducir medidas físicas del contacto de ambas tasas (<1e−10). El
  extremo fino debe coincidir **exactamente en sus cuatro arrays** con
  la cápsula autenticada. No guardar otra cápsula ni historias en disco.
- Observar V/A reales en rectangular y Hann, sin impulsar los extremos
  ni diferenciar una posición enventanada. Fuente y presión continua
  de celdas en 0–6/6–12/12–24/0–24 kHz. Mantener ceros/fase absoluta.
- Integrales analíticas de Hermite por bloques, con momentos compartidos
  para pasos uniformes y BLAS real. Comparar con observadores originales
  en 0/60/6/8/10/12/16/20/24 kHz (<1e−10). No modificar las fuentes
  congeladas de los productores previos. Cinco pruebas independientes
  pasan antes de ejecutar el contacto, incluida fase, extremos vivos,
  cuadratura, bloque y rechazo de grillas no uniformes (0,115 s).
- Presión con Green continuo, cuadratura 4 para la grilla completa; 4/8
  en siete sondas de transferencia completa (<0,02%), separadas de la
  comparación con el código original (<1e−10). No hay ambiente o EQ.
- Comparar fuente/presión 1536→3072 y captura fina contra decimación
  por dos, ambas ventanas y todas las bandas: cada error <0,1%.
  Grillas de frecuencia 125/62,5/31,25 Hz; comparar finalmente
  62,5→31,25 en todas las normas, cada error <0,1%. Mantener fallos.
- Balance mecánico contra puerto medido <1e−9, impulso <1e−9, calor
  no negativo. No confundir ese balance con un ledger angular del aire.
- Repetir sondas de carga 6/8/10/12/16/20/24 kHz, usando la dirección
  Hann de V del contacto fino; Rayleigh 8/12 (<0,02%) y celdas
  256×192/512×384 (<2%). Conservar umbral direccional 2% y ambas
  columnas de error. Pasar controles no aprueba sondas fallidas.
- Presupuesto conservador de arrays <12 GiB, sin historia angular de
  27 GiB. Estado/contacto en RAM; única R existente, hash de manifiesto,
  tabla, fuentes, padres y cápsula de nuevo al terminar. Global falso.

```text
python tools/audit-concert-grand-contact-band.py --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/contacto-24k.json
```

## Alcance

El extremo guardado no permite recuperar causalmente toda la historia del
ataque. Esta ejecución vuelve a integrarlo, con las mismas ecuaciones y
condiciones; sólo acelera su observación espectral. Los momentos uniformes
son exactos en aritmética exacta; el control con el observador anterior mide
también el efecto de redondear los espaciados temporales nativos.

Normas espectrales de la ventana, con ganancias originales: Hann atenúa
parte del ataque. Fracción de norma de presión en estos micrófonos no es
una medida de radiación total ni de perceptibilidad. Z aplicada al vector
de una sonda no reconstruye la memoria viva ni cambia la fuente. Este
ensayo sólo refina tiempo y captura; no certifica resolución espacial de
cuerda/tabla por encima de 6 kHz, identificación real, unísono o timbre.
No ampliar automáticamente la banda aprobada de carga 60–6000 Hz.

Sin WAV, cambios de fábrica, paquete instalado ni duplicados en SHARED.
El ledger angular del contacto y su presupuesto siguen pendientes.

## Resultados

Ensayo completo [contacto-24k.json](medidas-P6-contacto-banda-superior/contacto-24k.json):
**9/13 controles**, **1711,709 s (28,53 min)**, pico **4,895 GiB**,
estimación conservadora previa **8,911 GiB** (<12). Global falso.
Fallan oracle analítico, convergencia temporal por banda, integración de
frecuencia y refinamiento de celdas. Los fallos se conservan sin cambiar
umbrales, fuerzas, fieltro, geometría, R o condiciones iniciales.

### Reproducción física

Las 13 medidas físicas comparadas de cada tasa reproducen el padre con
**error 0**. Posición, velocidad, onda y relajación al terminar a 3072 kHz
son **exactamente iguales** a los cuatro arrays de la cápsula autenticada.
Hashes de fuentes, padres, tabla, R/manifiesto y cápsula verificados durante
la ejecución y otra vez al terminar. Ninguna historia nueva en disco.

- Balance mecánico máximo **2,42070e−12** (<1e−9), contra puerto medido;
  no certifica almacenamiento/flujo angular del aire.
- Impulso máximo relativo **5,16455e−15** (<1e−9), calor no negativo.
- Contacto fino: fuerza máxima **16,184555963 N**, impulso
  **0,016122755562 N·s**; mismo valor que la referencia dirigida.
- Captura fina contra decimación, V/A/P, ambas ventanas/bandas: máximo
  **2,99401e−9** (<0,1%). La captura converge mucho mejor que el paso
  físico de contacto; refinar sólo la captura no resuelve el fallo temporal.
- Cuadratura de transferencia de presión 4/8, máximo **1,18849e−10**
  (<0,02%); implementación BLAS real contra original **8,79505e−16**.

### Costo del observador analítico y límite de precisión

| Tasa de contacto | Dinámica/captura | V/A, 772 frecuencias, dos ventanas | Oracles, sólo 9 frecuencias |
| --- | --- | --- | --- |
| 1536 kHz | 521,827 s | 2,280 s | 5,253 s |
| 3072 kHz | 1001,169 s | 4,288 s | 10,210 s |

Son trabajos de distinto tamaño, no una comparación directa de costo por
llamada. Los momentos por bloques aceleran la observación del espectro;
la dinámica sigue dominando el ensayo. No se ha acelerado aún el primer
contacto ni adoptado la eliminación lineal del subsistema de tabla/aire.

El oracle falla únicamente en Hann de V a 24 kHz: **4,96601e−10** a
1536 y **4,34068e−10** a 3072, frente a **1e−10**. Las otras sondas,
incluida A, pasan ese límite. A 24 kHz la norma de V Hann fina es
**4,75105e−12**; la discrepancia corresponde aproximadamente a
**2,06229e−21** en su norma absoluta. No se relaja el criterio por su tamaño.
La modulación global resta tres transformadas; cancelación y orden de
suma son una hipótesis numérica a comprobar con pesos locales estables,
no un fallo físico demostrado ni una prueba del error audible.

### Convergencia temporal de contacto

| Observación | V, 0–24 kHz | A, 0–24 kHz | P, 0–24 kHz |
| --- | --- | --- | --- |
| Rectangular, 1536→3072 | 0,00246719% | 0,0139024% | 0,00297064% |
| Hann, 1536→3072 | 0,00327170% | 0,0169017% | 0,00245269% |

Estas normas globales pasan el valor de 0,1%, pero el gate exige **cada
banda** y sigue falso. En Hann 12–24 kHz fallan V **1,73153%**,
A **2,23017%**, P **2,03722%**; también falla 6–12 kHz (V 0,290359%,
A 0,326953%, P 0,433597%). Rectangular pasa todas sus bandas.
No seleccionar una nueva tasa de nota completa ni cambiar automáticamente
los límites porque estas colas aporten poco a la norma total.

### Fuente y presión del tramo completo

Fracción 6–24 kHz respecto a 0–24 kHz, contacto fino, trapecios de 31,25 Hz:

| Observación | V | A | P, dos micrófonos |
| --- | --- | --- | --- |
| Rectangular | 0,605393% | 0,774678% | 0,103420% |
| Hann | 0,000383477% | 0,0293942% | 0,000365104% |

La presión alta de esta C4 tiene poca norma en ambas ventanas; conservar
la sensibilidad de la observación y el fallo de convergencia relativo.
No demuestra irrelevancia de la banda para otras notas, ausencia de modos
altos, similitud perceptiva con Steinway o que el error de carga produzca
la falta de cuerpo. Las cifras Hann atenúan parte del golpe legítimo.

### Acción de carga, nueva dirección del contacto

| Hz | Error Z causal/Rayleigh, mismas celdas | Frente a celdas finas | Cambio de celdas 256×192→512×384 |
| --- | --- | --- | --- |
| 6000 | 0,712873% | 4,43480% | 3,82568% |
| 8000 | 0,873471% | 6,48174% | 5,81416% |
| 10000 | 1,73632% | 6,92359% | 6,31231% |
| 12000 | 2,52027% | 7,19019% | 6,63437% |
| 16000 | 11,2253% | 14,3412% | 9,50396% |
| 20000 | 48,5418% | 47,8653% | 4,65857% |
| 24000 | 72,6085% | 72,4721% | 1,19678% |

Rayleigh 8/12 difiere como máximo **1,77304e−11**: pasa la cuadratura.
El refinamiento de celdas falla **2%**; no basta aumentar el orden de
cuadratura. Sobre la malla vigente Z pasa la sonda de 10 kHz y falla desde
12 kHz, a diferencia del coast; direcciones distintas prueban cosas
distintas. Contra celdas finas también falla la sonda de 6 kHz. La dirección
del contacto cambia hasta **2,28763%** entre tasas; estas sondas no son una
certificación temporal de la dirección ni del operador completo.

La certificación histórica de carga conserva su alcance de operador y
malla originales; no valida estas acciones frente a una malla refinada.
No generar una nueva R o tratar la cápsula antigua como estado válido de
una carga nueva sin autenticar/reconstruir el sistema correspondiente.

### Control de cuadratura diseñado durante la reconstrucción fina

La trayectoria gruesa terminó en **521,827 s** y reproduce exactamente
las medidas físicas del padre. Sus observaciones detectan dos límites
numéricos: la integración Hann de presión en 6–12 kHz cambia **0,483898%**
entre 62,5/31,25 Hz, frente a 0,1%; Hann de V a 24 kHz difiere del oracle
**4,96601e−10**, frente a 1e−10. Son datos parciales de esa tasa; no
equivalen al resultado del contacto fino ni se aprueba todavía el ensayo.

Para el primero se fija una comprobación adicional **sin volver a integrar
el contacto**: en las dos bandas altas, la grilla guardada es uniforme,
por lo que `S_h=(4 T_h−T_2h)/3` reproduce exactamente los pesos de Simpson
compuesto sobre los mismos datos. Comparar 62,5→31,25 Hz de esas normas,
manteniendo 0,1%. Debajo de 6 kHz hay puntos adicionales y se conservan
trapecios; la norma total suma las tres bandas. No extrapolar la identidad
a una grilla irregular, cambiar fuente/ganancia, recortar valores negativos
o esconder el resultado anterior. Tres pruebas pasan (0,003 s), incluida
igualdad con Simpson independiente para una curva oscilatoria y exactitud
para polinomio cúbico. El informe de control heredará todos los otros
controles originales; **no corrige ni aprueba el fallo del oracle**.

```text
python tools/audit-concert-grand-contact-quadrature.py --parent docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/contacto-24k.json --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/cuadratura-contacto-24k.json
```

### Resultado del control adicional

[cuadratura-contacto-24k.json](medidas-P6-contacto-banda-superior/cuadratura-contacto-24k.json)
aplica esa identidad al informe completo autenticado: máximo de integración
**0,0184258%** (<0,1%); Hann de P en 6–12 kHz cambia **0,00275693%**.
El fallo original de trapecios (**0,484016%** máximo) sigue en su informe.
Con este control quedan **10/13**, global falso: oracle, convergencia
temporal y malla siguen fallando. No se han vuelto a medir esos controles.

Normas Hann con Simpson alto/trapecios bajo: fracciones de V/A/P
**0,000383183 / 0,0293766 / 0,000364518%**; cercanas al diagnóstico
original, sin igualar ganancias, eliminar modos o cambiar la fuente.

## Decisión y siguiente paso

Se completó la reconstrucción y medición del ataque; no se aprueba aún
el diagnóstico completo ni se amplía la banda del modelo. La captura y
el espectro global convergen, las pequeñas colas Hann no cumplen el
criterio relativo por banda, y el error de malla es distinto del error
del aproximante. Separar esos tres problemas antes de retocar fieltro.

Siguiente orden:

1. Estabilizar la observación Hann de V con pesos locales y contrastar
   contra el mismo oracle y límite; mantener ambos informes fallidos.
2. Medir contribución **absoluta** del error por banda a la presión/fuente
   total, como diagnóstico adicional para priorizar resolución. No usarlo
   para borrar el criterio relativo incumplido ni prometer perceptibilidad.
3. Refinar la malla de las sondas relevantes y conservar los siete vectores
   complejos pequeños como datos del informe para poder reutilizarlos.
   Los informes actuales guardan normas, no los vectores; una nueva
   medición de esas direcciones exige reconstrucción, no puede inferirse
   de la cápsula final. Evitar guardar la historia completa.
4. Ledger angular del contacto y duración/release, con presupuesto válido.
   La [derivación del puerto inercial](NOTA-P6-observador-ventana-finita.md)
   identifica una vía de reutilización para bancos de contactos; todavía
   necesita implementación y controles contra el integrador completo.

Quedan unísono, identificación real y escucha. Sin WAV ni cambio del
plugin instalado; resultados y diseño sólo en el repo.
