# Modelo 13: contacto de martillo con tres cuerdas

Prototipo nativo de investigación, 30 de septiembre de 2026. Motivación: el usuario percibe los agudos del Unísono 12 más delgados y casi como una sola cuerda. El modelo 13 queda disponible para escucha; todavía no está seleccionado para producción ni instalado como plugin.

**Evaluación posterior:** el usuario rechaza el 13 y señala que el adelgazamiento viene de varias iteraciones. No se considera una mejora auditiva. Véase [la revisión del timbre acumulado](../concert-grand-timbre-history-2026-09-30/README.md).

## Qué cambió

El motor anterior calculaba un contacto con una cuerda equivalente y repartía su estado entre las tres cuerdas. Además, cuando quedaban menos de cuatro modos bajo el límite del cálculo, reemplazaba el contacto por una fórmula: desde aproximadamente G♯6 también se perdía la excitación estructural de caja asociada al contacto.

El prototipo usa una cabeza de martillo compartida y tres cuerdas con estados modales propios. Cada contacto tiene su compresión, fuerza e historia de relajación del fieltro. La suma de fuerzas frena el martillo; cada fuerza excita su cuerda. Pequeñas diferencias fijas de altura y afinación rompen la simetría de la excitación. Se aplica desde C2 a los grupos de tres cuerdas, conservando el modelo anterior en los graves de una y dos cuerdas. Los pesos de contacto siguen la convención existente del pedal una corda.

Se recupera el contacto y su excitación de caja incluso con pocos modos disponibles. No se añadieron correcciones de ecualización por nota. Se conserva la ruta acústica de los mecanismos de la variante C anterior.

## Configuración seleccionada para esta escucha

| Parámetro | Valor |
| --- | --- |
| Paso del contacto | 2 µs |
| Base modal del contacto | 8 kHz |
| Dispersión máxima efectiva de alturas | 80 µm |
| Ancho de unísono | 0,05 |
| Relajación del fieltro | 0,2 ms |
| Dispersión de rigidez / desafinación de polarización | 0 / 0 |
| Distancia de micrófono | 2,64 m |

Los valores de altura, fuerza radiada y acoplamiento son parámetros efectivos provisionales; no son mediciones identificadas de un Steinway. Se mantiene la base de contacto calibrada de 8 kHz y la continuación analítica del espectro por encima de ella. El pulso de caja usa la fuerza media de las tres cuerdas para conservar la calibración anterior; la fuerza que frena la cabeza es la suma.

Se probó y descartó una base de 30 kHz porque adelgazaba G6. También se probaron cambios globales de dureza y exponente del fieltro. La variante elegida conserva esos parámetros y cambia su relajación global; no adapta por separado F4, G4 o C5.

## Ajuste y comprobaciones

El ajuste reutiliza el corpus histórico BitKlavier/Steinway: 8 pares nota/velocidad para ajuste y otros 13 para comprobación interna. No es un conjunto independiente nuevo. Se añadieron 12 pares de notas intermedias como guardas de cuerpo, sin comparación con una muestra original. El error agregado pasa de 0,9178 a 0,9090 en ajuste y de 1,1815 a 1,1550 en comprobación interna. La mejora es modesta y no demuestra por sí sola fidelidad perceptiva.

Las guardas protegen el ataque y dos ventanas posteriores. Las pérdidas de cuerpo se evalúan cuando la ventana de referencia supera −80 dBFS: este es un umbral operativo del análisis, no una afirmación sobre audibilidad. Se permite recuperar energía de ataque, pero se limita su pérdida. Tras detectar el problema de G6 se ampliaron las guardas antes de publicar los audios.

- 107 pruebas regulares aprobadas; 74 ignoradas. Dos huellas de audio heredadas permanecen idénticas.
- Ocho renders de C4, C5, C6 y C7 a velocidades 60/124, con el contacto nuevo desactivado, coinciden bit a bit con la variante C anterior.
- Las pruebas del contacto comprueban simetría, contactos desiguales, contacto ausente y balance de energía en condiciones conservativas.
- Al pasar de 2 a 1 µs, la diferencia máxima entre energías de bandas del ataque es 0,103 dB en C4/C5/C6/C7 a velocidades 60/124.
- La escucha final cubre C4/F4/G4/C5/G5/C6/G6/C7 a 60/84/124: cambio de ataque entre −0,068 y +3,228 dB; pérdida máxima de cuerpo en ventanas sobre el umbral: 1,510 dB.
- En el análisis de notas fuertes sostenidas, con hasta 16 parciales hasta 10 kHz, los intervalos de mínimos recurrentes contados son `[7, 2, 0, 0, 0]` tanto en C como en 13, para 0,1–0,5 / 0,5–1,5 / 1,5–3,5 / 3,5–7,5 / 7,5–11,5 s. No demuestra ausencia de crujido audible en cualquier nota o dinámica.
- Los MIDI conservan eventos, tempos y controladores. Ambos WAV de piezas se exportan con la misma ganancia fija de 21,8531 dB, sin normalización por nota; picos 0,4582 y 0,7804, sin recorte.

El análisis de graves filtra la grabación completa antes de recortar el comienzo, evitando crear un transitorio artificial. En C7 fuerte, la energía de 60–300 Hz relativa al total en 0–40 / 40–100 / 100–300 ms es: original −19,86/−11,07/−13,47 dB; C −34,36/−45,57/−49,93 dB; 13 −24,48/−24,46/−22,65 dB. Se recupera parte del cuerpo, pero persiste una diferencia. Grabación y modelo tienen condiciones distintas; esa banda incluye sala y mecanismos y no identifica por sí sola una causa física única.

## Límites y rendimiento

El puente no realimenta las cuerdas durante el contacto; después se conserva la proyección existente sobre el puente, con una componente horizontal compartida. No hay todavía un sistema completo de seis polarizaciones con tabla armónica recíproca. La continuación por encima de 8 kHz es analítica y persisten la normalización modal y la curva de sonoridad existentes: el balance de energía del contacto no se convierte directamente en presión absoluta al micrófono.

El benchmark nativo usa bloques de 128 frames a 48 kHz, con un plazo de 2667 µs. Con ocho notas nuevas, C registra media/máximo de 1904/2664 µs y 13 de 3274/4179 µs; con cuatro notas, el máximo de 13 es 2985 µs. El prototipo excede el plazo en esos casos y necesita optimización antes de integrarse. Esto es un benchmark offline, no una prueba del driver ASIO a 44,1 kHz/256 frames.

## Audios y reproducción

En `C:\SHARED\Modelo-13` están los agudos C5/G5/C6/G6/C7 a 60/84/124, los medios C4/F4/G4/C5 y los agudos sostenidos 12 s. Hay versiones `C-anterior` con las mismas secuencias; estas son nuestro modelo previo, no muestras originales. Cada WAV tiene metadatos con secuencia, ganancia y huellas. Las piezas están en `C:\SHARED\Modelo-13-chopin-nocturno-op9-2.wav` y `C:\SHARED\Modelo-13-liszt-la-campanella.wav`.

Fuente del prototipo: `local/concert-grand-coherent-unison`. Renderizador congelado: `frozen-joint-13b`. SHA-256 del binario: `e0201aa2a39b844717daf6b963bfc144c2079498e3b92662a3d5b0750e242d2d`. `model.json` fija sus parámetros; `verification.json` registra pruebas y huellas. `research.patch` contiene los cambios acumulados contra `plugins/concert-grand/src` del checkout histórico `C:\Users\kalex\.codex\worktrees\concert-grand-refit\rackforge`.

## Referencias físicas

[Weinreich, KTH: cuerdas ligeramente desafinadas](https://www.speech.kth.se/music/5_lectures/weinreic/mistuned.html) describe cómo el puente puede modificar frecuencias y decaimientos, de modo que la desafinación no produce necesariamente batimientos persistentes. Sus umbrales numéricos ilustran un caso concreto y no se aplican universalmente al teclado.

[Woodhouse, Euphonics: múltiples cuerdas y dobles decaimientos](https://euphonics.org/7-3-multiple-strings-and-double-decays/) muestra que cuerdas idénticas excitadas simétricamente pueden comportarse como una sola cuerda más gruesa, y que romper la simetría y representar el acoplamiento modifica la respuesta. Esto motiva probar diferencias en el contacto real; no demuestra que el presente modelo reproduzca completamente ese mecanismo.
