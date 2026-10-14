# Campanella: los dos ataques cerca de 00:49

**Corrección posterior del usuario:** el audio al que se refería era
Nocturno. Este informe conserva un hallazgo independiente sobre Campanella;
no diagnostica el pasaje que el usuario había escuchado. El análisis correcto
del reporte está en [Nocturno: Fa5 repetidos](nocturne-attacks.md).

El usuario describe notas cortas con demasiado «tick» respecto del tono,
en particular una nota repetida alrededor de 00:49. Se analiza la versión
`1ce58df6` que acaba de escuchar. **No se modifica el DSP en este diagnóstico.**

## Localización y resultado principal

El par que encaja en ese entorno es **Do♯7, MIDI 97**:

| Inicio en el MIDI/render | Velocidad | Tecla abajo | Pedal |
| --- | ---: | ---: | ---: |
| 48,936198 s | 82 | 26,042 ms | 0 |
| 49,233073 s | 83 | 44,271 ms | 0 |

No son dos velocidades excepcionalmente bajas. El primer Do♯7 previo está en
48,342 s; el siguiente después del par aparece en 49,960 s junto con A#6.
Los tiempos proceden del MIDI original, no de una estimación auditiva.

**Para ese par, la evidencia apunta al tono que se agota muy rápido, más que
a un exceso del ruido mecánico añadido.** Quitar todo el mecanismo cambia
menos de 0,04 dB las primeras cuatro ventanas de nivel del pasaje. La cuerda
sigue produciendo un pulso tonal corto. Esa prueba no afirma que los ruidos
sean inaudibles ni que todas las otras notas descritas tengan la misma causa.

## Controles causales

Cada variante del pasaje se procesa **desde el comienzo hasta 52 s**, con
todos los eventos y controladores originales, una instancia continua,
44,1 kHz, bloques de 256, programa Concert y 200 ms de calentamiento. El
prefijo original es idéntico muestra por muestra al render completo escuchado.
No se comienza a sintetizar en 00:49 ni se cambian los números de golpe.

Se apagan por separado Action Noise, Release Noise, Impact Burst y los cuatro
controles de mecanismo/pedal mediante su posición real de apagado. La caída
del impulso metálico es prácticamente nula en este registro. Quitar Release
Noise tampoco devuelve cuerpo. El contraste de una variante con el original
es un efecto causal, no un stem estrictamente aditivo: la salida contiene
procesamiento no lineal.

RMS estéreo del pasaje, dBFS, tiempos relativos al Note On:

| Golpe / versión | 0–15 ms | 15–40 ms | 40–80 ms | 80–140 ms |
| --- | ---: | ---: | ---: | ---: |
| Primero, original | −24,05 | −25,04 | −31,48 | −41,11 |
| Primero, sin mecanismo | −24,06 | −25,04 | −31,48 | −41,11 |
| Segundo, original | −24,03 | −25,03 | −31,68 | −41,76 |
| Segundo, sin mecanismo | −24,02 | −25,05 | −31,70 | −41,80 |

La ventana posterior a 140 ms en la pieza contiene el acompañamiento
siguiente; no se usa como si fuera la cola aislada del Do♯7.

Los controles frescos aislados incluyen G#5/C7/C#7/D7 a velocidad 82,
C#7 a velocidades 60/110 y una comparación de C#7 con tecla abajo 26 y
500 ms. No son stems de la pieza: su historial y sus números de golpe son
distintos. **Con el mecanismo apagado, los primeros 500 ms de esos dos
gates de C#7 son idénticos byte por byte.** Es una nota por encima del último
apagador del programa. El Note Off temprano no corta el tono en este caso.
La captación aislada original cae aproximadamente 20 dB entre sus ventanas
15–40 y 80–140 ms, incluso con la tecla mantenida.

## Localización dentro del motor

Se usa el exportador de traza existente de la biblioteca actual, sin añadir
instrumentación al camino de audio. Su audio coincide con el consumidor
público independiente en los primeros 500 ms del C#7 mantenido. La traza
muestra el fundamental real, alrededor de **2224,08 Hz** con la afinación
estirada del modelo, y los cuatro estados verticales/horizontal existentes.

En C#7/v82, a partir de los polos y del drenaje realmente usados:

- T60 de la rotación individual, antes del drenaje colectivo: **2,59 s**.
- T60 del modo coherente inicial: **0,258 s**.
- Entre 5–20 y 150–200 ms, la norma conjunta cae **12,74 dB**, mientras
  la proyección del parcial cae **29,57 dB**. Se pierde movimiento y también
  eficacia de su suma audible. Las normas son internas, no energía en julios.

No se debe confundir el T60 del modo coherente con la duración de toda la nota.
El aftersound puede seguir existiendo a nivel muy bajo. C7 y D7 vecinos tienen
T60 coherentes de 0,259 y 0,214 s: el comportamiento abarca esta zona y no
corresponde a una única nota defectuosa.

La distinción entre pérdida intrínseca y pérdida del movimiento colectivo a
través del puente es relevante para la doble caída; véase
[Weinreich, cuerdas acopladas](https://www.speech.kth.se/music/5_lectures/weinreic/strings.html).
En el código actual, `t60_seconds` suma `viscoelastic_loss * bridge_speed_factor`,
mientras `slow_t60_seconds` usa sólo
`SLOW_WIRE_SHARE * viscoelastic_loss * slow_bridge_factor`. La diferencia entre
las tasas rápida y lenta se convierte después en drenaje colectivo del puente.
Así, una parte importante de lo etiquetado como pérdida del propio alambre
se está tratando como pérdida dependiente de la configuración colectiva.
Es un punto concreto a revisar; no demuestra que mover toda esa tasa a la
diagonal, o eliminarla, produzca automáticamente el piano correcto.

Dos controles adicionales, comunes a todas las notas y sólo diagnósticos:

| C#7/v82 | T60 individual | T60 coherente |
| --- | ---: | ---: |
| Modelo actual | 2,591 s | 0,258 s |
| `RADIATION_RATE = 0` | 7,285 s | 0,333 s |
| `STRING_HF_LOSS = 0` | 3,558 s | 1,078 s |

El segundo identifica una contribución grande del canal llamado
viscoelástico. **Apagar una pérdida física es una prueba causal, no una
corrección seleccionada.** Cambia también otras notas, parciales y colas;
no se propone conservar ese ajuste global a cero ni un arreglo local de C#7.

## Comparación conocida e historial de nuestras revisiones

Se recuperan las grabaciones originales conocidas de **C7 exacto**, capas
v8/v12 del corpus Steinway ORTF, con sus hashes originales. No hay una
grabación exacta de C#7 utilizada aquí y no se transpone C7 como si lo fuera.
Son archivos editados del corpus ya conocido, no una validación nueva/ciega.
La comparación es de envolvente relativa del fundamental, sin equiparar SPL.

Caída entre las ventanas 25–45 y 125–145 ms, alineadas sólo para análisis:

| C7 | Grabación conocida | Modelo actual |
| --- | ---: | ---: |
| Velocidad 60 / capa v8 | −9,88 dB | −18,77 dB |
| Velocidad 92 / capa v12 | −9,64 dB | −18,71 dB |

El déficit temprano del vecino C7 respalda investigar esta caída. No identifica
por sí solo una admitancia única del puente ni una tasa correcta para C#7.

También se verifican los WAV completos de cuatro revisiones. Para el primer
golpe, el nivel en 80–140 ms era **−37,32 dBFS** en la release y **−41,11**
ahora; para el segundo era **−37,74** y ahora **−41,76**. El ataque cambia
sólo unos −0,3 dB. **El acoplamiento normalizado agravó este desequilibrio**:
la separación ataque/cola aumenta unos 3,5–3,7 dB. La pequeña separación
de unísonos apenas cambia el pasaje y la revisión de tensión es idéntica a
la de acoplamiento en estas ventanas. El problema ya existía en la release,
pero la normalización dejó esta debilidad más expuesta. Esto limita la
aceptación global anterior; no se oculta la regresión específica.

## Audios y siguiente corrección que merece probarse

`C:/SHARED/Piano-Ticks-Campanella-2026-10-14/` conserva los recortes
46,8–51,2 s originales, las variantes sin cada mecanismo y los controles de
pérdidas. Los archivos `Doble-Cs7-*.wav` repiten **tres veces** el recorte
48,75–49,40 s, con 350 ms de silencio entre repeticiones. Se conservan todas
sus muestras y ganancia originales; no se normaliza cada opción.

La siguiente hipótesis justificada es **separar consistentemente las pérdidas
propias de cada cuerda de la disipación colectiva por el puente y revisar
sus tasas**, buscando una cola tonal audible sin alargar indiscriminadamente
las notas. Bajar ruidos mecánicos por sí solo no aborda lo que muestran
estos dos golpes. Este turno deja el diagnóstico y los controles; no adopta
una fórmula nueva sin comparar sus efectos en el resto del teclado.

- [Ablaciones, eventos, métricas y hashes](short-attacks.json).
- [Traza, pérdidas, gate y referencia C7](short-decay.json).
- [Cuatro revisiones, mismas muestras y ventanas](short-attacks-history.json).
- [Recortes breves para escuchar](short-attacks-listening.json).
- Herramientas: `tools/analyze-concert-grand-short-attacks.py` y
  `tools/analyze-concert-grand-short-decay.py`.
