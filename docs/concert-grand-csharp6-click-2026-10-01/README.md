# Diagnóstico dirigido del golpe en C#6

El usuario indica que la variante de reutilización de cuerda sigue sonando
igual respecto al click, y localiza el defecto en el quinto grupo del audio
`Notas-cortas`, cerca de 00:08: **C#6, MIDI 85**, tres golpes de velocidad 70,
28 ms de tecla mantenida cada 90 ms. La identificación fue confirmada por el
usuario; no se trata del quinto golpe individual.

Se congela el renderer de la variante anterior (`db93d789d5aa84376fd5eb57932c3245e8a71556d3ba06678ea1903b6fb7d425`).
Durante el diagnóstico no se cambiaron coeficientes de producción ni se
atribuyó el síntoma a otra causa sin la audición de las ablaciones. El commit aceptado sigue
siendo `1bb8436` y la propuesta anterior sigue sin commit.

## Pruebas realizadas

Se repiten las mismas siete alturas y todos sus eventos desde el cuadro cero,
con el mismo renderer, programa Concert y ganancia común de exportación 0 dB.
Se ablaten por separado:

- el pulso estructural añadido al puente (`CONTACT_BOARD_GAIN=0`);
- los dos osciladores auxiliares duplex (`DUPLEX_LEVEL=0`);
- emisión mecánica (`MECHANICAL_EMISSION_GAIN=0`);
- fuente longitudinal y su impacto (`LONGITUDINAL_MIX=0`, `IMPACT_CLANG=0`);
- sala tardía (`ROOM_MIX=0`);
- llegada inicial de 4 ms en lugar de 1.5 ms (`ATTACK_RAMP_S=0.004`).

Las trazas del ataque fresco se obtienen además para B4, B5, C#6 y F#6,
con 200 ms de preparación silenciosa. En C#6 el mecanismo añadido al ataque
es exactamente cero, pero eso no descarta sonidos de liberación posteriores.
El contacto calculado dura 1.964 ms y alcanza 7.990 N. Son valores del modelo,
no mediciones de un martillo real.

La retirada del pulso de caja cambia principalmente los graves del transitorio.
La retirada del duplex cambia su componente aguda, pero tiene poca energía
respecto de la nota completa. Alargar la entrada modifica la reconstrucción
del estado precalculado; no modifica el fieltro ni demuestra una fuerza física
más realista. Estas alternativas se prepararon para diagnóstico.

Primeros 15 ms del ataque C#6, RMS estéreo sin normalización:

| Variante | 20–250 Hz | 1–4 kHz | 4–8 kHz |
| --- | ---: | ---: | ---: |
| A actual | 0.003062 | 0.029177 | 0.000495 |
| B sin pulso de caja | 0.000126 | 0.029179 | 0.000495 |
| C sin duplex | 0.003062 | 0.029174 | 0.000219 |
| D llegada 4 ms | 0.003153 | 0.027161 | 0.000299 |

Las métricas no deciden cuál de estas contribuciones es el golpe percibido.
El usuario resolvió esa distinción con el audio preparado: **«la C no tiene
el tick»**. La C retira únicamente la excitación duplex añadida.

## Corrección seleccionada

Se cambia `DUPLEX_LEVEL` de 0.018 a cero tanto en el código como en la
calibración común y en el valor inicial del panel. Se evita crear los dos
osciladores con nivel cero. El cambio se aplica a todos los programas; no
se altera el contacto, el pulso de caja, el unísono principal de tres
cuerdas, la curva de velocidad ni los filtros de salida.

El comentario anterior afirmaba que el duplex recibía energía sólo a través
del puente. En realidad `Component::start` inicializaba directamente dos
osciladores al golpear cada tecla. Se corrige esa descripción en el código
y en `PIANO_MODEL.md`. No se presenta esta retirada como un modelo físico
completo de duplex: su acoplamiento pasivo queda pendiente. La aproximación
histórica sigue disponible para investigación; su prueba de decaimiento
activa explícitamente el nivel 0.018.

La banda de 4–8 kHz de los primeros 15 ms de C#6 baja aproximadamente
7.09 dB. Los RMS de 20–250 Hz y 1–4 kHz varían menos de 0.001 dB.
Esta medida describe el cambio espectral, no prueba por sí misma que toda
la pieza carezca de clicks.

### Verificación

- Renderer final: `3b6ac0541fcd740b2cdab1586158b2e66790a6d367438f7ac91228970bb1d2b7`.
- Los primeros 463,050 cuadros de la secuencia de notas son idénticos bit
  por bit a la ablation C escuchada por el usuario.
- Los 5,115,600 cuadros de La Campanella (0–116 s) son idénticos bit por bit
  al renderer congelado anterior con `DUPLEX_LEVEL=0`. Se recorta 106–115 s
  después de reproducir todo el MIDI previo, con sus pedales.
- 124 pruebas nativas aprobadas, 75 diagnósticos ignorados. Incluyen el
  unísono de tres cuerdas y la concordancia del panel con el motor.
- Dos pruebas de huella ejecutadas por separado: `232b085ed4248398` y
  `4371ab4adc1542ab`. Se actualizan deliberadamente por la retirada del duplex.
- WASM compilado y tres pruebas del runtime aprobadas: arranque,
  equivalencia de una/cuatro instancias y repetición tras liberaciones cortas.

Los audios nuevos y `mediciones.json` están en
`C:/SHARED/Click-duplex-corregido`: C#6 antes/después, secuencia corta nueva
y pasaje de La Campanella antes/después. Exportación común a 0 dB, sin
normalización ni EQ. La audición del nuevo pasaje queda pendiente.
Esta corrección y la propuesta de reutilización de cuerda siguen sin commit;
el paquete publicado 0.172.0 no se reemplaza con estos renders.

Reproducir (numpy, scipy, soundfile):

```powershell
python tools/audit-concert-grand-duplex-fix.py --baseline-binary local/concert-grand-csharp6-click/renderer.exe --candidate-binary local/concert-grand-duplex-fix/renderer.exe --baseline-midi local/concert-grand-short-click/candidate-midi/performance.f32 --events local/concert-grand-air/audition/Campanella-B/events.txt --probe-events local/concert-grand-short-click/notes-B/events.txt --baseline-probe local/concert-grand-short-click/notes-B/performance.f32 --selected-probe C:/SHARED/Click-Csharp6/diagnostico/sin-duplex/performance.f32 --work local/concert-grand-duplex-fix/audit --output C:/SHARED/Click-duplex-corregido
```

El script falla si el nuevo audio no coincide exactamente con la ablación
seleccionada. Ejecutables, WAV y logs quedan fuera de Git. El informe final
se conserva aquí como `correccion-mediciones.json`.

## Audios

`C:/SHARED/Click-quinta-nota/Csharp6-A-B-C-D.wav` contiene A, B, C y D, cada
sección con tres golpes y 350 ms de silencio entre versiones. Los recortes
individuales están al lado, junto a `LEEME.txt` y `mediciones.json`.
Las ablaciones adicionales e informes con hashes se guardan en
`C:/SHARED/Click-Csharp6/diagnostico`.

Los WAV, ejecutables y trazas están fuera de Git. Scripts locales:
`local/concert-grand-csharp6-click/diagnose.py` y `export_diagnostics.py`.
Las ablaciones usan `tools/audit-concert-grand-attack-sources.py`,
`local/concert-grand-csharp6-click/model.json` y `profiles.json`, el MIDI de
`local/concert-grand-short-click/notes-B/events.txt` y su raw de referencia.
