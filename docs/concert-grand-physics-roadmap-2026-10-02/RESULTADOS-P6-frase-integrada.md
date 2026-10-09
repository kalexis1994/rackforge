# P6 — Integración de contacto acelerado en la frase compartida

Fecha: 2026-10-09. Etapa anterior guardada en `3580fc7`.

## Objetivo y alcance

Integrar el contacto de portadoras a 128 kHz con sostenido, reiteración y
apagadores en la misma trayectoria viva. Se compara el recorrido completo
de dos segundos con las referencias escuchadas de C4–G4–C5, a velocidad 84,
con y sin pedal. Las referencias tienen coast a 16 kHz y contacto midpoint
a 768 kHz. Esta comparación mide reproducción de un control temporal
grueso; no certifica convergencia hacia la solución continua.

Se mantienen nueve cuerdas 96/192, una tabla de 606 coordenadas y un aire
reducido de rango 192. Ganancia fija 0,25/Pa, observación a 128 kHz, WAV
float32 estéreo a 48 kHz y 100 ms de silencio inicial. No hay muestras,
EQ, normalización por nota, fades ni envolvente de cola. El gate acústico
heredado sigue fallando: 9,202 % frente al límite del 5 %.

## Integración

Los eventos conservan el orden y los tiempos del productor congelado:
C4 0–0,14 s, G4 0,25–0,39 s, C5 0,50–0,64 s, reiteración C4
0,75–0,89 s y subida del pedal a 1,05 s. El reloj usa enteros a 128 kHz;
se impide que un paso grueso cruce un evento. Al lanzar una cabeza, los
estados de cuerdas, tabla, onda y relajación se comprueban sin reinicio.

Coast trabaja a 16 kHz para conservar el control anterior. Si detecta un
recontacto, se rechaza ese intento y se recalculan ambas mitades del
apagador desde el estado anterior con paso fino. No se suma pérdida de
un intento rechazado. El controlador inicia los apagadores libres cerrados
y una subida de pedal redundante conserva su estado.

## Hipótesis de coste: contracción exacta del aire

El perfil del sostenido vivo encuentra dos costes comparables: en 32 pasos,
mecánica **0,580 s** y aire **0,542 s**, dentro de 1,139 s perfilados.
La suma de residuos consume 0,353 s; proyecciones/inversas 0,278 s y
AVF no lineal 0,280 s. Son tiempos inclusivos que pueden solaparse:
no sumar todos esos subcostes como partes independientes.

En el operador reducido, la suma `sum_k R[k] @ offsets[k]` se organiza
como un único GEMV con una matriz contigua. Se retienen todos los lags,
modos, constantes, estados y pérdidas. El layout ocupa **191.397.888 bytes
(182,53 MiB)** adicionales en RAM, compartidos por contacto y coast;
no se escribe otro R ni caché al disco. Hay un límite explícito de 512 MiB
para evitar empaquetar automáticamente el operador completo mucho mayor.

Se reutiliza la cápsula autenticada de 40 ms, de unos 2 MiB, como condición
inicial emparejada. Su historia no está declarada convergida. Se comprueban
identidad de tabla, R y base de reducción antes de usarla. El perfil se
calcula aparte de las mediciones de velocidad para excluir su sobrecoste.

| Ensayo local | Original | GEMV contiguo | Cociente | Error presión | Máximo error de arrays |
| --- | ---: | ---: | ---: | ---: | ---: |
| Coast 8 ms, 16 kHz | 4,447 s | 4,078 s | 1,0903× | 1,95e-14 | 6,15e-15 |
| Coast 2 ms, 64 kHz | 4,181 s | 3,830 s | 1,0916× | 3,45e-14 | 1,07e-14 |
| Contacto 2 ms, 128 kHz | 8,542 s | 8,032 s | 1,0635× | 5,30e-13 | 9,63e-14 |

Todos pasan equivalencia bajo 1e-8. No se usa esta mejora local del 9 %
como estimación de la aceleración completa; el cambio de método de contacto
y el coste de apagadores también intervienen en la frase.

## Pruebas

Cuatro pruebas nuevas pasan: suma completa/actualización Padé para tres
pasos y estados no nulos; dinámica y pérdidas de contacto/coast; frase viva
con reiteración/pedal y un evento entre ticks gruesos; detección por bandas
de un aumento conocido del 10 % a 1 kHz sin cambio de ganancia global.
Se verifican presupuesto, identidad del operador y ausencia de mutación de
estados. Las dos pruebas del observador anterior pasan antes del commit.

## Comparación completa

Ambos controles completados en **1994,381 s (33,24 min)**, incluida
construcción, ensayos de equivalencia, perfiles y salida acústica.
Medidas y perfiles se conservan una sola vez en
[el informe](medidas-P6-frase-integrada-2026-10-09/frase-integrada.json).

El error global usa los WAVs float32 a la misma ganancia y duración. Se
comparan bandas 0–60, 60–800, 800–2500, 2500–6000, 6000–12000 y
12000–24000 Hz, con pesos de Parseval. Para cada banda se guarda error
relativo y contribución absoluta al error total; no ocultar una banda de
poca energía detrás de la norma global. El control de reproducción pide
1 % global y 1 % en cada banda de 60–12000 Hz, sin aprobar la física completa.

### Control con pedal

Trayectoria completa **947,540 s (15,79 min)**, cociente de coste **1,8451×**
frente a la referencia anterior. Cuatro lanzamientos, 800 pasos finos de
contacto y 31.900 pasos de coast; ningún reintento por recontacto. Esto ya
es tiempo del recorrido completo, no extrapolación de un ataque local.

Error global de WAV **0,036646 %**: pasa 1 %. Picos 0,331672 y RMS final
5,328e-8. Movimiento simpático G4 antes de su propio golpe: norma modal
intrínseca 27,966 µm, no desplazamiento puntual de la cuerda.

| Banda | Fracción de energía de referencia | Error relativo de banda | Error absoluto respecto al total |
| --- | ---: | ---: | ---: |
| 60–800 Hz | 97,5602 % | 0,013680 % | 0,013512 % |
| 800–2500 Hz | 2,43601 % | 0,212836 % | 0,033219 % |
| 2500–6000 Hz | 0,00122636 % | **2,047124 %** | 0,007169 % |
| 6000–12000 Hz | 0,00000173346 % | **10,108351 %** | 0,001331 % |

**Falla el criterio por bandas**; no se cambia el umbral por su baja energía.
La contribución absoluta evita confundir ese 10 % con una diferencia del
10 % del piano completo. La contracción exacta pasa equivalencia local
con presión bajo 5,31e-13; no hay evidencia de que explique este cambio.
La nueva integración de contacto y su calendario de separación/coast
necesitan un contraste más fino. La dispersión del midpoint de referencia
a 768 kHz es una hipótesis, no una causa identificada por este ensayo.

WAV disponible como diagnóstico en
`C:/SHARED/Piano-contacto-integrado-2026-10-09/Frase-integrada-pedal.wav`.
Verificado independientemente: 100.967 frames, 48 kHz, dos canales float32,
finitud, sin clipping y SHA256 igual al informe. El control aceptado sigue
siendo la referencia; no se selecciona una nueva calibración de fábrica.

### Control sin pedal y cierre

Trayectoria completa **913,275 s (15,22 min)** frente a 1672,597 s
(27,88 min) de la referencia corregida: **1,8314×**. Cuatro lanzamientos,
808 pasos finos y 31.899 pasos de coast, sin reintentos por recontacto.
Los conteos de ambos controles reproducen exactamente los 256.000 ticks
de dos segundos; los lanzamientos ocurren a 0 / 0,25 / 0,50 / 0,75 s.

Error global **0,026404 %**: pasa 1 %. Pico 0,268777 y RMS final 4,302e-8.
La norma modal intrínseca simpática de G4 es 4,176 µm antes de su golpe.

| Banda | Fracción de energía de referencia | Error relativo de banda | Error absoluto respecto al total |
| --- | ---: | ---: | ---: |
| 60–800 Hz | 97,4059 % | 0,008373 % | 0,008264 % |
| 800–2500 Hz | 2,58822 % | 0,141016 % | 0,022687 % |
| 2500–6000 Hz | 0,00145873 % | **2,622953 %** | 0,010018 % |
| 6000–12000 Hz | 0,00000166353 % | **8,462703 %** | 0,001092 % |

También falla el criterio por bandas. La diferencia aparece en ambos
controles; eso no identifica su causa ni aísla al apagador. La repetición
de pedal-up sin pedal no abre los apagadores y el último descenso se deja
evolucionar físicamente, sin fade ni corte artificial de salida.

WAV: `C:/SHARED/Piano-contacto-integrado-2026-10-09/Frase-integrada-sin-pedal.wav`.
Ambos archivos tienen 100.967 frames, 2,103479 s, dos canales float32 a
48 kHz. Verificados independientemente: SHA256, formato, finitud y clipping;
también fuentes, referencias originales, eventos y gates conservados.
El informe confirma fuente/tabla/R sin cambios. Ningún trabajador sigue activo.

La aceleración del recorrido completo es **1,83–1,85×**, no 12–20×:
aquellos factores medían sólo prefijos de contacto contra tasas finas.
La física y los coeficientes se conservan; ésta es una integración más rápida
del mismo modelo y no una nueva búsqueda de timbre.

**Decisión:** conservar el backend como candidato numérico y los WAVs como
diagnósticos. No aprobar reproducción por bandas, convergencia temporal
completa, precisión del aire ni selección de fábrica. El siguiente ensayo
debe contrastar las bandas contra referencias temporales/capturas más finas,
separando dispersión de contacto y transición a coast antes de cambiar
fieltro, afinación o cuerpo. Después completar banco con F4 y escucha.

## Pendientes

- Convergencia temporal del recorrido completo, incluidos los estados de
  aire: los controles cortos anteriores no certifican dos segundos a 16 kHz.
- Atribuir el fallo de bandas altas contra contacto/captura más finos antes
  de usar el error respecto al control anterior para retocar el timbre.
- Fidelidad del operador acústico y energía/radiación de duración completa.
- Ampliar el banco con F4 y contrastar el timbre/ataque con el Steinway.
- Continuar el trabajo de coste según el perfil completo. El producto
  contiguo conserva las ecuaciones, pero no resuelve por sí solo el coste
  de proyecciones, residuo no lineal y apagadores.

El plugin instalado conserva su versión; ésta es una trayectoria física
offline del registro de tres teclas, no un instrumento completo en tiempo real.
