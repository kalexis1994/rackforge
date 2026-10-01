# Evaluación completa del sonido: modelo 14B y Steinway

30 de septiembre de 2026. Evaluación del candidato congelado 14B, programa
Concert Grand. Resultados y audios en **C:/SHARED/Evaluacion-completa-14B**.

## Qué encontramos

La diferencia cambia con la nota y la intensidad. En varios casos, el modelo
concentra energía en frecuencias altas mientras falta energía baja, aun cuando
el nivel total se aproxima al original. También persisten errores grandes en
el decaimiento de ciertos parciales. Por tanto, igualar volumen, aumentar todos
los graves o alargar todas las notas no corrige conjuntamente los casos medidos.

### 1. El pasaje señalado de La Campanella muestra el desequilibrio

Se comparó 2:26,7–2:28,4, conservando el tiempo de los MIDI y una sola ganancia
calibrada en notas aisladas. Respecto al Steinway reproducido por sfizz:

| Medida del pasaje | Modelo 14B menos referencia |
| --- | ---: |
| Energía total RMS | +0,85 dB |
| Banda 250–600 Hz | -10,07 dB |
| Banda 4–8 kHz | +27,27 dB |
| Banda 8–16 kHz | +30,32 dB |

La banda 4–8 kHz está a -41,24 dBFS en el modelo y -68,52 dBFS en la referencia.
Como comprobación, se filtró un contexto amplio antes de recortar el pasaje:
con bandas Butterworth de cuarto orden, 250–600 Hz da -9,79 dB y 4–8 kHz da
+31,58 dB. Cambia la ponderación temporal/frecuencial, pero se conserva el
desequilibrio. Los filtros sólo se usaron para medir; no se aplicaron al piano
ni a los audios publicados. Las cifras de la tabla proceden del espectro
ponderado por una ventana Hann; el RMS total se calcula sin esa ponderación.
Las diferencias se refieren a energía integrada del pasaje, no a amplitud de un
único pick ni a una medición de sonoridad perceptiva. Hay notas superpuestas,
captación y sala grabadas, muestras transpuestas y distintos algoritmos de pedal.
No se puede atribuir toda la diferencia a un mecanismo único del martillo o de
la caja. El patrón es compatible con la percepción de golpe fino y duro.

En el Nocturno, 3,7–4,4 s, la banda 4–8 kHz aumenta 3,61 dB, pero 8–16 kHz
disminuye 8,22 dB. El defecto del ataque de 00:04 tampoco puede describirse como
un exceso universal de toda la banda aguda: esta ventana incluye el sonido que
ya estaba presente y no separa el transitorio de pocos milisegundos.

![Pasajes MIDI](C:/SHARED/Evaluacion-completa-14B/pasajes-midi.png)

### 2. El cuerpo tonal sigue desequilibrado entre notas

Comparación directa con los WAV originales, sin sampler ni transposición,
velocidad MIDI representativa 124. Una sola ganancia para el conjunto.

| Nota | Nivel 50–200 ms | Nivel 0,5–1,5 s | Error en caída de H1 entre esas ventanas |
| --- | ---: | ---: | ---: |
| C4 | +1,90 dB | -2,65 dB | -15,71 dB |
| C5 | -3,88 dB | -1,94 dB | +4,25 dB |
| C6 | -0,47 dB | -3,72 dB | -0,39 dB |
| F#6 | +3,57 dB | +7,27 dB | +8,31 dB |
| C7 | +2,45 dB | +2,21 dB | +0,93 dB |

El error negativo de caída indica que el fundamental del modelo cae más; un
error positivo indica que cae menos. En C4, la caída del fundamental es casi
16 dB mayor que en el original aunque la pérdida de energía total del cuerpo
sea mucho menor. Los otros parciales pueden ocupar esa energía sin recuperar
el equilibrio tonal. F#6 presenta el problema opuesto en esta medida.

La mediana de energía inicial bajo 0,7 veces el fundamental, relativa al total,
es **10,70 dB menor en los agudos fuertes** del modelo. En C7/v124, la diferencia
es -14,45 dB. Se mide en 20–50 ms, con una máscara de fiabilidad de referencia.
Esa región incluye contribuciones estructurales, captación, sala y mecanismos;
no es una separación de la tabla armónica ni una identificación de su fuerza.
Las ventanas cortas tienen resolución limitada en graves.

![Parciales](C:/SHARED/Evaluacion-completa-14B/parciales-inicio-cuerpo.png)

### 3. La curva de intensidad necesita calibración por registro

Al comparar velocidades 28 y 124 en los WAV originales:

- C4: rango del original 18,96 dB; modelo 20,17 dB.
- C7: rango del original 24,48 dB; modelo 19,43 dB.
- A6: rango del original 27,05 dB; modelo 19,56 dB.

El error mediano del rango sobre las 30 raíces es -1,02 dB y el error absoluto
medio 2,32 dB. Los errores de notas concretas son mayores. Los números describen
la respuesta de esta biblioteca editada y su asignación de velocidades; no son
velocidades medidas del martillo ni presión acústica absoluta del piano real.

El sampler de las piezas añade 4 dB de extensión de dinámica y ganancia dentro
de cada capa. Comparándolo, el error mediano de rango es -4,13 dB. Separar ambas
referencias evita atribuir toda esa diferencia al modelo físico.

![Mapa original](C:/SHARED/Evaluacion-completa-14B/mapa-grabaciones-originales.png)

## Liberación, pedal, repeticiones y cola

Se realizaron **48 controles**: C4/F#4/C5/C6/F#6/C7, velocidades 60 y 124, y
cuatro condiciones: tecla sostenida, soltada a 0,8 s, sustain levantado a 2,5 s
tras soltar la tecla, y segundo ataque a 1 s. Las dos engines coinciden con su
propia condición sostenida antes del note-off en los 24 controles de prefijo.
Esto comprueba que las diferencias posteriores proceden de los eventos cambiados.

En C4/F#4/C5/C6, el modelo reduce la energía unos 6,6–11,9 dB en 50–200 ms después
del note-off respecto a su condición sostenida. El sampler reduce más en esos
casos, pero usa una envolvente elegida de 30 ms: **no es una medida de cómo
amortigua un Steinway real**. Las grabaciones no identifican de manera independiente
sus tiempos originales de note-off. En F#6/C7 el cambio de energía es pequeño,
coherente con las teclas sin apagador. Los aumentos relativos de bandas casi
vacías deben consultarse junto a su nivel absoluto, no interpretarse como ruido
fuerte por el cociente solo.

Las notas largas se mantienen 12 s en el modelo y se conservan 4 s después de
soltarlas. Las métricas de referencia quedan marcadas como no soportadas cuando
termina el archivo original. No se ajustará una pérdida de cuerda usando como
equivalentes una tecla sostenida y una grabación posiblemente ya amortiguada.

Se contaron mínimos de envolvente recuperados de al menos 3 dB, con suavizado y
límites de nivel, como diagnóstico adicional. Estos indicadores no separan
cuerdas, no constituyen una puntuación de rugosidad y dependen del piso de señal.
No bastan para decidir una nueva desafinación o introducir variación aleatoria.

## Cobertura y método

- **480 pares exactos:** 30 raíces desde A0 hasta C8, cada tercera menor, y
  las 16 capas grabadas. Velocidades representativas 4,12,...124.
- **12 sondas de interpolación:** F4/G4/G5/G6 a 60/84/124, mapeo al sample más
  cercano a +/-1 semitono. Están separadas de los resultados de raíces exactas.
- **48 controles** de liberación, pedal y repetición, y **dos MIDI completos**.
- Potencia de los dos canales promediada antes del logaritmo; evita cancelar
  muestras por sumar señales estéreo con fases opuestas. El estéreo se informa
  también por separado.
- Inicio acústico detectado a -40 dB del máximo inicial con resolución aproximada
  de 1 ms; se conservan sensibilidades a -50/-30 dB. Los pasajes MIDI conservan
  el tiempo original, sin alinear cada nota ni reescalar cada ataque.
- Ventanas de 0–20 / 20–50 / 50–200 / 200–500 ms y 0,5–1,5 / 1,5–3,5 /
  3,5–7,5 / 7,5–11,5 s. Espectros con ventanas de 512,2048,8192 muestras, usando
  máscaras de referencia y modelo para detectar tanto energía ausente como extra.
- Ganancias calibradas únicamente con los 14 pares de entrenamiento históricos:
  **32,84337 dB para WAV directos** y **26,40118 dB para reproducción SFZ**.
  Cada comparación conserva su ganancia única. La reproducción SFZ tiene una
  atenuación adicional observada aproximadamente constante en los controles;
  la calibración común la absorbe. Las dos condiciones no se mezclan en una puntuación.
- No se calcula un porcentaje de fidelidad. Todo reutiliza el corpus histórico;
  no se presenta como validación ciega independiente.

## Decisión y siguiente trabajo

La evaluación señala tres objetivos concretos: distribución de energía del
ataque, balance y pérdidas de parciales por registro, y respuesta dinámica.
El siguiente experimento debe registrar por separado la excitación del contacto,
el estado modal de las cuerdas y la transferencia puente/tabla/micrófono para
localizar dónde aparece cada diferencia. Después corresponde ajustar ese
subsistema junto con las pérdidas y la salida, manteniendo estas métricas y las
notas que pueden empeorar visibles. La evaluación actual no demuestra una causa
única ni selecciona un modelo nuevo.

## Archivos para consultar y escuchar

- `raw-recordings.json` y `grabaciones-originales-por-nota.csv`: comparación
  directa de grabaciones, sin transposición, curvas y diferencias por nota.
- `evaluation.json`, `per-note.csv`, `summary.json`: reproducción SFZ, espectros,
  envolventes, controles y sondas interpoladas. Los errores de bandas muy débiles
  son datos diagnósticos y deben filtrarse por fiabilidad antes de optimizar.
- `performance-comparison.json`: ambos MIDI, ventana por segundo y pasajes señalados.
- `verification.json`, `provenance.json`: comprobaciones y hashes.
- `Steinway-medios-evaluacion.wav` y `Modelo-14B-medios-evaluacion.wav`:
  C4/F4/G4/C5, cada nota a 60/84/124. Tres segundos sostenidos y 0,4 s de separación.
- `Steinway-agudos-evaluacion.wav` y `Modelo-14B-agudos-evaluacion.wav`:
  C5/G5/C6/G6/C7, mismo orden de intensidades y tiempos.
- `*-liberacion-pedal-repeticion.wav`: C4/C5/C6 a 124; por nota, liberación,
  pedal y repetición; cinco segundos por condición y 0,4 s de separación.

Los pares de audios usan una ganancia y un trim de seguridad comunes por conjunto,
sin normalización por nota ni ecualización/compresión. Un fade final de 15 ms
evita un clic al recortar los fragmentos. Las notas intermedias están transpuestas
como se detalla; el programa y los coeficientes del 14B siguen congelados.

## Créditos de las grabaciones

Biblioteca [The bitKlavier Grand](https://bitklavier.com/the-bitklavier-grand/),
Steinway D de Princeton, Taplin Auditorium, Fine Hall, enero de 2021. Grabación:
Andrés Villalta; piano: Matt Wang; edición y reducción de ruido: Jeff Gordon,
Katie Chou, Christien Ayers, Matt Wang y Dan Trueman; producción: Dan Trueman;
apoyo del Departamento de Música y Center for Digital Humanities de Princeton.

[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Adaptación: reproducción
MIDI/SFZ, selección de intensidad, transposición de notas intermedias, mezcla,
ganancia común y recorte/fade de audiciones. Conservar crédito y licencia al
compartir esos audios. No implica respaldo a RackForge. Las muestras editadas
y tratadas con reducción de ruido no constituyen una calibración de presión
acústica absoluta ni de fuerza del martillo.
