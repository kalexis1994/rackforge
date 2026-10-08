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

Pendientes de la ejecución completa.
