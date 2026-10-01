# Unísonos y vibración recurrente — 30 de septiembre de 2026

La respuesta reactiva mencionada al final de esta etapa se implementó después
en el prototipo. Véase [la continuación y audición 10](../concert-grand-reactive-bridge-2026-09-30/README.md).
Este documento conserva los resultados de la etapa inicial.

## Decisión

Se conserva el modelo 07. Ninguna de las once variantes de afinación/rigidez
ensayadas mejora la recurrencia sin deteriorar el nivel de la cola. Las dos
ablaciones del puente son diagnósticos y no candidatos instalables.

## Qué se implementó

Prototipo con una afinación y una rigidez efectivas por cuerda, constantes a
lo largo de sus parciales. La afinación conserva el centro geométrico del
unísono; cada cuerda sigue su propia ley de inarmonicidad. Esto sustituye,
cuando se activa, las desviaciones de afinación sorteadas por parcial.
Los parámetros no son propiedades físicas identificadas del Steinway.

El prototipo está aislado en `local/concert-grand-coherent-unison`. El plugin
del proyecto principal no fue modificado. `research.patch` conserva el cambio
respecto al plugin del worktree histórico `concert-grand-refit`; no debe
aplicarse a otra revisión suponiendo que son equivalentes.

## Evidencia

Muestras originales, sin transposición: C4, F#4, A4 y C5, velocidades MIDI
60, 84 y 124, micrófonos stage del BitKlavier. Se reutilizó este corpus para
exploración; no constituye validación independiente. No extrapolar los
resultados a F4/G4 ni a todo el teclado.

Se contaron mínimos de envolvente por parcial que recuperan al menos 3 dB,
entre 1,5 y 3,5 segundos. Todos los candidatos usan los mismos parciales
seleccionados a partir de la referencia. Una caída monótona no se cuenta.
Estos mínimos son indicadores, no un número de defectos audibles independientes.

| Configuración | Mínimos tardíos | Cambio medio de nivel tardío |
|---|---:|---:|
| Modelo 07 | 70 | 0 dB |
| Coherente, ancho 0,5, dispersión B 0,02 | 33 | −6,33 dB |
| Coherente, ancho 1,0, dispersión B 0,02 | 50 | −2,40 dB |
| Coherente, ancho 1,5, dispersión B 0,02 | 101 | +0,39 dB |
| Coherente, ancho 2,0, dispersión B 0,08 | 155 | +2,41 dB |
| Modelo 07 sin amortiguación del puente | 86 | +10,10 dB |
| Coherente 0,5/0,02 sin amortiguación del puente | 50 | +11,86 dB |

El nivel usa la misma ganancia fija que el modelo 07; no se normalizó cada
variante para ocultar pérdidas. Los resultados completos están en `results.csv`.
Los JSON de procedencia contienen hashes de ejecutable, modelo y referencias.
Las medidas detalladas y WAV están bajo las carpetas `experiment*` del prototipo.

Quitar amortiguación manteniendo la radiación no es una configuración física
consistente: solo aísla una contribución del algoritmo. La comparación tampoco
mantiene igual sonoridad, y el umbral absoluto de detección puede verse afectado.
Permite descartar esa ablación como arreglo, no identificar una causa única.

## Qué aprendimos y qué falta

Cambiar solamente la separación de frecuencias no ha solucionado el equilibrio
entre oscilación y cuerpo. Tampoco funciona simplemente quitar la pérdida del
puente. No se ha demostrado que falte una única pieza física ni que el modelo
haya llegado a un límite.

La implementación actual acopla los estados de las cuerdas mediante una
proyección disipativa común. La tasa se obtiene de la conductancia del puente.
La próxima hipótesis a contrastar es la respuesta completa de ese acoplamiento:
su componente reactiva y cómo modifica frecuencias, fases y modos colectivos,
además de la energía que se radia. Esto requiere una derivación y validación de
pasividad antes de introducirlo en el motor, y un refit conjunto de afinación,
pérdidas y respuesta de la tabla. No está implementado ni probado todavía.

Un candidato futuro deberá conservar ataque y energía tardía, reducir la
recurrencia y comprobarse con notas sostenidas de 10–12 segundos para evitar
desplazar el problema fuera de la ventana de medida. Después: notas y velocidades
no usadas en el ajuste, liberación, pedal, reiteraciones y piezas MIDI.

## Verificación

94 pruebas nativas aprobadas; 74 pruebas especiales ignoradas por la ejecución
normal. Las tres nuevas comprueban afinación coherente, ley de rigidez y perfiles
finitos/repetibles para las 88 teclas. Los fingerprints predeterminados se
conservaron: `243e622a8b59910d` y `706b728c4d0a6b06` con pedales.
Esto verifica integridad del prototipo, no fidelidad perceptiva ni rendimiento
ASIO en el equipo del usuario. No se distribuyó ni instaló un plugin nuevo.

El entorno de esta sesión permite escribir en el proyecto principal, pero no
en el worktree histórico ni en `C:\SHARED`; por eso estos resultados están aquí.
