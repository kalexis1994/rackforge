# Nocturno: ataque seco de los Fa5 repetidos cerca de 00:49

**Corrección del usuario:** el audio de su informe era Nocturno, no
Campanella. Al escuchar el recorte 47,5–51,6 s, confirma «En la melodía».
La investigación anterior de C#7 en Campanella queda como un hallazgo
independiente; no es el diagnóstico de este problema de escucha.

Se mantiene el motor de `1ce58df6`, con fuente y ejecutables autenticados.
Este trabajo añade herramientas y observadores en copias locales, sin cambiar
la síntesis de producción ni sus controles predeterminados.

## Notas identificadas

El par repetido de la melodía es **Fa5, MIDI 77**:

| Inicio | Velocidad | Tecla abajo | CC64 al inicio / final |
| --- | ---: | ---: | ---: |
| 49,748698 s | 64 | 251,302 ms | 127 / 127 |
| 50,191406 s | 66 | 1384,115 ms | 91 / 122 |

El segundo comienza con pedal 91, no 127; el pedal cambia durante la nota.
La traza muestra ambos golpes reutilizando la misma voz, libre en ese
momento. No se confunde la corta percepción del sonido con la duración MIDI:
el segundo Fa5 permanece presionado más de un segundo. La melodía previa es
G5 en 48,833333 s. Cerca del primer Fa5 también entran F4 y A#3, por lo que
el nivel global de una ventana no representa una única fuente aislada.

## Qué establecen las pruebas

Se procesa el MIDI original desde el comienzo hasta 53 s, una instancia
continua, 44,1 kHz, bloques de 256, programa Concert y calentamiento silencioso
de 200 ms. El render original reproduce **todas las muestras** del prefijo
del Nocturno completo que recibió el usuario. Se comparan apagados separados
de Action Noise, Release Noise, Impact Burst y todos los ruidos mecánicos.

Al apagar Action Noise, el RMS global de los primeros 12 ms cambia **+0,09
y +0,26 dB** en los dos golpes; apagar Release Noise cambia **+0,02 y +0,09
dB**. El impulso metálico tiene un efecto mucho menor. El signo positivo
es posible porque hay suma de señales y realimentación: no son stems aditivos.
En 3–10 kHz, la diferencia causada por Action Noise queda unos 18–20 dB por
debajo de la señal original en esos 12 ms. **Estas cifras no demuestran que
el ruido sea inaudible**, pero no muestran que apagarlo elimine la mayor
parte del ataque de este pasaje.

Para observar la cuerda sin sustituir el motor se compila una copia del
fuente con dos observadores exclusivamente de prueba:

- Estados de las voces al final de cada bloque.
- Salida mono y fuerza del Fa5 antes de la tabla, muestra por muestra.

Ambos observadores reproducen los **53 s completos idénticos byte por byte**
al consumidor público de producción. La copia mantiene el orden de cálculo;
no se vuelve a hacer avanzar una voz para observarla. Las columnas de fuerza
tienen unidades internas y no se presentan como presión acústica calibrada.

Los dos golpes activan el fundamental, alrededor de **698,47 Hz**, en la misma
voz. Su proyección interna cae unos **5–6 dB en los primeros 140–150 ms**,
no desaparece después del ataque. Los polos reales dan un T60 individual
de aproximadamente **8,54 s** y un T60 coherente de **1,48 s**. No se infiere
de ello la duración de la señal final ni su calidad perceptual.

Este comportamiento es distinto al T60 coherente de 0,258 s del C#7 de
Campanella. **No queda justificado atribuir al Fa5 la misma pérdida extrema
ni aplicar aquí la prueba de apagar pérdidas como si ya fuese una solución.**

## Repetición: impulso de una muestra y controles de contacto

La versión actual combina `RESTRIKE_FRESH = 0`, `MERGE_RAMP_S = 0` y
`MERGE_RAMP_CONTACT = 0`. En la repetición se conserva el desplazamiento
existente y se añade la nueva contribución a la cuadratura de velocidad en
**una muestra**, unos 22,7 microsegundos. El modelo tiene maquinaria para
repartir esa contribución durante un contacto, pero está apagada por defecto.
El propio comentario de `MERGE_RAMP_S` registra un ensayo anterior donde el
reparto aumentó el mayor salto de muestras; no se ignora ese antecedente.

Se comparan tres variantes usando los controles ya existentes, sin cambiar
fuente: original, contacto temporal del modelo (`MERGE_RAMP_CONTACT = 1`)
y contacto fijo de 4 ms (`MERGE_RAMP_S = 0.004`). Para medir la contribución
de cada golpe, se vuelve a renderizar cada variante omitiendo sólo ese Note On.
El prefijo anterior al evento debe permanecer idéntico. Los primeros 12 ms
preceden a la nota siguiente, evitando atribuir diferencias posteriores de
número de golpe a este ataque. La realimentación posterior sigue incluida.

| Contribución del golpe, primeros 12 ms | Primero | Segundo |
| --- | ---: | ---: |
| Original, RMS dBFS | −38,356 | −37,760 |
| Contacto temporal del modelo | −38,414 | −37,818 |
| Contacto fijo de 4 ms | −38,785 | −38,192 |

El pico de la contribución pasa de **0,03084 a 0,02889** en el primer golpe
y de **0,03294 a 0,03087** en el segundo al repartirla en 4 ms. El mayor
salto entre muestras cambia poco y no empeora cuatro veces en este pasaje:
0,003453 → 0,003445 y 0,003765 → 0,003745. Ese indicador también contiene
la pendiente normal de una sinusoide; no es una medida exclusiva de clicks.

**Resultado modesto:** alrededor de −0,43 dB en los primeros 12 ms de la
contribución. El impulso instantáneo es un mecanismo concreto a investigar,
pero estas cifras no lo confirman como explicación suficiente del «tick» ni
demuestran que 4 ms resuelva la escucha. No se recentra el control.

También se conserva un control con la ruta existente de voz nueva
(`RESTRIKE_FRESH = 1`), sólo diagnóstico. Modifica asignación de voces,
historial y colas, además de la repetición; no se adopta como modelo físico
correcto ni se presenta su diferencia como aislamiento de una sola causa.

## Audios y alcance de la conclusión

Carpeta `C:/SHARED/Piano-Ticks-Nocturno-2026-10-14/`:

- `Nocturno-47.5-51.6-original.wav` y variantes sin cada ruido: contexto usado
  para que el usuario identifique la melodía.
- `Melodia-49.55-50.55-original.wav`.
- `Melodia-49.55-50.55-contacto_modelo.wav`.
- `Melodia-49.55-50.55-contacto_4ms.wav`.
- `Melodia-49.55-50.55-voz-nueva.wav`, intervención diagnóstica adicional.

Los últimos cuatro repiten tres veces el mismo segundo, con 350 ms de
silencio entre repeticiones, manteniendo la ganancia original. Son WAV
float32 estéreo, muestras idénticas a sus crudos, sin normalizar cada opción.

La localización y la continuidad del tono quedan verificadas. Falta confirmar
perceptualmente qué parte de la repetición y de su proyección acústica produce
la sensación desproporcionada; no se declara resuelta. No se atribuye este
reporte a Campanella ni se cambia el motor sobre esa identificación anterior.

Datos y reproducción:

- [MIDI, ablaciones y recortes](nocturne-attacks.json):
  `tools/analyze-concert-grand-nocturne-attacks.py`.
- [Estados en contexto](nocturne-context.json):
  `tools/trace-concert-grand-nocturne-context.py`.
- [Fuerza real de Fa5, sin alterar la salida](nocturne-melody.json):
  `tools/trace-concert-grand-nocturne-melody.py`.
- [Contactos y contribución de cada repetición](nocturne-restrike.json):
  `tools/compare-concert-grand-nocturne-restrike.py`.
- [Control de voz nueva](nocturne-fresh-control.json).
