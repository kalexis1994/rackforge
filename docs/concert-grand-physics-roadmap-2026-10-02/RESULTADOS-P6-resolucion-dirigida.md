# P6 — Separar resolución espacial de cuerda y paso de contacto

Fecha: 2026-10-07. Continúa los [dos fallos conservados del barrido](RESULTADOS-P6-resolucion-cuerda.md).
Auditor y pruebas: **27848bc**.

## Diseño fijado antes de medir

Matriz de cuatro casos: **48/96 transversales por dirección**, 2N axiales,
contacto a **1536/3072 kHz**. Mismos 3 ms de contacto y 0,5 ms de coast a
256 kHz; cuerda C4 supuesta, 5 m/s de martillo, fieltro, 606 coordenadas de
tabla y única R sin cambios. Cada coast continúa el extremo vivo de su
contacto. Historias físicas de tabla en RAM; no se guardan WAV ni paquetes.

Se reconstruye el caso 48/96 a 1536 kHz para comprobar contra los
diagnósticos mecánicos y presión del padre con límite relativo 1e−10.
No basta comparar integrales de bandas: cada nueva historia conserva su
presión compleja para las cuatro parejas dirigidas:

- 48→96 transversales a cada una de las dos tasas, límite de presión y
  posición/velocidad física de tabla **1%**.
- 1536→3072 kHz con cada uno de los dos bancos, límite de presión **0,1%**.

Se mantienen todas las bandas y umbrales anteriores: 60–480, 480–1980,
1980–6000 y 60–6000 Hz, y presión temporal DC–6 kHz. Una banda fallida
en cualquier pareja impide aprobar el conjunto. Trabajo mecánico por paso
1e−9, impulso/momento 1e−10, captura 0,1%, seis trabajos/energías de contacto
0,2% con piso 1e−14 J; separación y calor no negativo. No se modifican
límites al terminar. Sin certificado angular ni aprobación del timbre.

Siete pruebas pasan (4,408 s): energía/gradiente/Hessiana contra Gauss
independiente hasta 96 transversales/384 axiales, incluido extremo móvil;
cierre axial ideal, contacto/trabajo y continuidad viva; las parejas usan
referencias a igual tasa/base, y fallar una banda en cualquier pareja se
conserva como fallo. Los módulos de física y observación del padre permanecen
autenticados por SHA. La carga sólo está validada hasta 6 kHz, aunque la
cuerda refinada contiene frecuencias libres superiores: sigue pendiente
validar la banda relevante antes de adoptar el banco para audio.

## Resultados

El [ensayo completo](medidas-P6-resolucion-cuerda/resolucion-dirigida.json)
terminó en **3082,568 s (51,38 min)**, pico **3,287 GiB**. Pasan **14/14
controles**; `directed_gates_passed=true`. `all_gates_passed=false` y
`preflight_only=true`: no es una certificación de una nota completa ni del
piano real. No hay ajuste de parámetros ni cambio del plugin instalado.

### Cuatro contactos con idéntico impulso incidente

| Transversales/axiales | Tasa de contacto | Mayor frecuencia transversal libre | Fuerza máxima centrada | Tiempo de contacto/captura |
| --- | ---: | ---: | ---: | ---: |
| 48/96 | 1536 kHz | 18511,72 Hz | 16,18506263 N | 498,505 s |
| 48/96 | 3072 kHz | 18511,72 Hz | 16,18508386 N | 967,536 s |
| 96/192 | 1536 kHz | 59966,80 Hz | 16,18453737 N | 500,429 s |
| 96/192 | 3072 kHz | 59966,80 Hz | 16,18455596 N | 986,373 s |

Los dos bancos contienen los mismos 20 modos libres bajo 6 kHz. Los modos
superiores influyen en el contacto; ampliar la base no equivale a observar
más parciales dentro de esa banda. La fuerza ya cambia muy poco de 48 a 96,
en contraste con la gran diferencia entre tres y 48 del barrido anterior.
Todos los casos quedan separados a los 3 ms. En el más fino, la primera
separación está entre 1,823242 y 1,823568 ms.

### Resolución espacial comprobada a ambas tasas

Errores relativos de 48 frente a 96; sin igualar ganancias. El límite es
1% para presión compleja de **cada** banda, temporal y tabla física.

| Observable | A 1536 kHz | A 3072 kHz |
| --- | ---: | ---: |
| Presión compleja 60–480 Hz | 0,008559% | 0,008559% |
| 480–1980 Hz | 0,018215% | 0,018215% |
| **1980–6000 Hz** | **0,038670%** | **0,038665%** |
| 60–6000 Hz | 0,017046% | 0,017046% |
| Presión temporal DC–6 kHz | 0,016612% | 0,016612% |
| Posición física de tabla | 0,005559% | 0,005559% |
| Velocidad física de tabla | 0,011472% | 0,011472% |

Pasan ambas filas espaciales. El fallo anterior 24→48 de 1,506794% queda
en su informe, no se convierte en aprobado. La nueva pareja 48→96 pasa
al ampliar la base, también cuando se mantiene fija cualquiera de las
dos tasas nuevas.

### Resolución temporal comprobada con ambos bancos

Errores de contacto a 1536 frente a 3072 kHz, coast fijo a 256 kHz.
El límite para cada presión es **0,1%**.

| Observable | 48 transversales | 96 transversales |
| --- | ---: | ---: |
| Presión compleja 60–480 Hz | 0,000240% | 0,000239% |
| 480–1980 Hz | 0,002050% | 0,002049% |
| **1980–6000 Hz** | **0,025344%** | **0,025329%** |
| 60–6000 Hz | 0,003614% | 0,003612% |
| Presión temporal DC–6 kHz | 0,003518% | 0,003517% |

Pasan ambas columnas temporales. El error anterior 768→1536 del banco de
48 era 0,101373% y fallaba; permanece registrado. En el mismo banco, el
nuevo error de esa banda es aproximadamente una cuarta parte, compatible
con orden dos para este observable temprano. No prueba orden uniforme de
todas las coordenadas, frecuencias o ventanas.

### Controles auxiliares y procedencia

Los diagnósticos comparados del banco 48/96 a 1536 kHz se reproducen
**exactamente**: contacto/coast, pico/RMS e integrales de presión. Máximos
de todo el ensayo:

- Balance mecánico por paso: **3,19994e−12** frente a 1e−9.
- Impulso/momento de martillo: **5,16455e−15** frente a 1e−10.
- Captura de presión: **3,88524e−8 relativa**, frente a 0,001.
- Refinamiento de seis trabajos/energías en ambos bancos: **0,00123338%**,
  frente a 0,2%. Todos los calores registrados son no negativos.

Se verificaron después del cierre los SHA de productor y helpers, informes
padres, tabla y la única R. Criterios, geometría y micrófonos coinciden con
el padre. La R conserva SHA `484c8bcdbb334e97b26a8044536f2a5f19e3da2004df44b01c1df8b6d29777f7`;
no se creó otra caché ni se escribieron WAVs/paquetes en SHARED.

## Decisión y siguiente etapa

La pareja 48/96 y 1536/3072 **pasa la convergencia exigida de presión
temprana dentro de 0–6 kHz**. Se puede usar 96/192 a 3072 kHz como referencia
de esa ventana para los próximos controles; no se adopta aún como instrumento
audible ni se afirma fidelidad al Steinway. La carga, geometría y materiales
siguen siendo los del banco supuesto, y fuera de 6 kHz falta validación.

Siguiente: comprobar banda relevante de la fuente/carga, balance angular
independiente y sensibilidad temporal del coast del banco refinado desde
su contacto vivo, después ampliar duración. No heredar automáticamente el
certificado angular o de propagación obtenido con tres modos o con el
banco 48. Para el coste del campo existe un [prototipo finito](NOTA-P6-observador-ventana-finita.md)
con estados de ambos extremos: aún requiere replay enlazado, almacenamiento
independiente y comparación completa de flujo/coste antes de adoptarlo.

Este ensayo guarda diagnósticos, no espectros complejos, trayectorias ni
memorias finales. El siguiente certificado deberá reconstruir el contacto
de referencia con las mismas fuentes autenticadas y conservar sus puertos
y extremos en RAM durante los controles vinculados. No inferir radiación
del cierre mecánico ni volver a correr todo el barrido grueso.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1; BLAS de un hilo, sin bytecode.
Una R bajo `local/concert-grand-physics`, sin nueva caché ni copias en SHARED.
Se declara presupuesto de arrays de 12 GiB antes de construir el aire; no
se materializa historia angular de paso×tap×rango. Es una estimación de
arrays, no un límite del sistema operativo. Cada tentativa usa informe nuevo.

```powershell
python tools/audit-concert-grand-string-resolution-directed.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --resolution-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-resolucion-cuerda/resolucion-completa.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVA_RESOLUCION_DIRIGIDA.json
```
