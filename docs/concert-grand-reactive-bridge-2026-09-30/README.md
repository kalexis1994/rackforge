# Puente reactivo y audición 10

Después de la preferencia del usuario por el 10, se ensayaron separación de
apoyos y patrones de afinación. Véase [la etapa del candidato 11](../concert-grand-tuning-pattern-2026-09-30/README.md).
Este documento conserva los resultados de la etapa 10.

## Resultado y estado

El usuario escuchó menos vaivén en la variante coherente 09, pero todavía lo
percibió. Se implementó y probó la respuesta reactiva del puente en un prototipo
aislado. La audición 10 permite escuchar ese cambio. No es un modelo aprobado
para sustituir al 07: recupera parte de la cola temprana respecto al 09, pero
conserva oscilaciones y en C5 pierde energía más adelante.

La investigación y los WAV están preservados. No se modificó ni instaló el
plugin de producción. Código de investigación:
`local/concert-grand-coherent-unison`, con snapshot portable en `research.patch`
respecto al plugin del worktree histórico `concert-grand-refit`.

## Cambio físico ensayado

Antes se usaba únicamente Re(Y), la conductancia de la misma tabla modal que
radia el sonido. Ahora se calcula también Im(Y), su susceptancia, usando las
mismas masas, pérdidas, frecuencias y proyecciones espaciales:

```
d = omega_mode² - omega²
Y = inverse_mass * projection² *
    [2*sigma*omega² + i*omega*d] / [d² + (2*sigma*omega)²]
gamma = T/L * Re(Y)
beta  = T/L * Im(Y)
```

Los estados complejos de las cuerdas rotan como `exp(-i*omega*t)`. La proyección
coherente del conjunto se multiplica por
`exp(-gamma*norm/fs + i*beta*norm/fs)`; los estados ortogonales no se modifican
por esta operación. La norma es el número de cuerdas activas más el cuadrado
de la proyección horizontal. Por tanto la parte reactiva cambia fase/frecuencia
sin introducir por sí sola pérdida, y la parte real mantiene la contracción.

Es una aproximación de acoplamiento débil, evaluada en una frecuencia congelada
por parcial. No incorpora memoria adicional del puente ni constituye una
resolución completa y recíproca de cuerda y tabla en el tiempo. Escalas reactivas
inferiores a uno se usaron solo para aislar la contribución; la audición 10 usa
la respuesta completa calculada por el banco existente.

La distinción entre modos colectivos, cambios de frecuencia y tasas de decaimiento
está ilustrada en los cálculos originales de [Woodhouse en Euphonics](https://euphonics.org/7-3-multiple-strings-and-double-decays/).
El acoplamiento puede producir batidos o una transición entre decaimientos;
incluirlo no garantiza eliminar todos los batidos.

## Exploración con muestras

Se reutilizaron C4, F#4, A4 y C5 a velocidades 60/84/124 de BitKlavier stage.
Se ensayaron seis respuestas reactivas y otras dieciocho combinaciones de
movilidad, sensibilidad de micrófono y ancho del unísono. No es validación ciega.

Para las doce parejas, entre 1,5 y 3,5 segundos:

| Perfil | Mínimos de envolvente | Nivel medio respecto al 07 |
|---|---:|---:|
| 07 | 70 | 0 dB |
| 09, coherente | 33 | −6,33 dB |
| 10, coherente + puente reactivo | 27 | −4,07 dB |

La recuperación de nivel es parcial. Reducir la movilidad puede devolver el
nivel tardío, pero cambia mucho el tramo intermedio. Por ejemplo, un candidato
con ancho 0,8, movilidad 0,00025 y sensibilidad 5,4 pasó las guardas exploratorias
de ataque/nivel tardío, con 67 mínimos y +0,08 dB tardíos, pero su tramo 0,5–1,5 s
subió 4,82 dB y la reducción de recurrencia fue pequeña. No se promovió.
Una guarda numérica limitada no equivale a fidelidad global del instrumento.

Los ajustes de sensibilidad fueron globales y explícitos, no normalizaciones
por nota. `results.csv` contiene toda la exploración; cada carpeta de resultados
conserva los hashes de ejecutable, modelo base y muestras originales.

## Comprobación con notas de 12 segundos

C4, F4, G4 y C5, velocidad 84, mantenidas doce segundos y liberadas con
velocidad 64; se renderizaron dos segundos más de release. F4/G4 amplían la prueba
del modelo y no se compararon con muestras originales transpuestas.

Mínimos por parcial, sumados sobre las cuatro notas:

| Perfil | 0,1–0,5 s | 0,5–1,5 s | 1,5–3,5 s | 3,5–7,5 s | 7,5–11,5 s |
|---|---:|---:|---:|---:|---:|
| 07 | 10 | 27 | 29 | 21 | 3 |
| 09 | 4 | 23 | 13 | 10 | 0 |
| 10 | 5 | 16 | 17 | 7 | 0 |

La prueba larga no demuestra una reducción global de recurrencia respecto al
09: cambia su distribución temporal. Entre 1,5–3,5 s, 10 recupera 2,01 dB de
nivel medio respecto al 09 en estas cuatro notas. Entre 7,5–11,5 s, C5 pierde
9,12 dB respecto al 09. No sería correcto concluir que toda la cola mejoró.

Se usaron los mismos parciales seleccionados desde el 07. Solo se cuentan
mínimos con recuperación de al menos 3 dB y por encima de los pisos definidos.
Las colas muy débiles pueden dejar de contribuir: cero mínimos al final no
prueba que el movimiento físico haya dejado de oscilar. Los recuentos tampoco
son un número de defectos audibles independientes.

## Audios

En `C:\SHARED` están `07/09/10-PUENTE-intensidades.wav` y
`07/09/10-PUENTE-sostenidas-12s.wav`, junto al TXT de orden y JSON de procedencia.
Todos usan el canal izquierdo, la misma ganancia y el mismo trim de −0,2721 dB.
Los archivos de intensidades contienen C4/F4/G4/C5 a velocidades 60/84/124,
cuatro segundos sostenidos por fragmento y 0,4 s de silencio.
Los archivos largos contienen las cuatro notas a velocidad 84, con release real.
Los fragmentos cortos no simulan note-off; no deben usarse para evaluar el release.

## Verificación y siguiente hipótesis

96 pruebas nativas aprobadas. Dos pruebas nuevas comparan la fase de la movilidad
con la respuesta temporal y verifican energía, frecuencia colectiva y ausencia
de cuerdas fantasma. La prueba especial de reiteraciones/carga compartida pasó
con la afinación 10, en 44,1/48/96 kHz y bloques 256/512. Los dos fingerprints
predeterminados se conservaron. Los seis WAV son finitos y tienen pico inferior
a 0,9. Estas pruebas no sustituyen la escucha ni prueban rendimiento ASIO en vivo.

El siguiente límite concreto a estudiar es la proyección única compartida por
las tres cuerdas. Trata sus puntos de apoyo como equivalentes y deja dos
configuraciones verticales ortogonales. Un modelo espacial de movilidad entre
los puntos de apoyo podría cambiar cómo se radian y amortiguan esos estados.
Es una hipótesis; aún no se implementó ni se demostró que explique este defecto.
Requiere una matriz de movilidad pasiva y observaciones de radiación consistentes,
seguida de un ajuste temporal conjunto y validación independiente.
