# Modelo 14B: ataque finito al volver a tocar una cuerda

Prototipo para escuchar. Continúa el modelo 14; no modifica el plugin instalado.

## Diagnóstico de los pasajes señalados

- Nocturno, 4,005 s: G5, MIDI 79, velocidad 78. La misma nota sonó 2,344 s antes; una repetición puede encontrar una cuerda viva sin ser rápida.
- La Campanella, bajada alrededor de 2:27: contiene notas del registro alto que ya sonaron y otras repetidas a intervalos cortos. Muchas cuerdas agudas no tienen apagador.

Se renderizaron contrafactuales desde el inicio de los MIDI, conservando pedal, eventos y estado previo a cada fragmento. Quitar KNOCK_LEVEL/THUMP_BASE apenas cambia los picos por encima de 8 kHz; quitar CONTACT_BOARD_GAIN tampoco los explica. Repartir la excitación del golpe sí los modifica. Esto identifica un problema en la reconstrucción de nuevos ataques sobre voces vivas; no prueba que toda sensación de pick venga de ese mecanismo ni separa todos los ruidos del piano real.

## Corrección propuesta

La ruta anterior sustituye el estado complejo calculado del golpe por su magnitud con signo en la cuadratura de velocidad. Con las dos rampas compiladas a cero, lo entrega en una muestra. Desaparece la fase que distribuía el ataque entre parciales y cuerdas. El nuevo modo conserva ambas cuadraturas, el unísono y la contribución negativa inicial que suaviza el crecimiento. Solo la contribución nueva entra con una envolvente smoothstep; el movimiento que ya sonaba sigue evolucionando.

La variante seleccionada para audición usa RESTRIKE_PHASE_PRESERVE = 1 y MERGE_RAMP_S = 0,004 s, globalmente. Se conserva la configuración de tabla, pérdidas, fieltro, nivel mecánico y unísono del 14. No se añade EQ ni compresión. Se probaron 1,5 ms y 4 ms; el segundo reduce más el pico inicial de G5 en el pasaje señalado. **Los 4 ms son una calibración de audición, no una duración de contacto medida.**

Es una reconstrucción finita de estados modales. Todavía no simula el martillo entrando en contacto con las cuerdas inicializadas con su movimiento anterior. La rotación de la excitación en espera utiliza las pérdidas intrínsecas; no identifica un intercambio completo de energía martillo/cuerdas/tabla durante ese intervalo.

## Verificación

- 108 pruebas regulares y las dos huellas de audio heredadas pasan.
- La prueba analítica comprueba fase, magnitud y continuidad de una nueva contribución sobre un oscilador en movimiento, incluyendo fases negativas y distintas frecuencias/pérdidas.
- Con el modo nuevo desactivado, los dos fragmentos coinciden bit a bit con el 14.
- 16 notas aisladas sostenidas, C4/F4/G4/C5/G5/C6/G6/C7 a 60 y 124, coinciden bit a bit con el 14. No hay adelgazamiento de esas notas aisladas.
- En Nocturno, todo el audio anterior a la primera repetición que cambia coincide bit a bit.
- Dos secuencias de 529 eventos a 44,1/48 kHz y bloques de 256 frames producen salida finita y colas que decaen. No es una prueba de ASIO.
- Los MIDI completos conservan sus hashes de eventos y sus notas/tempos/controladores, con 10 s de cola.

verification.json contiene cambios de energía inicial por encima de 8 kHz en los ataques señalados. No es una puntuación perceptiva ni prueba que el pick haya desaparecido; hace falta escuchar el candidato. Al cambiar la fase, también cambia cómo se suma el nuevo golpe con el anterior: una diferencia de forma de onda no equivale por sí sola a pérdida de cuerpo.

## Audios

En C:\SHARED\Modelo-14B, estéreo 44,1 kHz/24 bit:

- Modelo-14B-nocturno-inicio.wav: primeros 9,5 s. La nota señalada sigue en 00:04.
- Modelo-14B-campanella-2m23s.wav: fragmento original de 2:23 a 2:33. El punto 2:27 está a los 4 s del fragmento.
- Modelo-14B-chopin-nocturno-op9-2.wav: pieza completa, 234,56 s con cola.
- Modelo-14B-liszt-la-campanella.wav: pieza completa, 285,16 s con cola.

Una ganancia común para las dos piezas y sus fragmentos, con la pequeña reducción adicional registrada en sus JSON para dejar el pico máximo en 0,85. No se normaliza cada pieza ni cada ataque por separado. Los archivos anteriores permanecen en C:\SHARED\Modelo-14.

## Reproducción

Binario/configuración congelados en local/concert-grand-coherent-unison/frozen-attack-14b. provenance.json registra hashes de fuentes, scripts y binario. research.patch conserva la investigación acumulada contra la carpeta de fuentes histórica especificada. El ajuste a las muestras y los coeficientes del modelo 14 no se vuelven a entrenar en esta revisión.
