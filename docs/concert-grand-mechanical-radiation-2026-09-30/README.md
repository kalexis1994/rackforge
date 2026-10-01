# Variante C: radiación acústica del mecanismo

## Problema observado

El ruido de teclas/acción tiene componentes graves y agudas. Queda especialmente
expuesto en el registro agudo, donde el tono lo enmascara menos. El usuario también
señala demasiado ruido de pedal en las piezas completas.

## Recorridos comprobados en el código

- Los golpes sintéticos de acción, los tres resonadores de la varilla y el golpe
  de fondo de tecla entraban en la suma que excita el puente y las resonancias
  simpáticas. Al iniciar una nota, `thump_keybed` era falso.
- El retorno de tecla ya tenía una vía directa de teclado, pero la acción del
  ataque no la utilizaba.
- El ruido de pedal se añadía a ambos canales después del cálculo de sus fuentes
  directas. No utilizaba distancia, patrón del micrófono ni el margen interno de
  nivel del instrumento.

## Cambio experimental

`MECHANICAL_RADIATION=1` clasifica los componentes sintéticos de acción y dirige
sus golpes, el fondo y el retorno de tecla por una fuente acústica de teclado.
Dejan de excitar directamente el puente o el banco simpático. El contacto físico
martillo-cuerda y su pulso estructural hacia la tabla mantienen sus coeficientes.

Teclado y pedal tienen vías con respuesta de micrófono, caída de presión `1/r` y
retardo fraccionario del recorrido adicional respecto al centro de la tabla. El
pedal usa una altura de fuente de 0,1 m; el teclado conserva la aproximación de
desplazamiento lateral de 0,9 m. Cada vía conserva su estado durante la cola.

Una emisión común de amplitud `0.125` y un filtro de un polo a 1500 Hz representan
provisionalmente emisión/atenuación del mecanismo y pérdidas de la caja. Son
parámetros de calibración acústica, **no coeficientes físicos medidos**. Se aplican
a todo el teclado y los pedales. El filtro actúa sobre el mecanismo, no sobre las
cuerdas ni sobre el pulso estructural del ataque.

El pedal se convierte desde su antigua escala posterior al margen interno hacia
la escala de fuentes internas mediante el margen compilado. Después pasa por el
mismo margen que las demás fuentes. Así se conserva el significado de su escala
de origen sin mantener el bypass acústico.

El campo tardío de sala recibe la emisión del mecanismo. Las primeras reflexiones
siguen usando la aproximación centrada en la tabla; no se implementó un trazado
completo independiente de superficies para cada fuente mecánica. Tampoco se
resolvió un sistema recíproco de vibración de caja/teclado/pedales. Las amplitudes
de origen son sintéticas y no están identificadas como fuerzas en newtons.

La separación entre vibración tonal y de banda ancha tiene antecedentes en
[Keane, *Separation of piano keyboard vibrations into tonal and broadband components*](https://www.sciencedirect.com/science/article/abs/pii/S0003682X0600137X).
Ese trabajo analiza vibraciones de teclas; no determina la ganancia ni la respuesta
acústica al micrófono empleadas en este experimento.

## Verificación

- 103 pruebas normales aprobadas. Las dos huellas de audio de compatibilidad se
  conservan: `0x243e622a8b59910d`, `0x706b728c4d0a6b06`.
- Diez notas sostenidas: C3/C4/C5/C6/C7 a velocidades 60 y 124. Con las fuentes
  sintéticas de ataque desactivadas, los WAV de cuerdas/contacto son idénticos
  entre ambas rutas. Este resultado corresponde a esas pruebas sostenidas.
- La contribución mecánica del ataque, aislada por diferencias entre renders,
  baja 9–14 dB en C4–C7. La energía total de los primeros 300 ms cambia menos de
  0,036 dB. La cola de C6/C7 cambia hasta 0,204 dB por la eliminación de excitación
  y resonancia inducida por el mecanismo; no equivale a una pérdida del tono
  sostenido cuando las fuentes mecánicas están desactivadas.
- Pedal aislado: antes, micrófonos a 0,66/2,64/8 m producen exactamente el mismo
  nivel. Con la nueva ruta, el nivel depende de la distancia. A 2,64 m, el RMS de
  los primeros 300 ms baja 19,26 dB.
- Nocturno y La Campanella conservan los mismos hashes de MIDI y eventos que la
  variante C anterior, con tempo, velocidades y pedales. WAV estéreo 44,1 kHz,
  24 bits y diez segundos de cola. No hay valores no finitos ni recorte PCM. La
  ganancia común de exportación cambia solo −0,0202 dB respecto a los WAV C previos.

## Reproducibilidad

`model.json`, `results.json`, `verification.json` y `research.patch` conservan la
configuración, medidas y hashes. El patch incluye el estado acumulado de la rama
de investigación y usa como base el plugin histórico identificado por su hash.
No debe aplicarse sobre otro estado del plugin sin reconciliar sus diferencias.

El renderizador congelado y la caché están en
`local/concert-grand-coherent-unison/frozen-mechanical-C` y `mechanical-midi`.
La auditoría se repite con `tools/audit-concert-grand-mechanical-radiation.py`.
Esta entrega es una prueba nativa de audio; aún no se distribuyó un plugin WASM
con estas vías ni se midió su coste en el callback del host.

Audios: `C:\SHARED\C-mecanica-acustica-chopin-nocturno-op9-2.wav` y
`C:\SHARED\C-mecanica-acustica-liszt-la-campanella.wav`.
