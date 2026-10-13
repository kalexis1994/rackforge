# P7 — Incógnitas nulas exactas y pasadas de FFT

2026-10-13. Continuación autorizada desde **a547c6bb**. Prototipo de un coro
a puente fijo, fuera del paquete de fábrica. No se cambia el potencial, el
número de modos, la corrección de trabajo determinada ni las tolerancias.

## Cambios seleccionados

El potencial es par en el campo horizontal: su gradiente es proporcional a
ese campo. El contacto de este componente actúa sólo verticalmente. Si una
cuerda comienza un bloque con posición, velocidad y fuerza horizontal
**exactamente cero**, ese eje permanece nulo, incluida la corrección
`c=lambda Δq`. Se comprueba esta condición por cuerda en cada bloque de vuelo
certificado y se omiten únicamente sus incógnitas nulas del solver escalar.

La C4 inicialmente limpia necesita **289 incógnitas** en vez de 577. Se
conservan las **1153 coordenadas mecánicas**, los modos horizontales y su
estado. Si se excita ese eje, se vuelven a incluir sus incógnitas; no se
aplica un umbral de amplitud. `--full-transverse-solve` conserva el control
sin esta omisión. Esto sólo demuestra el invariante del componente a puente
fijo; debe revisarse al incorporar otras fuerzas o el cuerpo móvil.

La FFT real N/2 recibe directamente los datos en la permutación que necesita
su kernel. La división original de la inversa se realiza al recoger los
campos. Se eliminan dos recorridos separados, manteniendo la aritmética de
las mariposas y la división por la misma potencia de dos.
`--split-fft-passes` conserva el control anterior.

## Costo y controles

C4, intensidad 84, tres cuerdas 96/192, 256 micro-pasos a 128 kHz, bloque
no lineal de 16 pasos. Plazo **2 ms**. Ensayos consecutivos después de las
pruebas, sin otro benchmark simultáneo; el primer ataque está incluido.

| Ruta | Buffers | Media ms | P99 ms | Máximo/primer ataque ms |
| --- | ---: | ---: | ---: | ---: |
| [Seleccionada](medidas-P7-cuerdas-nativas-2026-10-13/invariant16-final.json) | 512 | 1,9194 | 2,3351 | 3,7864 |
| [Todas las incógnitas y FFT separada](medidas-P7-cuerdas-nativas-2026-10-13/invariant16-full-control.json) | 512 | 2,2090 | 2,8360 | 3,9179 |
| [Portable seleccionada](medidas-P7-cuerdas-nativas-2026-10-13/invariant16-portable.json) | 128 | 2,0826 | 2,7357 | 3,9280 |

Todas conservan **cero asignaciones** y máximo de cuatro iteraciones externas.
La media de la primera ruta queda por debajo del plazo, pero P99 y máximo
**siguen fallando**. No se aprueba tiempo real por su media.

[Tres repeticiones con orden alternado](medidas-P7-cuerdas-nativas-2026-10-13/invariant16-repeat-control.json)
incluyen también el ejecutable congelado de a547c6bb, para que el control no
dependa sólo de desactivar opciones en el nuevo binario:

| Ruta | Medias de las tres corridas, ms | Mediana de medias, ms |
| --- | --- | ---: |
| Congelada a547c6bb | 2,1311 / 2,1308 / 2,0851 | 2,1308 |
| Todas las incógnitas y FFT separada | 2,1993 / 2,1617 / 2,1605 | 2,1617 |
| Seleccionada | 1,9725 / 1,9443 / 1,9760 | 1,9725 |

La mediana de las medias baja **7,4%** frente al ejecutable congelado.
El P99 seleccionado varía entre **2,5917 y 2,9893 ms**, y el máximo/ataque
entre **3,8827 y 4,4845 ms**. No se demuestra mejora del ataque: la ganancia
se concentra en el vuelo de la cuerda inicialmente plana. La diferencia
entre la primera media y la medida histórica 2,2314 ms no sustituye estas
comparaciones repetidas.

## Equivalencia y límites

Pasan **21 pruebas** de cuerdas y presión. Los tres controles nuevos cubren
FFT fusionada/separada en SIMD y portable, conservación del invariante con
reiteración no alineada y un movimiento horizontal de **1e−50** que mantiene
sus incógnitas y su vibración. El control con ambos campos excitados conserva
las 577 incógnitas. Se mantienen Gauss espacial independiente, referencia
por micro-pasos, ledger, certificado de impacto futuro y controles de cache.

[250 ms con reiteración a 125 ms](medidas-P7-cuerdas-nativas-2026-10-13/invariant16-reiteracion250ms.json):
estado **0,3241655%**, ledger **2,6324e−12**, ambos pasan. El control corto
permanece en estado **0,1006955%**, fuerza **0,0004888%** y ledger
**1,4955e−14**. La referencia monolítica se reutiliza autenticada, con los
mismos coeficientes y eventos. Frente al solver sin eliminación axial, el
cambio del estado corto queda del orden de 3e−13.

El ensayo adicional con ambos ejes perturbados (seed 83), 250 ms y
reiteración a 125 ms reproduce **exactamente** posición, velocidad, fuerza
y calor del ejecutable congelado; ledger **4,3603e−12**, 577 incógnitas.
No se atribuye a ese caso la ganancia por el eje nulo.

El benchmark sostenido de ese mismo estado perturbado, que no ejecuta la
reiteración, **falla** la convergencia en ambas versiones: buffer 127,
micro-paso 112, tiempo **0,254875 s**. Se guardan ambos errores y un diagnóstico
con contexto, no un tiempo parcial aprobado. Ese fallo anterior del solver
requiere refinamiento del bloque al fallar, con restauración de estado,
cache y controles de trabajo, antes de ampliar la aprobación de duración.

## Hipótesis descartadas

- [Coeficientes por columnas](medidas-P7-cuerdas-nativas-2026-10-13/carrier-layout-rejected.json):
  misma trayectoria corta exacta, pero medias 2,4993 / 2,4187 / 2,4311 ms
  frente a 2,1220 / 2,1081 / 2,1033 ms. Se descarta por costo; la disposición
  contigua por sí sola no produjo la mejora esperada.
- [Cache de los términos lineales iniciales](medidas-P7-cuerdas-nativas-2026-10-13/carrier-start-cache-rejected.json),
  junto con FFT fusionada y reutilización de eta: trayectoria corta exacta,
  pero medias 2,2671 / 2,2863 / 2,1945 ms frente a 2,1795 / 2,1533 / 2,1452 ms.
  Se descarta esa combinación. Se conservan parches aplicables a a547c6bb y
  hashes; su reconstrucción reproduce el hash de cada fuente candidata.
- [Bloques de 24/28/32 pasos](medidas-P7-cuerdas-nativas-2026-10-13/block24-28-32-reiteracion250ms.json):
  estado a 250 ms **5,8074 / 6,5213 / 1,8304%**, todos fallan el límite 1%.
  Cerrar el trabajo no basta para aprobarlos. Se mantiene el bloque de 16.

Sigue la robustez del solver ante el fallo sostenido, y después el costo del
contacto y el cierre de puertos con inercia completa. Cuerpo móvil, presión,
pedales y polifonía todavía no están integrados en este coro. No hay render
de La Campanella, WAV nuevo, aprobación global ni actualización de fábrica.
