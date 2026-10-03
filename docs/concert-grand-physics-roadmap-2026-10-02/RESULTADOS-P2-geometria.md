# P2 — resolución y geometría global del puente

2026-10-02. Continuación del piloto P2. Sin selección, paquete o calibración
nuevos. Resultados en `C:\SHARED\Fisica-pendiente-2026-10-02`.

## 1. Resolución: mantener el mismo modelo físico

Se mantuvieron modos, masas, coeficientes nativos y geometría del observador.
Se reconstruyó la observación a partir de los modos Ritz congelados y se refinó
la cuadratura 40×24 → 80×48 → 160×96 → 320×192. También se comparó la interpolación
de la línea del puente con 16/32/64/128 puntos frente a las formas continuas.
La reconstrucción independiente en las frecuencias modales coincide con el
observador digital exportado, con error relativo <0,001 en los tres bancos.

Se evalúa presión causal de fuerza armónica unitaria, 88 posiciones de notas,
frecuencias 20–8000 Hz. Es una verificación numérica del **banco estructural
bajo 800 Hz** y su respuesta forzada; no del complemento sintético alto ni
de un piano completo con sala, tapa y ruido. Las integrales son dentro del
observador Rayleigh heredado, sin realimentación acústica sobre la estructura.

| Banco | Error relativo de presión 40×24 frente a 320×192 | Error relativo de formas en puente, 16 puntos | Con 64 puntos |
| --- | ---: | ---: | ---: |
| Panel | 0,0840% | 7,4743% | 0,4368% |
| Panel + puente | 0,0559% | 2,6499% | 0,1516% |
| Panel + costillas + puente | 0,0313% | 3,1951% | 0,1833% |

Las normas globales pueden ocultar cancelaciones locales. Por eso se guardan
también siete alturas, F#2/C3/C4/C5/C6/F#6/C7, H1/H2/H3. El mayor cambio de nivel
por los 16 puntos en esos casos es ≈0,806 dB; en el banco con costillas, ≈0,550 dB.
En C5, ese error del fundamental es ≈0,121 dB. La discretización no explica
su déficit tardío de casi 30 dB en el ensayo anterior. **No se alteró el
renderer para aumentar puntos como si eso recuperara el cuerpo.** Queda
pendiente usar mayor resolución si un banco fuera a promoverse.

La geometría activa del renderer es el par vertical heredado, centrado en la
tabla, `BOARD_ROOM_GEOMETRY=0`; no es una reconstrucción medida del par ORTF
de la grabación. La coincidencia con el observador nativo no certifica
coincidencia con los micrófonos reales.

## 2. Curva: estructura y punto de fuerza deben corresponderse

El banco recto tiene 35 modos bajos; 16 tienen una proyección de fuerza
prácticamente nula en la línea central (norma <1e−7 del máximo entre modos).
Con un contorno y apoyos simétricos, esas configuraciones no se excitan desde
esa línea. Eso es una propiedad de este modelo idealizado; no se afirma
haber identificado el problema real del Steinway.

La relevancia mecánica de puente y regiones de tabla está documentada por
[Boutillon y Ege](https://arxiv.org/html/1305.3057v1). Nuestro ensayo de curva
es una derivación reducida propia, no la reproducción de su piano o método.

Se supone una línea `y(x)=0,5 m + 0,10 m sin(pi x / 1,4 m)`, manteniendo madera,
sección, espesor de panel, costillas y apoyos. Sin ajuste por nota ni nuevo
detune. La amplitud de 10 cm es una hipótesis global, no una medida del Steinway.

Se restringe el desplazamiento de placa a la línea y se calcula su segunda
derivada respecto al arco:

```
g = 1 + y'(x)^2
W_x  = w_x + y' w_y
W_xx = w_xx + 2 y' w_xy + y'^2 w_yy + y'' w_y
W_ss = W_xx/g - W_x y' y''/g^2
M_bar = integral mu Phi Phi^T ds
K_bar = integral EI Phi_ss Phi_ss^T ds
```

Matrices Gram positivas; se añaden a la placa. Sigue siendo flexión de una
línea centroidal: falta torsión de barra curva, excentricidad/composición,
membrana, corona y contorno real. Este modelo no identifica toda la física
del puente.

Controles:

- **Recto:** asset anterior idéntico, 35 modos bajos + 121 altos heredados.
- **Curva-solo-trayectoria:** mismas masas, polos y superficie rectos, pero la
  fuerza y carga se evalúan sobre la curva. Diagnóstico de mover el punto de
  contacto sin trasladar la estructura; no representa un puente consistente.
- **Curva-estructural:** masa, rigidez, polos, formas, trayectoria de fuerza,
  carga y superficie corresponden al mismo puente curvo. 33 modos <800 Hz;
  conserva los mismos 121 modos altos, todavía sintéticos.

## 3. Verificaciones

- Derivación respecto al arco frente a diferencias finitas: error relativo
  2,58e−7. Curva nula recupera masa/rigidez de la viga recta: <1,73e−15.
- Masa y rigidez pasan Cholesky. Ortonormalidad en masa <1,02e−14 y residuo
  modal relativo <4,04e−13.
- Banco curvo 52×32 frente a 44×28: cambio máximo de 256 polos 0,2957%;
  movilidad entre tres puntos, 20–1000 Hz, cambia 0,1343%.
- Refinar la cuadratura de la línea a 1008 puntos cambia matrices <2,09e−14.
- Admitancia recíproca, error <1,74e−18; conductancia semidefinida positiva
  en puntos/frecuencias probados, autovalor mínimo +2,17e−7.
- Tres pruebas del renderer congelado verifican masa unitaria, mismas
  proyecciones de fuente/carga y conservación de modos altos.
- Los cuatro ataques fuertes de Base y Recto coinciden exactamente con sus
  respectivos controles causales del ensayo anterior.

## 4. Audio: ninguna mejora global aceptada

Renderer congelado, 44,1 kHz/256, observador causal en **todas** las variantes.
C4/C5/C6/F#6 × velocidades 28/60/92/124; tecla 2 s, salida 4 s, más sostenidos
de 12 s a velocidad 124. Misma ganancia, sin EQ ni normalización. La referencia
Steinway es la misma ORTF con sus hashes/procedencia verificados.

| Perfil | Score medio, 16 casos | Peor caso |
| --- | ---: | ---: |
| Base con observador causal | 7,81308 | 11,38369 |
| Recto | 8,81989 | 12,83527 |
| Curva-solo-trayectoria | 11,01138 | 22,80295 |
| Curva-estructural | 7,75351 | 10,36043 |

La curva consistente reduce error respecto al banco recto y mejora el peor
caso respecto a la base causal, pero apenas cambia el promedio de esa base
(−0,76%). La distribución no es uniforme:

| Nota, promedio de cuatro intensidades | Base causal | Recto | Curva estructural |
| --- | ---: | ---: | ---: |
| C4 | 6,5936 | 8,3911 | 7,9815 |
| C5 | 6,0891 | 12,4307 | 9,2005 |
| C6 | 9,0234 | 6,9000 | 6,3504 |
| F#6 | 9,5462 | 7,5578 | 7,4817 |

**La base instalada con el observador habitual obtiene 6,83418** en estos
16 casos. El control causal es necesario para aislar geometría, pero no debe
confundirse con el sonido instalado ni usarse para anunciar que el nuevo
modelo supera al actual.

C5/124: error del fundamental tardío Recto 29,883 dB → Curva 22,856 dB;
Base causal 4,383 dB. En C4 la curva pierde la recuperación tardía del recto:
2,727 → 17,189 dB, frente a Base causal 11,406 dB. El score no basta para
promover una variante y no se hicieron retoques locales para cubrir esos casos.

## 5. Separar carga y presión aclara C5

Un segundo diagnóstico usa los coeficientes nativos completos y una fuerza
armónica fija. Se calcula por separado la conductancia de puente y la
transferencia de presión causal a los dos micrófonos. Incluye el complemento
alto heredado, con su cuadratura nativa. No incluye envolventes de cuerda,
pedal, sala, preamplificador o cambio de frecuencia por amplitud.

Para el fundamental de C5:

| Banco | Conductancia / base | Presión por fuerza fija frente a base |
| --- | ---: | ---: |
| Recto | 4,900× | −12,159 dB |
| Curva-solo-trayectoria | 53,004× | −5,402 dB |
| Curva-estructural | 5,361× | −4,265 dB |

La curva consistente mejora la proyección de presión frente al recto, pero
la carga mecánica continúa muy alta. Mover sólo la trayectoria excita modos
que antes estaban oscuros, sin garantizar que radien al par centrado; la carga
puede crecer mucho mientras el sostenido cae. Eso explica por qué agregar
modos excitados no equivale a recuperar cuerpo en este prototipo.

La conductancia no se puede convertir directamente en el decaimiento audible:
hay normalización de cuerda, proyección colectiva, pérdidas intrínsecas y
movimiento no lineal. Tampoco está medida la conductancia del Steinway.
El diagnóstico localiza una sensibilidad del modelo, no identifica una masa
real ni justifica reducir arbitrariamente la carga.

## Archivos y siguiente etapa

- `P2-resolucion/report.json`: convergencia y controles del observador.
- `P2-puente-curvo-estructural`: bancos, pruebas y convergencia mecánica.
- `P2-puente-curvo-audio`: audios individuales e intensidades de cada variante;
  28/60/92/124 empiezan a 0/3/6/9 s. Comparador: Base causal / Curva / Steinway.
- `P2-puente-curvo-rutas`: carga y transferencia separadas.
- `resumen-P2-geometria.json`: hashes, métricas y controles compactos.
- `local/concert-grand-ritz/build/source-geometria.zip`: fuentes congeladas
  de estos scripts y geometría; renderer nativo anterior sin recompilar.

**Sin selección.** La cuadratura baja no explica el déficit; geometría y
admitancia importan y deben conservarse junto con la proyección acústica.
P2 sigue siendo parcial: geometría, micrófonos y masa efectiva no identificados,
complemento alto sintético y realimentación mecánica aproximada a frecuencia
congelada. No está verificado para cambios en vivo, pedales o WASM.

El siguiente paso del orden de trabajo es **P3: auditoría de energía y
normalización de las cuerdas**, empezando por los términos cruzados de
`tension_step`. Separar esa auditoría de otra recalibración de tabla. Conservar
como pendiente P2 la comprobación de validez de la carga débil cerca de los
picos de movilidad, para no reemplazarla por un simple ajuste de pérdidas.
