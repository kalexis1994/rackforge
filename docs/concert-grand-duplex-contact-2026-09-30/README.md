# Click residual en La Campanella: contacto de la resonancia duplex

El usuario señaló un click después de 5,35 s del fragmento 92–100 s.
Los eventos próximos son B5 (MIDI 83, velocidad 69) a 97,341156 s y B6
(MIDI 95, velocidad 71) a 97,407551 s. La aguja de alta frecuencia coincide
con el segundo, en el frame 4295673 de la pieza a 44,1 kHz.

## Diagnóstico con el historial MIDI completo

Se conservaron todos los eventos desde frame cero, con pedales y vibraciones
previas. El render usado para registrar las etapas internas es idéntico,
muestra a muestra, al anterior. El salto aparece en la suma de cuerdas y
entra después en la caja; no nace en la ruta mecánica ni en el pulso de
contacto estructural. Tampoco desaparece al alargar las rampas existentes.

La ablación `DUPLEX_LEVEL=0` reduce unos 51 dB la energía inicial de
8–18 kHz en la señal mono de este evento. Los resonadores de los tramos
de cuerda detrás del puente recibían su nuevo impulso directamente en
`c`, sin seguir la rampa de contacto de las cuerdas principales. Aunque
`s` queda continuo antes del tick, en ese mismo tick `c*sin(omega)` entra
completo en la salida; en B6, el primer resonador duplex está cerca de
8,3 kHz y el salto es audible como un click.

## Corrección experimental

En `RESTRIKE_PHASE_PRESERVE=1`, el impulso duplex nuevo gira con su
resonador y llega durante el mismo contacto smoothstep que los parciales.
La vibración existente no se reinicia ni se atenúa. El resonador permanece
activo y conserva la contribución compleja completa al terminar la rampa.
La ruta histórica con el control en cero se conserva. Las notas nuevas
siguen usando su ataque original. No cambian los coeficientes del modelo.

El cambio está en la copia de investigación ignorada
`local/concert-grand-coherent-unison/src/lib.rs`; no se instaló ni modificó
el plugin de producción. El ejecutable congelado es
`local/concert-grand-click-97s/renderer.exe`, SHA256
`7af3409d4a63d62641a006203e2dafe4fbd100c4baa70579af62c89c2c800028`.

## Verificación y entregables

- 113 pruebas nativas pasan; 75 pruebas de laboratorio ignoradas.
- Prueba analítica a 8,31 y 16,4 kHz: llegada suave, preservación de ambos
  estados complejos y limpieza del impulso pendiente al iniciar otro contacto.
- Seis ataques nuevos (C2 fuerte, C4 a 60/124, C5 fuerte, F#6 fuerte,
  C7 fuerte): audio idéntico muestra a muestra al candidato anterior.
- En el evento señalado, potencia de 8–18 kHz del primer milisegundo,
  promediando ambos canales: **−31,60 dB** respecto del render anterior.
  La reducción no equivale a una mejora de fidelidad de todo el instrumento.
- Dos MIDI completos, con los mismos tiempos, velocidades, controladores,
  frames y ganancia común de 25,120884443707872 dB. PCM24 estéreo a
  44,1 kHz, sin EQ ni compresión adicionales.

Resultados en `C:/SHARED/Contacto-duplex-corregido`: fragmentos Antes y
Corregido, piezas completas en MIDI, modelo y `validacion.json`.
La escucha del usuario determina si quedan otros clicks perceptibles.
La comprobación se reproduce con `tools/verify-concert-grand-duplex-contact.py`.
