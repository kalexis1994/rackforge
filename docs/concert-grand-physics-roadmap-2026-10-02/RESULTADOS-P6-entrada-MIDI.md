# P6 — Entrada musical y coste del banco

Fecha: 2026-10-11. Etapa de nota/release guardada en `08464306`.

## Qué falta para una pieza

La nota completa ya tiene martillo, cuerdas, tabla/aire compartidos y
apagador. Una pieza añade cobertura de alturas, control musical continuo,
historia entre eventos y un coste mucho mayor. No exigir identificar todos
los parámetros para una escucha diagnóstica; tampoco llamar a esa escucha
una reproducción identificada del Steinway.

| Trabajo | Estado de esta etapa | Criterio para continuar |
| --- | --- | --- |
| Leer notas, velocidades y tempo del MIDI real | Implementado y cotejado con otro parser | Ningún evento perdido; error de reloj ≤ medio tick |
| Mantener la misma cuerda/tabla/aire entre notas y bloques | Implementado en el reproductor | Estados idénticos al productor anterior con repetición, pedal y release |
| Captura mecánica por bloques | Implementada y probada | Buffer acotado, sin reiniciar historia ni duplicar la frontera |
| Banco con las alturas del fragmento | Constructor de registro pequeño implementado | Reproduce matrices/puertos anteriores; presupuesto impide banco denso gigante |
| Presión retardada y resampleo en bloques | Pendiente | Conservar cola de convolución y estado del resampler; cotejar con reconstrucción monolítica |
| Sustain continuo CC64 | Pendiente | Conservar valores originales y modelar engagement pasivo; no ocultar conversión binaria |
| Pedal suave CC67 | Pendiente | Acción/contactos explícitos y trabajo de sus cambios; no reducir volumen como sustituto tácito |
| Coste por segundo musical | Insuficiente en el ensayo preciso | Medir el fragmento/banco, acelerar sin aprobar precisión por conservación de energía |
| Teclado completo | Pendiente para pieza completa | Eliminar matrices mecánicas globales densas; resolver escalado de cuerdas y registro grave |

## Demanda de los archivos preservados

Informe primario: `medidas-P6-entrada-MIDI-2026-10-11/plan.json`.
No se copian los MIDI: se leen en su carpeta original de `piano-comparacion`.
Hashes coinciden con los registros de Modelo-14B. El archivo de cierre
`verificacion-plan.json` usa el parser estándar independiente
`analyse-performance-midi.py`: coinciden todas las notas/controladores/programas
de ambos archivos, incluidos note-on de velocidad cero como note-off.

| Pieza | Duración MIDI | Golpes | Alturas distintas | Teclas simultáneamente sostenidas | Alturas retenidas, proxy binario |
| --- | ---: | ---: | ---: | ---: | ---: |
| Nocturno Op.9 No.2 | 224,563802 s | 1498 | 70 | 6 | 43 |
| La Campanella | 275,158854 s | 4249 | 75 | 8 | 47 |

Ambos archivos usan canal 0. Hay CC64 con valores intermedios y CC67 activo.
El Nocturno incluye CC66=0, sin sostenuto activo. El programa 0 se conserva
en el plan; el modelo SI sigue teniendo un único juego de parámetros.

El máximo error de cuantización del plan es **1,302083e−6 s**, dentro del
medio tick a 256 kHz (1,953125e−6 s). Frente al parser independiente, máximo
**1,302085e−6 s**, incluida su acumulación de tiempo en flotante. El plan
mantiene el orden original cuando varios eventos caen en el mismo tick.
El proxy de voces usa CC64 binario y contadores de teclas; no estima la
energía física de las colas ni el medio pedal.

## Memoria y tiempo

El constructor actual crea matrices densas del núcleo mecánico, además
de factores y memoria acústica. Con tres cuerdas y 96/192 modos por cuerda:

| Pieza | Coordenadas del núcleo | Una matriz float64 | Sólo B/V monolíticos a 128 kHz |
| --- | ---: | ---: | ---: |
| Nocturno | 81.246 | 49,18 GiB | 259,56 GiB |
| La Campanella | 87.006 | 56,40 GiB | 318,04 GiB |

Una matriz no es el pico de RAM: se construyen varias. Guardar sólo las
alturas efectivamente pulsadas también omite otras cuerdas simpáticas;
no es por sí solo el piano completo. No reservar esos bancos en esta etapa.

La trayectoria precisa anterior tarda 1908,177 s para 0,4 s físicos:
**4770 segundos de cálculo por segundo de audio**. Extrapolar ese caso de
tres teclas da **12,40 / 15,19 días** para esas duraciones. Son extrapolaciones,
no predicciones del teclado completo, que cambia el coste. No prometer una
pieza de varios minutos en horas con este método sin otra medida.

## Reproductor y controles

`concert_grand_performance.py` utiliza los subflujos existentes. Cada
relanzamiento conserva exactamente cuerdas, tabla y estados acústicos.
La historia de tecla/pedal persiste entre bloques. El descenso del apagador
conserva la rampa anterior de 30 ms. Retries por recontacto no aceptan un
estado ni consumen tiempo antes de recalcular las dos mitades disipativas.

El callback recibe B/V Hermite uniformes a 128 kHz por bloque, con una
frontera compartida. Se retiene sólo el último nudo al pasar al siguiente;
el consumidor debe evitar duplicar esa muestra. No reconstruir presión
mediante FFTs aisladas por bloque y concatenarlas: aún falta validar la
convolución y resampleo continuos, incluida su cola y el silencio inicial.

El reproductor falla antes de lanzar notas si falta una altura, hay más de
un canal o se piden mecanismos no implementados. CC64 intermedio requiere
una opción explícita de diagnóstico binario; programas requieren una opción
explícita de parámetros congelados. CC66/67 activos no se omiten. Es una
cola de eventos del modelo, no un plugin ni un lector de cualquier mensaje
MIDI: antes de usar el plan se deben revisar todos sus flags de mecanismos.

Cuatro pruebas de entrada/reproducción pasan en **19,979 s**: tempo entre
tracks, velocidades cero, valores originales, eventos entre ticks de coast,
repetición, cinco bloques y release. Los arrays finales son **idénticos**
al productor congelado en el fixture; calor de apagador y contadores también.
Tasas iguales conservan la ruta de coast después de separación, corrigiendo
la selección que mandaba toda la nota al solver de contacto. No se modifica
el productor anterior ni su evidencia.

Una quinta prueba pasa en **0,953 s**: el constructor nuevo con C4/G4/C5
produce mass/K/transform/damping y contactos **idénticos** al anterior.
Construye las cuatro nuevas alturas y rechaza un banco gigante antes de
reservar sus matrices. El límite de 512 MiB es para **una** matriz, no para
la RAM completa de la ejecución.

## Primer fragmento y ensayo presente

Los primeros diez segundos de La Campanella usan sólo **63,75,87,99**:
D#4/D#5/D#6/D#7. Se puede empezar por ese registro sin ampliar a las 75
alturas de toda la obra. El Nocturno ya usa 25 alturas en sus primeros diez
segundos, por lo que no es el fragmento inicial más barato con este constructor.

`measure-concert-grand-musical-register.py` construye ese banco sobre la
tabla y reducción acústica existentes y mide un ataque de 6 ms, con el
primer pitch/velocidad originales (63/28). Usa sustain levantando apagadores
como el CC64=127 inicial. El **CC67=127 original no se aplica** en esta sonda:
es un control de coste con fieltro congelado y está declarado en el informe,
no se presenta como interpretación fiel del fragmento.

Terminado: informe `medidas-P6-entrada-MIDI-2026-10-11/registro.json`;
autenticación y aritmética en `verificacion-plan.json`. Banco de **5218
coordenadas mecánicas**, doce cuerdas y cuatro martillos. Construcción
**11,226 s**; sonda **42,516 s**; preparación, factores y controles incluidos
**127,308 s**. Una matriz del núcleo ocupa **217.486.368 bytes**; es el dato
de una matriz, no el pico de RAM ni una certificación de todo el teclado.

700 pasos de contacto y 418 de coast: **700 + 2×418 = 1536 ticks** a
256 kHz, sin recontactos rechazados. Tres bloques de captura, 768 intervalos
totales, máximo **257 nudos / 2.493.928 bytes nominales** de buffer B/V/t.
Spline/temporales y los estados físicos son adicionales. Valores B/V y estado
final finitos; una sola puesta en movimiento, con pitch/velocidad 63/28.
Trabajo externo **0,000287838609 J**; cierre mecánico, fieltro, material y
puerto acústico: **−8,08648e−15 J / 2,80938e−11 relativo**. No hay pérdida
de apagador porque el sustain abre los cuatro. Fuentes, tabla y R sin cambios.
El cierre independiente comprueba hashes y aritmética; no hay arrays de
estado guardados para otra reproducción, ni oracle de presión de esta sonda.
No extrapolar 6 ms de ataque como medida estable del coste de diez segundos.

Sin WAV, sin nueva R, sin cápsulas/historias guardadas. Coeficientes y
ley de cuerda se mantienen supuestos, con tres cuerdas por altura;
no extrapolar esto como identificación de graves entorchados o acción real.
La base acústica sigue siendo la anterior y su gate espacial fallido no cambia.

## Siguiente paso

Completar observación de presión y resampleo continuos, con un cotejo contra
la reconstrucción monolítica del mismo estado vivo. Luego conectar CC64/67
con supuestos declarados y evaluar un fragmento corto del registro de cuatro
notas. Medir coste antes de aumentar duración. Mantener pendientes los fallos
temporales/Hann de la cola y los espaciales, sin usarlos como bloqueo absoluto
de una escucha diagnóstica ni convertirla en aprobación del modelo completo.
