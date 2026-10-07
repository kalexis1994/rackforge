# P6 — Refinamiento de radiación a 512 kHz

Fecha: 2026-10-07. Continúa el [coast refinado de 96 modos](RESULTADOS-P6-coast-refinado.md), que pasó 15/16 y falló radiación 128→256 kHz: 0,274024% con cotas, límite 0,2%.

## Diseño fijado antes de medir

- Misma C4 supuesta, 96 transversales por dirección/192 axiales, fieltro,
  puente, 606 coordenadas de tabla y R única autenticada. Reconstruir el
  contacto de 3 ms a 3072 kHz/5 m/s; no modificar fuerzas o coeficientes.
- Tres coast de **0,5 ms a 128/256/512 kHz**, desde el mismo extremo vivo,
  conservando cuerda, tabla, aire y martillo. No variar la duración.
- Reproducir contacto y presión 0–3,5 ms de la matriz dirigida; reproducir
  física/presión de los coast 128/256 del padre en su grilla original
  de 256 kHz. Límite relativo heredado **1e−10**.
- Nuevo certificado temporal angular completo con base de observación Q
  y residuo acotado. Los intervalos de almacenamiento inicial/final y
  flujo 128/256 deben solaparse con los anteriores; no exigir igualdad
  bit a bit de estimaciones obtenidas con bases de observación distintas.
- Comparar **256→512** en los seis términos de energía/trabajo, cada uno
  con límite **0,2%** y piso 1e−14 J. El flujo se observa independientemente;
  no se infiere del trabajo mecánico ni se cambia el umbral por el fallo.
- Mantener balance adverso 1e−9, observación 1e−4, replay 1e−11, pérdidas
  no negativas, flujo inferior >1e−14 J, tabla 1% y razón nativa <0,35.
  Tabla: 65 tiempos comunes a 128 kHz, sin interpolación.
- Presión finita a 3–3,5 ms: misma geometría/Green continuo, bandas
  60–480/480–1980/1980–6000/60–6000 Hz y temporal DC–6 kHz, grilla común
  **512 kHz**. Cada error fino <0,1%, captura <0,1%, kernel <0,02%.
  La reproducción del padre se evalúa separadamente a 256 kHz.

Quince criterios específicos: reproducción física/presión, solapamiento del
campo padre, autenticación del seed, replay, balance, observación, pérdidas,
flujo positivo, refinamiento de trabajos, tabla nativa, razón, presión,
captura, kernel y fuentes inalteradas. Las sondas espectrales ya medidas
no se repiten ni se adoptan; el certificado usa la ruta temporal vigente.

## Estado persistente único

Tras reproducir el contacto y los coast anteriores, guardar una única
cápsula local NPZ con **los cuatro estados completos** (posición, velocidad,
memoria de onda y relajación). Hash del payload, fuentes del productor,
padres, tabla, R, geometría y tasa/contacto quedan enlazados por un manifiesto
pequeño. Leer de vuelta y exigir igualdad exacta de todos los arrays. No
sobrescribir una cápsula existente ni permitir una R/carga distinta.

La cápsula es un cache de cálculo reutilizable, no un informe duplicado.
No contiene R ni historias B/V/puerto, no se versiona y no va a SHARED.
Permite a una siguiente herramienta continuar sin repetir el contacto;
no acelera la dinámica de la primera reconstrucción ni certifica otro
contacto, velocidad o geometría. El lector verifica bindings/dimensiones,
pero corresponde al siguiente auditor autenticar sus archivos de origen.

## Resultados

Cinco pruebas de cápsula/intervalos pasan (0,041 s): igualdad exacta de los
cuatro arrays, prohibición de sobrescritura, rechazo de bindings/payload
alterados, dimensiones/valores inválidos y cotas independientes disjuntas.
Preflight de la tabla real: estimación conservadora **9,941 GiB**, menor
que el presupuesto de 12 GiB. No requiere nueva R ni historias en disco.

Pendientes. Conservar `all_gates_passed=false` aun si pasa este coast: faltan
banda superior de fuente/carga, balance angular del contacto completo,
mayor duración, release, unísono, identificación real y escucha.

## Reproducción

Python/NumPy/SciPy del entorno refit, BLAS de un hilo, sin bytecode.
Historias en RAM, presupuesto conservador de arrays 12 GiB. Un solo
informe en repo; estado local único de unos pocos MB, ninguna R nueva.

```powershell
python tools/audit-concert-grand-coast-512.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --resolution-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-resolucion-cuerda/resolucion-dirigida.json --coast-reference-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-96modos.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --seed local/concert-grand-physics/P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz --report NUEVO_COAST_512.json
```
