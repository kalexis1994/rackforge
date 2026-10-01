# Modelo 14: calibración del cuerpo sostenido

Prototipo para escucha, 30 de septiembre de 2026. Continúa la revisión del timbre acumulado tras el rechazo del modelo 13. No reemplaza el plugin instalado ni se selecciona todavía para producción.

## Hallazgo en los controles efectivos

La tabla usa `Controls::board_loss()`, que convierte el parámetro público 31, Board Damping, de porcentaje a factor de pérdida. En Concert Grand su valor inicial es 2,3 %. `BOARD_LOSS_FACTOR = 0.046`, utilizado en búsquedas anteriores, no fija ese valor: aparece en la normalización del incremento de pérdidas de los modos graves. Cambiarlo puede alterar la zona baja, pero no realiza el cambio global de amortiguamiento que su nombre y documentación antigua sugerían.

La búsqueda actual distingue los parámetros efectivos del programa de los coeficientes de investigación. El renderizador de audiciones ahora también acepta ambos conjuntos por variante y registra `params_by_profile`, incluyendo la firma del render largo. No se modificó el motor de producción.

## Diagnóstico y ajuste

Primero se separaron pérdidas internas y carga del puente mediante ablaciones. Quitar la parte viscosa o su factor de impedancia apenas modifica el fundamental sostenido en los casos inspeccionados. Reducir la carga del puente sí lo modifica fuertemente. Estas ablaciones son diagnósticas: no se selecciona una variante que quite disipación conservando radiación.

Se probaron configuraciones globales de movilidad, formas de modos, densidad, amortiguamiento efectivo, afinación, rigidez, respuesta reactiva y diferencias de pérdidas entre cuerdas. Todas las variantes candidatas conservan la misma movilidad de tabla para calcular carga y radiación. No hay ganancias, ecualización ni coeficientes de pérdida distintos para notas concretas.

El objetivo separa energía próxima a H1–H6, equilibrio entre parciales y sus caídas en 50–200 / 200–500 / 500–1500 ms. Así, un pulso grave de ruido no satisface un fundamental tonal ausente. Se conserva tanto el promedio como la peor combinación nota/intensidad. Cuando una nota suave solo tiene un parcial retenido, no se inventa una medida de equilibrio armónico. Las muestras originales están editadas, niveladas y tratadas con reducción de ruido; los cocientes de envolvente son medidas del audio, no identificación directa de materiales o fuerzas.

## Configuración de escucha

| Propiedad | Modelo 14 |
| --- | --- |
| Amortiguamiento principal efectivo de tabla | 4,6 %; parámetro 31 |
| Densidad modal | 1,0; conservada |
| Movilidad media | 0,0005 m/(N·s); conservada |
| Afinación coherente por cuerda | activada |
| Ancho de unísono | 1,0; ley original de registro, con retención en agudos |
| Dispersión de rigidez | 0,005 |
| Carga reactiva del puente | activada |
| Desafinación de polarización | 0 |
| Pérdidas adicionales efectivas por cuerda | 0 / 2,0 / 3,4 s⁻¹ |
| Distancia de micrófono | 2,64 m |

El unísono tiene diferencias de afinación; no se vuelve a colapsar a frecuencias idénticas. Las pérdidas adicionales son contracciones diagonales positivas, que no añaden energía. Son coeficientes efectivos de este modelo reducido: **no representan mediciones de imperfecciones pequeñas del alambre ni identifican una causa física única**. La aproximación actual puede estar absorbiendo mecanismos de pérdidas o acoplamiento que todavía no representa de forma explícita. Se conserva el contacto anterior; el contacto triple del 13 no forma parte de esta selección.

Las variantes con unísono y pérdidas idénticas mejoraban el error corto, pero recuperaban mínimos recurrentes en las colas largas. Se descartaron. También se descartó aumentar solo la conversión de presión de la tabla: empeoraba la peor nota. La variante seleccionada conserva la conversión de presión 3 y utiliza una sola ganancia general de exportación para compensar la diferencia mediana de nivel inicial sobre el conjunto de ajuste, manteniendo el margen de la audición C. No se normalizan notas ni intensidades por separado.

## Resultados y límites

Ajuste: 14 pares, C4/F♯4/C5/F♯5/C6/F♯6/C7 a 60/124. Comprobación interna: 21 pares, D♯4/A4/D♯5/A5/D♯6/A6/D♯7 a 36/84/108. Todo pertenece al corpus histórico BitKlavier stage; no es una prueba ciega nueva.

| Métrica diagnóstica | C previo | 14 |
| --- | ---: | ---: |
| Error compuesto en ajuste | 7,294 | 5,511 |
| Peor par de ajuste | 9,920 | 7,768 |
| Error compuesto en comprobación interna | 7,643 | 5,597 |
| Peor par de comprobación interna | 15,398 | 7,268 |

Son errores compuestos de diferencias en dB, no porcentajes de fidelidad. No todos los pares mejoran; los datos completos conservan las regresiones, incluyendo casos del registro superior. El objetivo no representa toda la percepción de timbre.

Caída de H1 fuerte en 0,5–1,5 s respecto a 0,05–0,2 s:

| Nota | C previo | 14 | Original stage |
| --- | ---: | ---: | ---: |
| C4 | −35,74 dB | −23,45 dB | −8,17 dB |
| C5 | −4,77 dB | −9,87 dB | −11,04 dB |
| C6 | −27,21 dB | −23,49 dB | −22,11 dB |
| C7 | −28,92 dB | −23,85 dB | −21,70 dB |

La diferencia residual de C4 sigue siendo grande en este indicador; no se declara resuelto el cuerpo de todo el teclado. Tampoco se conoce de forma independiente el instante de liberación de cada muestra original: las medidas no deben convertirse sin más en una constante física de pérdida.

Con ocho notas sostenidas 12 s a velocidad 124, el análisis de hasta 16 parciales hasta 10 kHz, a la ganancia final de escucha, cuenta mínimos recuperados de al menos 3 dB por intervalos:

| Variante | 0,1–0,5 s | 0,5–1,5 s | 1,5–3,5 s | 3,5–7,5 s | 7,5–11,5 s |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 7 | 4 | 0 | 0 | 0 |
| 14 | 68 | 26 | 0 | 0 | 0 |

Hay más movimiento inicial, conservando un asentamiento posterior en esta prueba. Los mínimos son indicadores; no equivalen a defectos audibles independientes ni prueban que desapareció la sensación de guitarra. El análisis usa una máscara común y límites de nivel; no se cuentan fluctuaciones arbitrariamente débiles. Los ocho renders nativos finales coinciden bit a bit con los usados para esta comprobación.

Se verificaron dos secuencias de 529 eventos a 44,1 y 48 kHz, bloques de 256 frames, con acordes repetidos y posiciones intermedias del sustain: salida finita, picos nativos 0,06271/0,06272 y caída de potencia tardía superior a 119 dB respecto a la sección activa. No es una prueba del driver ASIO. El binario no cambió respecto al prototipo ya verificado con 107 pruebas regulares y dos huellas de audio heredadas; estas verifican integridad, no calidad perceptiva.

Persisten el puente de frecuencia congelada, la polarización horizontal agregada, la normalización por golpe y la continuación analítica del ataque superior. Hace falta una identificación física más completa para atribuir los coeficientes a mecanismos reales. El modelo 14 debe juzgarse por escucha antes de integrarlo o trasladar su calibración a todos los programas.

## Audios

En `C:\SHARED\Modelo-14`:

- `Modelo-14-agudos-intensidades.wav`: C5/G5/C6/G6/C7 a velocidades 60, 84 y 124; 66 s.
- `Modelo-14-medios-intensidades.wav`: C4/F4/G4/C5 con las mismas intensidades; 52,8 s.
- `Modelo-14-agudos-sostenidas-12s.wav`: cinco agudos fuertes sostenidos 12 s y 2 s después de note-off; 72 s.

Las intensidades mantienen cada nota 2,5 s y conservan 1,5 s después de un note-off real; siguen 0,4 s de separación. Las teclas sin apagador continúan su caída natural. Canal izquierdo nativo, 44,1 kHz/24 bit, sin cambios de estéreo que confundan la comparación tonal. Ganancia fija de 26,49184 dB para todos los fragmentos; sin trim adicional ni recorte. Cada WAV tiene JSON de secuencia, ganancia y hashes.

## Reproducción

Binario congelado en `local/concert-grand-coherent-unison/frozen-sustain-14`, SHA-256 `e0201aa2a39b844717daf6b963bfc144c2079498e3b92662a3d5b0750e242d2d`. `model.json` fija los controles. El ajuste se reproduce con `tools/fit-concert-grand-sustained-body.py`; las audiciones con `tools/render-concert-grand-unison-audition.py`; el análisis de colas con `tools/audit-concert-grand-retained-unison.py`. `render-final.py` conserva el procedimiento de las notas con liberación real y las secuencias de estrés. `provenance.json` registra scripts, configuración y procedencia; las matrices completas de búsquedas quedan bajo `sustain-*` en el prototipo.


## Piezas MIDI con el modelo 14

Renderizadas con el mismo binario y configuración congelados del modelo 14:

- `Modelo-14-chopin-nocturno-op9-2.wav`: 234,56 s, 1498 ataques.
- `Modelo-14-liszt-la-campanella.wav`: 285,16 s, 4249 ataques.

En `C:\SHARED\Modelo-14`, estéreo 44,1 kHz/24 bit. Se conservan todos los mensajes de canal, incluidos los controladores de pedal y las liberaciones, el mapa de tempo y 10 s de cola. Una misma ganancia de exportación para ambas piezas, con reducción común adicional registrada en sus JSON para mantener el pico máximo en 0,85. No hay ecualización ni compresión añadidas. Los JSON registran hashes del MIDI, eventos, modelo, binario, audio nativo y WAV; las notas quedan balanceadas al terminar los MIDI.
