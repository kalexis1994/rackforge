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

Pendientes del ensayo completo. Mantener `all_gates_passed=false` incluso
si pasa este coast: quedan banda relevante superior de fuente/carga,
balance angular del contacto completo, mayor duración, acción/release,
unísono, identificación real y escucha. Sin plugin ni WAV nuevo.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1, BLAS de un hilo, sin bytecode.
Historias en RAM, presupuesto conservador de arrays de 12 GiB; informe
único en el repo y ninguna copia en SHARED. Una R existente en local.

```powershell
python tools/audit-concert-grand-refined-coast-field.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --resolution-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-resolucion-cuerda/resolucion-dirigida.json --coast-reference-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-0p5ms.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVO_COAST_REFINADO.json
```
