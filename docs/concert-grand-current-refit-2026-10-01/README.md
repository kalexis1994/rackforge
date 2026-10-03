# Refit del modelo audible actual: generalización y masas modales

## Resultado

No se aprueba una nueva calibración de fábrica. Los ajustes más fuertes de
densidad recuperaban cuerpo en las notas usadas para ajustar y empeoraban las
reservadas. Se descartan. La extensión de movilidad por frecuencia permite una
mejora moderada, pero su finalista tampoco cumple todas las comprobaciones
finales. Se conserva como experimento de escucha, con sus coeficientes
desactivados por defecto. No hay commit, paquete ni cambio de instalación.

| Prueba | Actual | Experimento regional |
| --- | ---: | ---: |
| Error conjunto medio, 42 pares de ajuste | 5,323 | 5,224 |
| Peor error conjunto, ajuste | 10,641 | 10,588 |
| Error conjunto medio, 63 pares reservados | 5,355 | 5,246 |
| Peor error conjunto, reservados | 10,627 | 10,633 |
| Mayor error de H1 tardío, reservados | 21,187 dB | 16,998 dB |
| Mayor error de bandas ataque/cuerpo, reservados | 13,294 dB | 14,057 dB |

La mejora media reservada es aproximadamente **2,04 %**. El peor error
conjunto queda prácticamente igual; el peor error de bandas empeora 0,76 dB.
El máximo empeoramiento de un par en la métrica conjunta es 0,761. El error
medio baja en los tres registros, pero eso no basta para declararlo una
corrección del timbre. C4 conserva un déficit grande de fundamental tardía:
recuperar unos 4 dB no elimina un déficit que todavía ronda 17 dB.

La métrica conjunta pondera errores medios de nivel de H1 (35 %), caída de
parciales (25 %), relaciones H2–H6/H1 (25 %) y bandas de ataque/cuerpo (15 %).
Está expresada como combinación de errores en dB, no como porcentaje de
fidelidad perceptiva. Los pesos y guardas son decisiones de ingeniería, no un
protocolo perceptivo validado ni un preregistro estadístico.

## Qué se cambió en el sistema de ajuste

Herramienta: `tools/fit-concert-grand-current-physics.py`.

Este informe conserva **score version 1**. Desde el diagnóstico del golpe
de C6 señalado en 00:57, la herramienta usa versión 2 por defecto, con un
término separado de cuerpo del impacto. Para repetir las ponderaciones de
este informe se debe usar `--score-version 1`. Véase
[diagnóstico del golpe](../concert-grand-impact-body-2026-10-01/README.md).

- Renderiza el modelo físico nativo real, con identidad SHA-256 explícita,
  preparación de 200 ms, bloques de 256 cuadros y ganancia de exportación
  unitaria. La ganancia de fábrica ya está dentro del motor.
- Evalúa por separado 0–20, 50–200, 200–500 y 500–1500 ms. Las tres ventanas
  sostenidas preceden al note-off; no confunde amortiguamiento de tecla con
  decaimiento mientras se mantiene pulsada.
- H1–H6 se miden individualmente. La máscara de fiabilidad depende únicamente
  de la referencia: no oculta un parcial por estar ausente del modelo. Excluye
  parciales 30 dB bajo el más fuerte inicialmente y colas 50 dB bajo él.
- Conserva todos los resultados por altura/velocidad, promedios por registro y
  máximos. La búsqueda penaliza regresiones de pares; las guardas finales
  condicionan la elección del perfil exportado como mejor.
- Las guardas exigen mejora del promedio, peor par, cada registro y peor H1
  tardío; toleran hasta 0,5 dB en máximos de caída, balance y bandas, y hasta
  2 unidades de error conjunto en un par. No sustituyen la escucha.
- Conserva parámetros, métricas, fuentes, capas, hashes de DLL/SFZ/mapeo y
  binario. Los cachés se invalidan si cambia el perfil, pares o renderer.
- Exporta secuencias iguales de intensidades y sus tiempos, sin normalización
  por nota ni EQ aplicado al audio.

## Etapas y referencias

El renderer inicial sin duplex es
`3b6ac0541fcd740b2cdab1586158b2e66790a6d367438f7ac91228970bb1d2b7`.
Se ensayaron conjuntamente densidad/pérdida de tabla, movilidad media,
conversión común de presión y pérdida adicional del unísono; luego contacto
con curvas suaves de rigidez, pérdida de velocidad y memoria del fieltro.

El ajuste inicial usa once alturas C2–C7 cada seis semitonos, a 60/110. Sus dos
finalistas se verifican en once alturas intermedias, a 36/84/124: 33 pares.
El error medio actual es 5,069; los finalistas suben a 6,838 y 6,232. Sus peores
errores pasan de 9,232 a 14,249 y 13,206. Es una regresión clara fuera del
conjunto de búsqueda; no se presentan como pianos mejorados.

Mover la densidad desplaza las resonancias y cambia sus relaciones de fase
y sus cargas. Los resultados son compatibles con un ajuste a alineaciones de
las raíces iniciales, sin demostrar una geometría real del Steinway. Por eso
el segundo experimento conserva todas las frecuencias y formas de la tabla.

Para el experimento regional se amplía el ajuste a las **21 raíces originales
C2–C7 cada tres semitonos**, a 60/110: 42 pares. Se reservan 28/92/124 sobre
esas mismas raíces: 63 pares. Las alturas intermedias verificadas previamente
ya forman parte de esta segunda búsqueda; no se llaman notas nuevas de una
prueba ciega. Las velocidades 28/92 no se usaron en las búsquedas iniciales.

Todas las referencias son el sampler SFZ histórico del bitKlavier Steinway D
Stage/Lid ORTF. Se verifica cada WAV por hash y cada capa por `v//8+1`. Todas
estas alturas son raíces grabadas, sin transposición. Las muestras originales
tienen edición y reducción de ruido; el mapeo SFZ contiene calibración de
niveles y release controlado. Se comprueba soporte de más de 1,6 s en el
archivo original. Este conjunto no identifica de manera única martillos,
masas, micrófonos ni geometría de tabla, y las velocidades MIDI no son medidas
de velocidad física del martillo. La validación es interna al mismo corpus.

## Extensión mecánica experimental

Renderer congelado:
`57a6c88d490e527f63ee9d9cfcdbb44a8bcd6b94f148a1c04a1fe7ca8fcb12e9`.

`bridge::regional_mobility` modifica la masa de cada modo con dos razones
positivas y suaves, en función de la **frecuencia del modo de tabla**, sin
parámetros por tecla. Tanto la respuesta a fuerza que radia como la admitancia
que carga las cuerdas usan exactamente esa masa. La calibración no desconecta
pérdida de radiación ni añade un ecualizador de salida.

Ege y Boutillon documentan regiones de placa y espacios entre costillas, con
movilidad dependiente de frecuencia ([descripción de movilidad](https://arxiv.org/abs/1210.5688),
[descripción global/local](https://arxiv.org/abs/1210.5109)). Eso motiva explorar
una media no constante. La interpolación elegida, y particularmente el cruce
grave de 200 Hz, son una parametrización reducida de ajuste; no implementan
las ecuaciones completas de esas costillas. No se atribuyen valores físicos
medidos a los parámetros ajustados desde presión acústica.

El experimento de escucha usa movilidad media 0,0004 m/(N s), razón grave
1,25 y aguda 1,5, conversión común de presión 3,375 y pérdida adicional del
unísono 3,5/s. La pérdida de tabla permanece 4,6 % y su densidad permanece en
el valor del programa Concert Grand. El contacto queda como en el modelo
aceptado. `DUPLEX_LEVEL`, `UNISON_WIDTH` y el spread de rigidez siguen en cero.

Los valores compilados de las dos razones son **1**. El helper retorna la
movilidad original en ese caso. C4/60 y F#6/110, cuatro segundos por par, son
idénticos bit por bit al renderer inicial; hashes en `mediciones.json`.

Pruebas: 125 nativas aprobadas, 75 diagnósticos ignorados; compilación WASM
SIMD aprobada y dos pruebas de equivalencia entre instancias aprobadas.
La prueba nueva verifica masas positivas, conductancia no negativa y la
misma escala de carga/respuesta a fuerza. No convierte la aproximación de
carga a frecuencia congelada en un solver transitorio recíproco completo.

## Escucha y reproducción

Carpeta: `C:/SHARED/Refit-global-2026-10-01`.

- `Experimento-movilidad-intensidades.wav`: sólo el experimento.
- `Actual-intensidades.wav`: la calibración aceptada sin duplex.
- `Steinway-intensidades.wav`: las mismas intensidades y raíces en el sampler.
- `Secuencia.json`: tiempos exactos. F#2, C3, C4, C5, C6, F#6; en cada altura
  28, 60, 92, 124. Cada golpe ocupa tres segundos: dos sostenidos, medio de
  release y medio de silencio. Total 72 s, estéreo PCM24 a 44,1 kHz.
- Las raíces del registro sin apagadores conservan su caída natural después
  del note-off; cada fragmento se recorta a 2,5 s con un fade final de 10 ms
  sólo para armar la secuencia de escucha.
- `Mediciones.json` y `Experimento.json`: resultados y coeficientes. El estado
  se marca `experimental_not_accepted`.

No se debe interpretar una preferencia por este experimento como prueba de
igualdad con el Steinway. El siguiente paso de modelado tiene que explicar
mejor la transferencia y el cuerpo sostenido, además de probar el acoplamiento
transitorio frente a la aproximación actual; esta búsqueda no identifica una
única causa física ni demuestra que ya se alcanzó un límite.

Ejemplo reproducible (numpy, scipy, soundfile y sfizz local):

```powershell
python tools/fit-concert-grand-current-physics.py --binary local/concert-grand-current-fit/regional-build/renderer.exe --expected-sha256 57a6c88d490e527f63ee9d9cfcdbb44a8bcd6b94f148a1c04a1fe7ca8fcb12e9 --work local/concert-grand-current-fit/replay --output local/concert-grand-current-fit/replay-report --profiles local/concert-grand-current-fit/regional-validation-profile.json --notes 36,39,42,45,48,51,54,57,60,63,66,69,72,75,78,81,84,87,90,93,96 --velocities 28,92,124 --score-version 1
```

Los archivos de audio de candidatos rechazados generados por esta tarea se
retiraron para recuperar espacio. Se conservan perfiles, features, fuentes,
manifiestos, logs y renderers; la herramienta los regenera cuando falta el
audio del caché. Los audios entregados, baselines y el experimento final se
conservan. Los bins/WAV están fuera de Git.
