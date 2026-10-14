# Acoplamiento común: normalización y cuerdas presentes

Se retira por pedido del usuario la calibración local del fundamental en MIDI
58–62, commit `0b7cf7f5`. La base A conserva únicamente el pequeño residuo
agudo previamente aceptado. La corrección B siguiente pertenece al
acoplamiento común y no contiene una selección de notas o de fundamentales.

## Inconsistencia encontrada y corrección

El reparto del golpe entre cuerdas, `shares`, es una condición de excitación.
Antes se usaban sus factores de desequilibrio para normalizar el drenaje,
pero la reacción se repartía con pesos verticales unitarios, siempre en tres
coordenadas. La tasa de caída coherente dependía así del desequilibrio del
golpe. En el grave también aparecían estados momentáneos en coordenadas de
cuerdas que no existen, aunque sus polos nulos volvían a borrarlos al rotar.

Ahora la proyección y reacción usan el mismo puerto mecánico:

`w = (a0, a1, a2, h)`, con `ai` igual a uno para una cuerda presente y a cero
para una ausente. `h` conserva el acoplamiento horizontal del modelo. La
normalización es `n = w·w`; las amplitudes del golpe no intervienen.
Si la retención coherente prevista en 256 muestras es `r`, se usa

`C = I − [(1 − r^(1/256)) / n] w wᵀ`.

La dirección coherente conserva `r^(1/256)` por muestra; las ortogonales
conservan su norma durante esta operación. La rotación/pérdida intrínseca
sigue siendo la etapa previa. Las coordenadas ausentes permanecen en cero.
Una coordenada acoplada momentáneamente silenciosa conserva sus polos:
puede recibir movimiento de sus vecinas. Se retira el parcial completo
cuando su norma conjunta cae bajo el umbral. La repetición copia el puerto
mecánico junto con el drenaje nuevo.

Esto corrige la consistencia del acoplamiento modal existente. Sus normas
cuadráticas tienen unidades internas: no se afirma un balance en julios del
instrumento completo ni una resolución recíproca completa de cuerdas y tabla.
La distinción entre configuraciones colectivas que radian y que conservan
movimiento está descrita por [Weinreich](https://www.speech.kth.se/music/5_lectures/weinreic/strings.html)
y por [Woodhouse](https://euphonics.org/7-3-multiple-strings-and-double-decays/).
Esos mecanismos explican por qué más movimiento almacenado no garantiza más
presión audible; las cifras siguientes son nuestros ensayos del motor.

## Resultado acústico: mixto, pendiente de escucha

Se renderizan **las 88 teclas a velocidades 60 y 110**, 176 pares A/B, una
instancia nueva por nota, tecla mantenida dos segundos y salida de cuatro.
La captación y exportación son las mismas; no se ajusta nivel por nota.

Caída de H1 entre las ventanas 100–200 y 500–1400 ms, velocidad 60:

| Nota | A: base sin ajuste local | B: puerto normalizado | Referencia conocida |
| --- | ---: | ---: | ---: |
| C3 | −16,46 | −15,93 | −10,80 |
| A3 | −3,76 | −3,83 | −19,46 |
| C4 | −16,09 | −14,31 | −5,78 |
| D#4 | −22,20 | −23,33 | −16,87 |
| C5 | −14,01 | −14,01 | −13,20 |
| C6 | −19,76 | −20,12 | −16,85 |

En los doce pares con referencia conocida, el error absoluto medio de caída
de H1 pasa de **6,623 a 6,475 dB**. H2 pasa de **3,372 a 3,484 dB** y H3 de
**7,802 a 7,859 dB**. El error conjunto de H1–H3 prácticamente no mejora.
Por ello **no se declara conseguido mejorar cuerpo y sostenido en todo el
teclado**. La referencia es el mismo corpus conocido, no una validación ciega.

Las medianas de cambio de nivel de las 176 notas son próximas a cero. Los
ataques de C#7/E7/A#7/C8 cambian más de 1 dB, con máximo absoluto de **1,79
dB**. En 1,5–1,9 s, D#6/v60 cae de −79,53 a −85,56 dBFS RMS, una diferencia
de −6,03 dB; no se oculta ese peor caso por estar bajo. Hay incrementos grandes
en otras colas cercanas al silencio que no deben interpretarse como mejoras
perceptuales de ese tamaño. Diez alturas adicionales se mantienen doce
segundos y se liberan durante dos, para observar también la respuesta tardía.

## Verificación y costo

- **81 pruebas habituales aprobadas**, ninguna fallida; 69 ensayos especiales
  ignorados por la ejecución normal. Las dos huellas se ejecutan además por
  separado y pasan: `581f8087da14bcbf` / `3f40537e871fc7c3`.
- Tres pruebas nuevas verifican la matriz de acoplamiento contra un cálculo
  f64 independiente, su contracción, su tasa coherente, ausencia de cuerdas
  fantasma y conservación de coordenadas silenciosas todavía acopladas.
- Compilación WASM con SIMD128 y pila de 8 MiB aprobada. No se ejecuta aquí
  una prueba de arranque, combustible o reparto entre instancias WASM.
- Guion nativo de 14 s, tres ejecuciones alternadas por versión: medianas
  **0,70285 s en A** y **0,70386 s en B**, diferencia **+0,14%**, dentro de la
  variación de las ejecuciones. Incluye escritura cruda. La corrección añade
  pesos y aritmética de reacción; no se afirma costo exactamente nulo.

## Audios completos

Carpeta: `C:/SHARED/Piano-Acoplamiento-general-2026-10-14/`.

- `Campanella-B-acoplamiento-general.wav`: 280,159 s; render B **26,89 s**.
  A es el WAV del unísono aceptado en
  `C:/SHARED/Piano-Campanella-unisonos-2026-10-14/escucha/Campanella-B-unisonos-agudos.wav`.
- `Nocturno-A-base-sin-ajuste-local.wav` y `Nocturno-B-acoplamiento-general.wav`:
  232,564 s, misma ganancia; render B **25,92 s**. A se vuelve a renderizar
  para excluir expresamente la antigua corrección local del Do central.
- `Notas-A-base-sin-ajuste-local.wav` y `Notas-B-acoplamiento-general.wav`:
  C2/C3/A3/C4/D#4/C5/C6/C7, primero a velocidad 60 y después a 110.

Las piezas se procesan enteras desde el principio en una instancia continua,
con tempos, velocidades y controladores originales. Sin normalización, EQ,
notas muestreadas ni eventos finales artificiales. Ganancia de exportación 1;
WAV float32 estéreo de 44,1 kHz, muestras idénticas al crudo, finitas y bajo
el fondo de escala. Se conserva el pedal final de cada MIDI.

[Datos, hashes y comandos](general-coupling.json).
[Pruebas y compilación](general-coupling-verification.json).
Reproducción: `tools/audit-concert-grand-general-coupling.py`.

La rama contiene esta corrección como candidato de escucha y la retirada
local ya está comprometida por separado. No se publica ni instala un paquete.
Sigue pendiente conseguir una mejora acústica general; corregir la
normalización por sí sola no la demuestra.
