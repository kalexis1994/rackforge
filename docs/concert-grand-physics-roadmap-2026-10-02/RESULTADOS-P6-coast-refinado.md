# P6 — Campo y propagación desde el contacto refinado

Fecha: 2026-10-07. Continúa la [matriz dirigida de cuerda/tiempo](RESULTADOS-P6-resolucion-dirigida.md).

## Diseño fijado antes de medir

Reconstruir la cuerda C4 supuesta de **96 transversales por dirección/192
axiales**, contacto de 3 ms a **3072 kHz**, mismos 5 m/s, fieltro, puente,
606 coordenadas de tabla y única R. Comprobar los diagnósticos del contacto
anterior y la continuación a 256 kHz, incluida presión de la ventana
0–3,5 ms, con límite de reproducción relativo **1e−10**.

Desde el mismo extremo mecánico/acústico vivo, tres coast de **0,5 ms** a
64/128/256 kHz. Ningún reset de cuerda, tabla, aire o martillo entre seed
y continuación. Mantener todo el estado de la trayectoria; sólo la
observación tiene base de datos Q con residuo acotado y replay completo.

El certificado establecido regenera todos los ángulos/taps y mide
almacenamiento y flujo positivos, cotejando norma completa y proyectada
con la R autenticada. El flujo **no** se infiere del trabajo mecánico.
Presión: contribución finita de aceleración a 3–3,5 ms, Green de celdas
continuo, grilla común de 256 kHz, bandas 60–480/480–1980/1980–6000/60–6000
Hz y temporal DC–6 kHz, sin impulsos delta en extremos.

Se mantienen los umbrales del certificado anterior: balances adversos
**1e−9**, precisión de observación **1e−4**, trabajo/energías **0,2%**
(piso 1e−14 J), presión/captura **0,1%**, tabla **1%**, kernel **0,02%**,
replay **1e−11**, pérdidas no negativas y flujo inferior positivo >1e−14 J.
La razón de tabla se mide en los 33 tiempos **nativos comunes** a 64 kHz,
sin interpolar, con límite **0,35** ya usado en el control de alineación.
La razón en grilla interpolada se registra separadamente como diagnóstico;
no sustituye ni cambia el criterio nativo fijado aquí.

## Prototipo espectral: comparación, no adopción

En los bloques angulares **0 y central**, comparar campo complejo y flujo
por Parseval de cada coast con replay temporal. Dos tamaños FFT por ventana:
N y `next_fast_len(N+7)`, bloques de 16 frecuencias. Conservar cada memoria
inicial/final y fase de padding; **12 comparaciones**, umbral **1e−10** para
campo y flujo. La vista de observación deja intactos proveedor y R físicos.
El certificado completo sigue usando la ruta temporal establecida.

Se separa el tiempo de preparación/replay del producto temporal con inputs
guardados. El kernel espectral incluye FFT del puerto, reconstrucción de
secciones y productos de campo. Generación del factor/proyección son costes
comunes excluidos de la comparación local. Estas sondas no aprueban una
aceleración global ni sustituyen el almacenamiento/flujo angular completo.

Once pruebas pasan (0,356 s): tres nuevas comprueban equivalencia exacta
del certificado con/sin vista de sondas, proveedor/R inmutables, rechazo de
extremos reiniciados y convergencia nativa sin spline; ocho controles del
observador existente pasan junto a ellas. No cambian fuentes físicas del padre.

## Resultados

Ensayo completo: **15/16 controles**, `coast_gates_passed=false` y
`all_gates_passed=false`. Informe único:
[coast-96modos.json](medidas-P6-coast-campo/coast-96modos.json).
Duración **2231,768 s (37,20 min)**; certificado angular con sondas
**1123,168 s (18,72 min)**; pico observado **4,853 GiB**, estimación
conservadora de arrays **8,157 GiB**, presupuesto 12 GiB.

Reproducción exacta del contacto/coast padre y de su presión en 0–3,5 ms.
Fuentes, padres, tabla, R, geometría, micrófonos y criterios heredados
verificados por hash/comparación después del ensayo. La base de observación
tiene rango **324/606**, cola relativa **9,89986e−13**; la dinámica conserva
las 606 coordenadas y toda la memoria. Replay completo: error **0**; norma
del factor completo contra R: **0**; norma proyectada: **3,51657e−16**.

| Control | Medida | Límite / resultado |
| --- | --- | --- |
| Balance total/aire adverso con cotas | máximo 4,32661e−11 relativo a energía inicial | <1e−9; pasa |
| Error de observación almacenamiento/flujo | máximo 1,53380e−6 relativo | <1e−4; pasa |
| Pérdidas y radiación | calor mínimo 0; potencia mínima positiva en cada coast | pasa |
| Tabla, tiempos nativos comunes | error 64→256: 0,00295840%; 128→256: 0,000695030% | <1%; pasa |
| Razón de tabla nativa | 0,234935 (interpolada sólo diagnóstica: 0,265902) | <0,35; pasa |
| Presión compleja 128→256 | 60–480: 0,000600151%; 480–1980: 0,000828139%; 1980–6000: 0,00311481%; conjunta: 0,00129775% | cada banda <0,1%; pasa |
| Presión temporal DC–6 kHz | 0,00129344% | <0,1%; pasa |
| Captura de presión fina | 0,00000576985% | <0,1%; pasa |
| Kernel continuo/discreto | 0,000716863% | <0,02%; pasa |
| Trabajo/energías 128→256 | falla únicamente radiación; detalle siguiente | <0,2%; falla |

### Fallo conservado: flujo radiado fino

El flujo integrado es **2,095536673 / 2,118341850 / 2,124156041 nJ**
a 64/128/256 kHz. Las cotas inferiores son positivas en los tres casos.
Entre 128 y 256 cambia **0,273718%**, o **0,274024%** incluyendo cotas,
por encima de **0,2%**. Diferencia observada **5,81419e−12 J**;
máxima diferencia acotada **5,82069e−12 J**. El ensanchamiento por
observación es sólo 0,112% de esa diferencia: este fallo no se resuelve
estrechando Q ni relajando el límite. El piso de 1e−14 J tampoco aplica:
la referencia radiada es 2,12416e−9 J.

Los otros cinco términos finos pasan: almacenamiento final de onda
**0,00388424%** con cotas; relajación final **0,0123296%**; calor de
relajación **0,0349496%**; calor material **0,000591339%**; trabajo del
puerto **0,00868893%**. Al final fino quedan **4,859779957 µJ** almacenados
en onda; el trabajo del puerto es **2,109134280 µJ**. La radiación se
observa independientemente, no por resta de esos términos grandes.
64→256 también falla flujo (1,34764% con cotas), registrado en el JSON.
Que presión hasta 6 kHz pase no certifica el flujo angular de banda completa.

### Sondas espectrales sobre el recorrido real

Las **12/12** comparaciones en bloques 0/192 pasan: error complejo máximo
**2,25109e−13**; flujo **1,47126e−13**, ambos <1e−10. Replay proyectado
máximo **5,11995e−15**. Tamaños FFT 32/40, 64/72 y 128/135; memoria viva
y fase de padding conservadas. El certificado temporal completo sigue
contabilizando almacenamiento/flujo y cotas del residuo a 606 coordenadas.
Las sondas cubren sólo dos bloques y sus 324 direcciones de observación.

Coste local por bloque/ventana: preparación del replay **0,304–1,690 s**,
producto temporal con inputs guardados **0,0257–0,0793 s** y kernel
espectral **0,276–0,874 s**. Para la misma pareja bloque/ventana, el kernel
espectral tarda **10,26–13,80 veces** el producto temporal (mediana 10,92).
El replay temporal puede compartirse entre bloques; no se debe cargar su
preparación de nuevo a cada producto para prometer una aceleración.
Estos tiempos excluyen generación/proyección comunes del factor; no son
un benchmark global ni de contacto largo. La medida de generación del
certificado en el JSON incluye las sondas interpuestas. **No se adopta
el prototipo por defecto**: conserva fase/flujo y evita la historia completa
de secciones, pero este ensayo corto no demuestra ahorro de tiempo.

## Siguiente paso y alcance

Primero añadir **512 kHz** al coast de 0,5 ms, desde el mismo seed fino
autenticado; comparar 256→512 con el mismo límite de trabajo/energías
0,2%, flujo positivo independiente, replay, presión y balance acotado.
La razón de error observada sugiere convergencia, pero no reemplaza ese
ensayo. Conservar este informe fallido y no seleccionar aún una tasa audible.
Después comprobar banda relevante superior de fuente/carga y el balance
angular del contacto completo antes de ampliar duración.

Quedan acción/release, unísono, identificación real y escucha. Sólo C4
supuesta, 0,5 ms de continuación; no nota completa ni aprobación de timbre.
Sin plugin ni WAV nuevo; paquete instalado 0.172.0 sin cambios.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1, BLAS de un hilo, sin bytecode.
Historias en RAM, presupuesto conservador de arrays de 12 GiB; informe
único en el repo y ninguna copia en SHARED. Una R existente en local.

```powershell
python tools/audit-concert-grand-refined-coast-field.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --resolution-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-resolucion-cuerda/resolucion-dirigida.json --coast-reference-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-0p5ms.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVO_COAST_REFINADO.json
```
