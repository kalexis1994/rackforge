# Concert Grand — cola de investigación e implementación

Papers pendientes de incorporar al modelo, en orden de implementación. Cada
item se marca al aterrizar en `plugins/concert-grand`, con la sección de
`PIANO_MODEL.md` que lo documenta.

## 1. Histéresis del fieltro (Stulov) — [x] (v0.19.0)

**Fuente:** A. Stulov, "Hysteretic model of the grand piano hammer felt",
*JASA* 97 (1995); PDFs en https://homes.ioc.ee/stulov/klaver2.pdf y
https://www.ioc.ee/~stulov/smac03.pdf.

**Qué dice:** todo martillo medido tiene característica fuerza-compresión
histerética: el fieltro es un material con memoria,
`F(t) = F0·[ξ^p − (ε/τ0)·∫ ξ^p(t')·e^-((t-t')/τ0) dt']`,
con dos parámetros hereditarios (ε, τ0) además de (F0, p). La carga es más
rígida que la descarga; el contacto disipa energía.

**Qué cambia en el modelo:** `simulate_strike()` usa hoy un resorte sin
pérdidas `F = K·ξ^2.5` — demasiado elástico, pulso simétrico irreal. La
histéresis acorta y asimetriza el pulso y oscurece la descarga. Es un filtro
exponencial corriente sobre ξ^p dentro del integrador: costo despreciable.

## 2. Validación de fases del bajo (Galembo–Askenfelt–Cuddy) — [x] (v0.19.1) — resultado: fases ordenadas medidas e impuestas, efecto audible casi nulo según el usuario; las fases no eran el cuello de botella

**Fuentes:**
- "Effects of relative phases on pitch and timbre in the piano bass range"
  (las fases relativas entre parciales son audibles en el registro grave).
- "Perceptual significance of inharmonicity and spectral envelope in the
  piano bass range" (el ancho de banda espectral pesa más que la
  inharmonicidad en el timbre grave).

**Qué cambia:** el sim del martillo (v0.18.0) ya produce fases correlacionadas;
estos papers dan el protocolo para validarlas contra las muestras YDP
(medir fases relativas de los primeros ~10 parciales del bajo en el ataque,
comparar real vs modelo con `tools/analyze-piano-sf2.py` extendido). También:
priorizar ancho de banda del bajo sobre ajustes finos de B.

## 3. Parámetros medidos de un grand real (Chabassier–Chaigne–Joly) — [~] parcial (v0.20.0): exponente del fieltro por registro (1.7→3.4, su rango medido 1.5–3.5) desde la Part 1 (M2AN 2014, PDF guardado); faltan las tablas numéricas por nota (masa, kH) del paper JASA/Part 2

**Fuente:** J. Chabassier, A. Chaigne, P. Joly, "Modeling and simulation of
a grand piano", *JASA* 134 (2013) — el modelo numérico completo de un
Steinway D, con tablas de parámetros por nota: masas de martillo, rigidez y
exponente del fieltro, datos de cuerda (tensión, masa, longitud, diámetros).

**Qué cambia:** `simulate_strike()` usa escalados inventados
(masa 0.06+0.85·pos^1.3, rigidez desde el tiempo de contacto objetivo).
Reemplazarlos por los valores medidos del paper, nota por nota
(interpolados), y derivar los tiempos de contacto en vez de imponerlos.

## 4. Pérdidas de cuerda de dos parámetros (Bensa et al.) — [ ]

**Fuente:** J. Bensa, S. Bilbao, R. Kronland-Martinet, J. O. Smith, "The
simulation of piano string vibration", *JASA* 114 (2003).

**Qué cambia:** nuestra curva T60(f, string_scale) es un fit global; el
modelo b1/b3 (fricción de aire + pérdida viscoelástica) da forma medida por
cuerda con dos parámetros publicados.

## 5. Tabla armónica medida en medios-agudos (Ege, Boutillon) — [ ]

**Fuente:** K. Ege, X. Boutillon, "Synthetic description of the piano
soundboard mechanical mobility" / tesis de Ege (2011).

**Qué cambia:** el board_response sintético (3 senos en log-f) y los 18 modos
del cuerpo podrían seguir la densidad modal y movilidad medidas, incluida la
transición a comportamiento de placa nervada (~1.1 kHz).

## Referencia permanente

- Muestras de calibración: YDP Grand en
  `rackforge-plugin-rf-dls/artifacts/rf-soundfonts-0.2.0.rfplugin`
  (assets/ydp-grand-piano.sf2); script `tools/analyze-piano-sf2.py`.
- Reglas aprendidas: sonido seco primero (staging a cero hasta nuevo aviso);
  nada de redes difusas (leen como reverb); el sustain agudo debe venir de
  resonancias afinadas discretas; el HF sostenido del bajo es del la cuerda
  (amortiguamiento por cuerda, no por frecuencia).
