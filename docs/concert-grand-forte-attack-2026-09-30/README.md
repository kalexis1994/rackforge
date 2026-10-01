# Ataque y batimientos fuertes: configuración elegida para escucha

## Decisiones de escucha

El usuario señaló un pluck en los ataques, particularmente en La Campanella
alrededor de 00:40 y 01:36. Después precisó que las intensidades suaves cumplen
mejor y que la última intensidad de cada nota rompe en batimientos.

Prefirió el timbre de `Contacto-mas-blando-intensidades.wav` y confirmó que
`Sin-batimientos-intensidades.wav` no presenta esa explosión. Pidió solamente
un endurecimiento pequeño de los graves. Se preservaron ambas decisiones.

## Configuración

Procedencia completa: `C:/SHARED/Sin-batimientos-ajuste-suave/modelo.json`.
Parte de rate-020, K_A0=4.900.000, contacto integrado hasta 19 kHz,
micro a 2,64 m y pérdidas de tabla a 4,6%. Se mantiene el contacto disipativo,
la radiación mecánica y la reconstrucción de reiteraciones de la base.

La variante aceptada tiene `UNISON_WIDTH=0`,
`STRING_STIFFNESS_SPREAD=0` y `POLARISATION_CENTS=0`. Mantiene tres estados
verticales y sus acoplamientos, con afinación y rigidez idénticas: es un
unísono ideal de diagnóstico, no una afinación identificada del Steinway.
La respuesta reactiva del puente permanece activa.

El ajuste leve usa K_A0=7.350.000. Con
`d=log10(1.5)/0.448`, se fija `FELT_K_DECADES=4-d` y
`FELT_TREBLE_DECADES=2-0.552*d`. La compensación conserva algebraicamente
la rigidez desde posición 0,448 hacia arriba y la aumenta progresivamente
por debajo; se respeta el suelo de la tabla de fieltro existente. Es una
curva global de voicing efectiva, no un ajuste por nota.

## Evidencia y límites

Diez notas MIDI: 24/36/48/60/65/67/72/84/90/96; velocidades 60 y 124,
ocho segundos sostenidos y dos de release. Una ganancia común, sin EQ ni
compresión. El sampler de F4/G4 usa F#4 transpuesta -1/+1 semitono.

- En los graves probados, la banda 1,8–8 kHz del inicio aumenta 0,54–2,37 dB.
- El cuerpo 200–1200 Hz a 50–200 ms cambia menos de 0,29 dB.
- Desde C4, el cambio de esas medidas es menor de 0,001 dB; la energía de
  la diferencia de los renders completos está entre -65,9 y -78,2 dB
  respecto a la base. La diferencia residual proviene de aritmética float.
- Silenciar tensión o modos longitudinales cambia muy poco estos ataques.
  Silenciar los mecanismos tampoco elimina los mínimos de envolvente del
  unísono, aunque un tick breve puede ser perceptivamente saliente.
- El conteo de mínimos por parcial no identifica por sí solo el defecto:
  el sampler presenta más mínimos en algunas ventanas. La aceptación
  perceptiva de la variante sin desafinación fue explícita del usuario.

Se ensayó también una transición inicial por velocidad. El usuario precisó
que los ataques suaves cumplen mejor, por lo que esa reconstrucción quedó
**desactivada** en la configuración actual. Su código permanece en el
prototipo aislado con valor predeterminado cero; se comprobó paridad binaria
de la primera parte de un ataque con el ejecutable anterior.

112 pruebas nativas aprobadas, 75 ignoradas. Esto no verifica rendimiento
ASIO ni fidelidad de todo el teclado. El corpus sigue siendo BitKlavier,
sin una validación independiente nueva.

## Entregables

`C:/SHARED/Sin-batimientos-ajuste-suave/` contiene los graves antes/después,
los medios/agudos, los fragmentos de La Campanella, configuración,
validación y log de pruebas. `MIDI/` contiene Nocturno y La Campanella
completos con eventos de pedal originales, 44,1 kHz, estéreo PCM24 y cola
de diez segundos. Los dos MIDI usan una misma ganancia y no recortan.

No se modificó ni instaló el plugin de producción. El ejecutable congelado
de investigación está en `local/concert-grand-soft-attack/renderer.exe`;
el código aislado, en `local/concert-grand-coherent-unison`.
