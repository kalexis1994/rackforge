# P6: rechazo de la frase, apagado de tecla y cobertura espectral

2026-10-05. El usuario rechaza la primera frase SI por poca definición,
predominio grave, falta de brillo y finales secos. Esa frase **no queda
aceptada como modelo musical**. Sus pruebas verificaban el paso temporal de
un sistema truncado; no verificaban cobertura modal ni fidelidad al piano.

## Diagnóstico medido

La escucha anterior usaba una cuerda por altura, 59 modos de tabla hasta
1193,508 Hz, micrófonos a 0,66 m sobre la placa, sin apagador ni sala. Se
cortaba el sostenido al segundo con una edición coseno de 8 ms. Ese final
no representa soltar una tecla: falta drenar energía de la cuerda y conservar
la vibración de tabla y la memoria del aire durante la liberación.

`medidas-P6-release/bandwidth.json` compara proporciones de energía, con
Hann en ventanas de 20–120 ms y 150–800 ms. Se retiran los 100 ms de silencio
del piloto; la referencia se cuenta desde el comienzo de su grabación.
Son C4/C5 exactos de Stage ORTF, capa 16, frente al piloto MIDI 124: no
se igualan instrumentos, fuerza ni micrófonos y no se identifica un filtro.

| Nota / ventana | 1,2–6 kHz / 60–1200 Hz, piloto | Steinway | Diferencia |
| --- | ---: | ---: | ---: |
| C4, temprano | −24,96 dB | −9,47 dB | −15,49 dB |
| C4, sostenido | −22,81 dB | −14,05 dB | −8,77 dB |
| C5, temprano | −20,65 dB | −12,06 dB | −8,58 dB |
| C5, sostenido | −16,37 dB | −18,53 dB | +2,15 dB |

Por tanto, un mismo shelf global tampoco corrige la evolución de ambas notas.
`balance.json` separa 40–250 Hz de 250–1200 Hz: el piloto no tiene más
energía **relativa** bajo 250 Hz en estos controles. En los ataques C4/C5
da −25,40/−26,01 dB frente a −19,08/−22,80 dB de la referencia. La
sensación grave puede provenir del predominio del fundamental y la falta de
armónicos; esta medición no invalida la descripción perceptual del usuario.

### Truncamiento de tabla

Se calculó presión por fuerza armónica unitaria en el puente de la misma
placa P2, pérdida 0,046 y micrófonos, conservando 59, 90 o 256 modos. Es
una **ablación estructural sin realimentación del aire** en todos los
controles; no se suma su respuesta a la nota SI ni se renderiza como piano.

En 1,2–3 kHz, 59 frente a 256 da 98,6–105,9% de diferencia compleja
relativa y 6,91–7,71 dB menos potencia integrada, según altura. Los 90
modos conservan todavía 80,5–86,0% de diferencia. En 3–6 kHz el cambio
puede subir o bajar la potencia por interferencia: agregar modos no es
equivalente a subir agudos. Los 256 del archivo sólo llegan a 2965,08 Hz;
**tampoco son un control convergido de banda alta**. Bajo 1200 Hz los
cambios complejos son menores, 3,0–6,7%.

La hipótesis prioritaria pasa a ser completar la admitancia/radiación alta
y medirla, antes de identificar el timbre con este banco. Ege/Boutillon
describen el cambio de régimen de una tabla con costillas alrededor de
1,1 kHz: [artículo y modelos](https://arxiv.org/abs/1305.3057).
Ese valor procede del instrumento estudiado y no es un umbral universal
ni una identificación de nuestra geometría supuesta.

### Posición de micrófonos

`colour.json` compara la misma trayectoria con altura 0,15 frente a 0,66 m.
La geometría cercana pasa aproximación de retardo 0,011419% y refinamiento
espacial 0,136920% a las frecuencias de prueba hasta 6 kHz. E4 gana unos
1,9 dB de proporción alta, pero G4 pierde 4,5–5,8 dB y C5 pierde unos
1,8–1,9 dB. **No se selecciona** este cambio ni se usa en el nuevo WAV.
El kernel cercano se conserva como dato único del control rechazado.

## Liberación de tecla

Se añadió `concert_grand_damped_coast.py`: tracción viscosa distribuida
sobre una zona interior de la cuerda. La velocidad incluye el extremo
móvil; la fuerza usa la transpuesta del mismo mapa para conservar
reciprocidad. La matriz de disipación es `RᵀR`, semidefinida positiva.
Su subflujo exponencial se intercala simétricamente con el coast existente.
El calor se obtiene de la pérdida exacta de energía cinética del subflujo.
No se ponen a cero cuerda, tabla ni estados acústicos; no se aplica fade
al sonido para simular el apagador.

Parámetros **supuestos**, no medidos: centro 0,30 L, ancho 0,08 L,
resistencia total 0,6 N·s/m, 16 puntos de Gauss, descenso suave de 30 ms.
El esquema es una primera aproximación resistiva. Faltan masa del
apagador, contacto unilateral, fricción, rigidez del fieltro, acción y
pedales. La tesis de Bank distingue la posición del apagador y los modos
con nodos próximos a ella de un simple cambio global de ganancia:
[sección 5.2.5](https://home.mit.bme.hu/~bank/thesis/pianomod.pdf).
No se atribuyen nuestros coeficientes a esa tesis.

Cuatro pruebas nuevas pasan: exponencial matricial independiente,
equivalencia exacta apagador abierto, balance con radiación angular
independiente en el fixture y orden dos frente a DOP853 del sistema
amortiguado completo. La cuadratura de la matriz de resistencia 16→32
en E4/72 modos difiere 1,5552e−7 en norma relativa.

Se continúan los estados existentes al segundo a 64/128 kHz; sólo se
guardan las colas nuevas. Los controles reales de nota y el WAV de release
se registran en `medidas-P6-release/nota-<60|64|67|72>.json` y `frase.json`.
El balance de radiación larga es inferido por almacenamiento menos calor,
igual que antes; no es otra integral angular independiente.

Pasan los cuatro controles reales para cada altura. Comparación de las
colas de 500 ms a 64 frente a 128 kHz (presión compleja en 60–6000 Hz):

| Nota | Presión | Posición tabla | Velocidad tabla | Calor apagador, fino | RMS final / sostenido |
| --- | ---: | ---: | ---: | ---: | ---: |
| C4 | 0,303258% | 0,095203% | 0,215262% | 0,01042564 J | −85,96 dB |
| E4 | 0,281123% | 0,059877% | 0,152959% | 0,00765996 J | −117,08 dB |
| G4 | 0,540019% | 0,057403% | 0,144389% | 0,00360464 J | −129,99 dB |
| C5 | 0,429350% | 0,052064% | 0,102160% | 0,00183972 J | −175,85 dB |

Las uniones al note-off tienen diferencias de posición/velocidad cero.
El RMS final mide los últimos 20 ms de presión frente a 150–800 ms de
sostenido. En las cuatro notas pasa el criterio de −60 dB; el archivo
termina después de esa cola y de su propagación, sin fade de edición.
Cada proceso tardó 475–497 s, incluyendo ambas resoluciones, carga,
almacenamiento y controles; los cuatro se ejecutaron simultáneamente.

WAV único: `C:/SHARED/Fisica-pendiente-2026-10-05/P6-release/Arpegio-control-release.wav`,
9,103479 s, estéreo float32 48 kHz, ganancia común 0,25 full scale/Pa,
pico 0,516407. SHA256
`4deae73fcf2aa4f0f5790db8b4e5310de65a0a8a6f185fba09aa68ed8b199f3d`.
Se reconstruye la presión desde los ataques/sostenidos originales más las
colas, en vez de anexar audio a WAVs ya cortados. No hay EQ ni cambio de
micrófonos. Las colas se suman con el siguiente ataque separado 1,2 s:
esa suma de notas independientes no constituye una tabla compartida.

Reproducción: `render-concert-grand-physical-release.py --note <nota>`
con asset/factor/kernel originales, resumen de contacto como
`--baseline-report`, informe del sostenido como `--parent-report`, CSVs
de esa altura y estados finales como `--state-64`/`--state-128`.
Después `assemble-concert-grand-release-phrase.py --shared <raíz-2026-10-05>
--reports <este-directorio> --audio <destino-WAV> --report <destino-JSON>`.
Elegir destinos nuevos; las herramientas verifican hashes/gates y no
sobrescriben mediciones. Los diagnósticos reproducibles son
`audit-concert-grand-phrase-colour.py`, `audit-concert-grand-board-bandwidth.py`
y `audit-concert-grand-low-balance.py`.

## Criterios antes de otra escucha del modelo completo

1. Cubrir la respuesta del puente y presión hasta 6 kHz con base estructural
   ampliada y refinamiento espacial. No llamar convergida a una comparación
   que sólo refina tiempo o captura del mismo sistema truncado.
2. Construir/verificar la carga del aire recíproca de esa base. No agregar
   modos altos sin pagar su trabajo acústico, ni aplicar una corrección dry
   sobre el WAV para presentarla como nueva física.
3. Revisar ataque y evolución armónica en C4/E4/G4/C5, comparando pérdidas
   internas de cuerda, contacto y tabla por separado. Una diferencia de
   energía integrada no identifica por sí sola la causa física.
4. Conservar el note-off continuo y exigir cola final al menos 60 dB bajo
   el RMS de sostenido antes de terminar un archivo sin edición audible.
5. Después: unísono y varias notas sobre la misma tabla/aire, intensidades
   distintas y pedal. Este arpegio frío no equivale a esa polifonía.

Los informes y fuentes tienen única copia en el repo. Datos nuevos únicos
en `C:/SHARED/Fisica-pendiente-2026-10-05/P6-release`. El WAV nuevo es sólo
un **control de release**, no una solución al brillo ni un candidato aceptado.
