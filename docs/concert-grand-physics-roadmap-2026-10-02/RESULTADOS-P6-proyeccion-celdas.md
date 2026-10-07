# P6 — Proyección de observación antes del histograma angular

Fecha: 2026-10-07. Implementación **bc6a362**. Continúa el
[certificado de coast y presión](RESULTADOS-P6-coast-campo.md).
No cambia la cuerda, fieltro, tabla, pérdidas, fuerzas ni carga acústica.

## Operación y controles

El factor de radiación tiene la forma `H = L(S)`, donde `S` contiene las
formas físicas en cada celda y `L` integra su área retardada por dirección,
aplica pesos de cuadratura y el mismo diferenciador temporal. Es lineal:
`L(S) Q = L(S Q)`. La ruta anterior construye cada H completo y multiplica
por Q. La alternativa proyecta las formas una vez y construye el factor
de observación con menos columnas.

Se conserva la construcción completa de H en todos los bloques para
comparar su norma con R[0]. También se comprueba la norma proyectada contra
`trace(Q.T R[0] Q)`. No se eliminan celdas, direcciones, canales opuestos,
pesos ni taps. La geometría se comparte mediante una vista privada; las
formas originales y la identidad autenticada de R permanecen intactas.
La proyección sólo participa en la observación de almacenamiento/flujo.

La ruta anterior sigue siendo la opción por defecto. El nuevo auditor usa
explícitamente `factor_projection='cells'`, conservando los umbrales de
balance, refinamiento y precisión. Se mantienen los residuos de todas las
memorias/inputs y sus intervalos analíticos con salvaguardas numéricas.
No son una prueba formal de redondeo por aritmética de intervalos.

Ocho pruebas del observador pasan. Las nuevas comparan campo, energía y
flujo contra la ruta anterior y el factor independiente, con memoria viva,
base completa y base incompleta. Se verifican también las variantes sin
suavizado, suavizada y de cuarta derivada; plegado por reflexión activado
y desactivado, pares reversos exactos y geometría original sin cambios.
Se rechazan métodos/probes inválidos.
Una alteración deliberada del factor proyectado se rechaza mediante su
control de norma incluso cuando no se ejecutan las tres sondas directas.

## Medición completa

El auditor autentica el observador anterior desde **e6c3a93** en Git y el
SHA de su informe; permite únicamente el cambio explícito de observación.
Todos los otros fuentes congelados, tabla y R deben coincidir. No relaja
la autenticación de las fuentes físicas ni sustituye informes históricos.

Reconstruye los mismos 3 ms de contacto a 768 kHz y los tres coast de
0,5 ms a 64/128/256 kHz con todos los estados vivos. Compara los diagnósticos
de dinámica, presión compleja/temporal y tiempos nativos comunes con los
informes anteriores. El nuevo certificado recorre todos los bloques y
compara almacenamiento, flujo y mínima potencia; las estimaciones anteriores
deben quedar dentro de los nuevos intervalos. El refinamiento de trabajo
se evalúa nuevamente con sus extremos adversos y los límites originales.

En tres bloques —primero, central y último— se compara además el factor
proyectado directamente con `H @ Q`, registrando por separado sus tiempos.
La comparación del tiempo total con el informe previo es **histórica**:
no constituye una medición aislada de dos ejecuciones completas. El tiempo
nuevo incluye esas sondas y la regeneración de ambos factores.

El [ensayo completo](medidas-P6-coast-campo/coast-proyeccion-celdas.json)
pasa **17 controles**. Reconstruye exactamente los 28 diagnósticos de
dinámica, los observables de tabla interpolada/nativa y las cuatro
comparaciones de presión compleja/temporal: diferencia relativa máxima
**0**. La misma base tiene rango **351**, cola 9,679745e−13; permanecen
los 606 modos físicos y todos los estados vivos. Se recorren **384 bloques**.

| Control de campo completo | Medida | Límite |
| --- | ---: | ---: |
| Mayor cambio de energía, flujo o mínima potencia | 2,750478e−16 | 1e−10 |
| Norma completa frente a R[0] | 0 | 1e−10 |
| Norma proyectada frente a Q.T R[0] Q | 2,133942e−16 | 1e−10 |
| Mayor cambio de factor en las tres sondas | 6,574477e−15 | 1e−10 |
| Replay de las tres memorias completas | 0 | 1e−11 |
| Balance total con extremos adversos | 4,583048e−11 | 1e−9 |
| Balance de aire con extremos adversos | 4,555940e−11 | 1e−9 |
| Máxima cota de observación | 2,326217e−7 | 1e−4 |

Las estimaciones anteriores quedan dentro de los nuevos intervalos. El
flujo fino es **9,623701568e−8 J**, con límite inferior positivo; se mide
desde el campo y no como resto del trabajo. Los seis términos de trabajo
fino pasan, máximo **0,0496604%** frente a 0,2%. Los controles nativos,
presión, captura y kernel reproducen los anteriores, con sus mismos límites.
El fallo original de orden en grilla interpolada continúa en el informe
padre; no se convierte en aprobación de toda la extensión spline.

## Coste y decisión

La identidad matemática es correcta, pero esta implementación **no acelera
el banco actual**. El certificado tarda **1488,233 s** (24,804 min), frente
a 1107,991 s (18,467 min) históricos: **34,3182% más tiempo**. Todo el
ensayo tarda **1798,766 s** (29,979 min); pico propio **4,655491 GiB** frente
a 4,308224 GiB del ensayo anterior.

| Desglose nuevo | Tiempo |
| --- | ---: |
| Proyectar las formas una vez | 0,399 s |
| Regenerar el factor completo | 726,561 s |
| Construir el segundo factor con formas proyectadas | 499,478 s |

Las formas proyectadas agregan **138.018.816 bytes**. Las historias de
inputs conservan sus 408.844.800 bytes en RAM; no hay otra R.

| Bloque, índice desde cero | H @ Q directo | L(S Q) espacial | Diferencia de factor |
| --- | ---: | ---: | ---: |
| 0 | 0,311 s | 1,519 s | 6,574477e−15 |
| 192 | 0,314 s | 1,391 s | 5,777795e−15 |
| 383 | 0,309 s | 0,866 s | 4,681818e−15 |

Estas tres comparaciones locales usan el mismo bloque completo regenerado
y Q, un hilo BLAS y el mismo proceso. El camino espacial evita un producto
matricial denso, pero repite histogramas y sus productos dispersos. Sus
tiempos locales resultan 2,80–4,88 veces mayores. No demuestran el coste
para otro rango, geometría o hardware. La comparación de certificados
completos sigue siendo histórica, sin dos ejecuciones aisladas emparejadas.

**Decisión:** conservar `factor` por defecto. `cells` queda como alternativa
explícita y control de equivalencia, no como aceleración aprobada. No se
extiende la escucha ni se cambia la física para compensar su coste. Una
posible mejora posterior debe reutilizar la construcción espacial o la
simetría de pares en vez de generar dos factores; necesita sus propias
medidas antes de adoptarse.

No se generan WAVs, paquetes ni copias en SHARED. Captures e historias sólo
viven en RAM y se reutiliza la R local única. La puerta general y la
aprobación del timbre permanecen falsas.

## Siguiente

Ampliar los parciales de cuerda y comprobar su convergencia espectral con
el puente/carga completos antes de extender la duración audible. El banco
actual mantiene tres modos transversales por dirección y seis axiales;
aprobar 606 modos de tabla no recupera esos parciales ausentes. Repetición,
apagador/release, unísonos e identificación del piano real siguen pendientes.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1; BLAS de un hilo y sin bytecode.
Elegir un informe nuevo. Para reproducir los auditores históricos usar
sus fuentes congelados: sus SHA estrictos rechazan el observador modificado.
El nuevo auditor registra ese cambio explícito y conserva sus controles.

```powershell
python tools/audit-concert-grand-cell-factor-projection.py --field-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-0p5ms.json --alignment-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-grilla-comun.json --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVA_PROYECCION.json
```
