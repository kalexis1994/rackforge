# P6 — Separar resolución espacial de cuerda y paso de contacto

Fecha: 2026-10-07. Continúa los [dos fallos conservados del barrido](RESULTADOS-P6-resolucion-cuerda.md).

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

Pendientes del ensayo dirigido. El informe conserva checkpoints y no
aprueba el modelo global aunque pasen sus controles de resolución.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1; BLAS de un hilo, sin bytecode.
Una R bajo `local/concert-grand-physics`, sin nueva caché ni copias en SHARED.
Se declara presupuesto de arrays de 12 GiB antes de construir el aire; no
se materializa historia angular de paso×tap×rango. Es una estimación de
arrays, no un límite del sistema operativo. Cada tentativa usa informe nuevo.

```powershell
python tools/audit-concert-grand-string-resolution-directed.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --resolution-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-resolucion-cuerda/resolucion-completa.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVA_RESOLUCION_DIRIGIDA.json
```
