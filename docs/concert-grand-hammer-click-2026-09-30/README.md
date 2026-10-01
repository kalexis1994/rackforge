# Click añadido al golpe: continuación de la variante sin batimientos

El usuario aceptó la variante sin batimientos y el ajuste leve de graves, pero
pidió quitar el click que percibe como martillo. La prueba actual conserva
esa base y cambia solamente `KNOCK_LEVEL=0` y `THUMP_BASE=0`.

El primero elimina los componentes afinados del chasquido y el burst de ruido
de ataque. El segundo elimina el ruido añadido de fondo de tecla. El martillo
sigue excitando las cuerdas mediante el contacto disipativo; el pulso estructural
del contacto y las rutas de caja permanecen activos. Se mantienen los mecanismos
de release y los pedales. No se añadió EQ, compresión ni una rampa nueva.

## Comprobación

Se compararon las capas por separado y el silenciamiento de toda la ruta
mecánica en C2/C4/F4/G4/C5/F#6/C7 a velocidades 40/60/84/124. La variante
final elimina los dos generadores de golpe, conservando la ruta mecánica para
los otros mecanismos. Las alternativas son diagnósticos, no configuraciones
seleccionadas adicionales.

En C2 fuerte, C4 a 60/124, C5 fuerte, F#6 fuerte y C7 fuerte, los primeros
300 ms muestran igualdad exacta de:

- fuerza y estados del contacto;
- amplitudes, fases y escala modales;
- carga calculada del puente;
- excitación del puente y de la tabla;
- señales propias de la tabla y sus observadores izquierdo/derecho.

La contribución `keybed_direct` del golpe queda exactamente en cero durante
esos ataques. Las trazas y sus hashes están bajo
`local/concert-grand-hammer-click/trace`; la verificación es reproducible con
`tools/verify-concert-grand-hammer-click.py`. Esto confirma la retirada de las
capas sintéticas; la escucha debe confirmar si resuelve todo el click percibido.

Nocturno y La Campanella mantienen los mismos hashes de eventos, número de
frames y ganancia común de la entrega anterior: 25,120884 dB. Se verificaron
salidas finitas, estéreo a 44,1 kHz, PCM24 y sin recorte. Se preservan diez
segundos de cola y los controladores de pedal completos.

## Archivos

`C:/SHARED/Sin-click-martillo/` contiene `modelo.json`, `validacion.json`,
`validacion-contacto.json`, las intensidades antes/después y fragmentos exactos
de Campanella 36–44/92–100 s y Nocturno 2–10 s.
`MIDI/` contiene ambas piezas completas. `Prueba-final/` conserva el diagnóstico.

No se modificó ni instaló el plugin de producción ni se cambió el código del
motor en esta etapa. Se usa el ejecutable congelado
`local/concert-grand-soft-attack/renderer.exe` de la etapa anterior.
