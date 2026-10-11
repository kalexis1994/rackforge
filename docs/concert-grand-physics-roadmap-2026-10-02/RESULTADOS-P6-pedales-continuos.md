# P6 — Sustain continuo y desplazamiento lateral del martillo

Fecha: 2026-10-11. Etapa anterior guardada en `983552b7`.

## Objetivo y alcance

Conectar los valores originales CC64/CC67 con acciones explícitas en el piloto
físico compartido, manteniendo cuerdas, tabla y aire vivos. No cambia el
plugin instalado. Los valores MIDI se conservan; los tiempos y la geometría
del mecanismo son hipótesis del prototipo, no parámetros identificados.

En un piano de cola el pedal suave desplaza lateralmente la acción, y un
martillo puede pasar de tres cuerdas a dos. Fuente primaria:
[Yamaha — How to Use Keyboard Pedals](https://hub.yamaha.com/keyboards/k-how-to/how-to-use-keyboard-pedals/).
Ese mecanismo motiva el control; Yamaha no proporciona los coeficientes,
tiempos ni geometría de solapamiento empleados en este ensayo.

## CC64: soporte del apagador y precarga continua

`PedalControl` convierte el valor íntegro en una posición objetivo `CC64/127`.
El soporte recorre una trayectoria smoothstep de 30 ms. Al recibir otro valor,
continúa desde la posición actual; repetir el objetivo no reinicia el viaje.
Cada tramo tiene pendiente cero en sus extremos. Si se interrumpe en movimiento
se reinicia la velocidad prescrita: no se modela la inercia del mecanismo.

La tecla sostenida mantiene su apagador abierto. Al soltarla, el retorno de
30 ms determina la penetración normalizada:

`penetración = max(0, retorno_tecla − altura_soporte_pedal)`

La intensidad del parche viscoso existente se multiplica por
`penetración^1,5`. Es no negativa y se integra mediante el mismo subflujo
disipativo pasivo. CC63 y CC64 no tienen un salto binario. El exponente y
los tiempos son supuestos; no se identifica un umbral real de medio pedal.
Los note-off solapados respetan el conteo de teclas, y un note-off redundante
no reabre ni reinicia el apagador.

Se conservan las pérdidas positivas calculadas por el parche y la transmisión
recíproca por su mapa de velocidad. No se añadió resorte conservativo del
apagador, masa vertical o ruido del pedal. El trabajo de levantar físicamente
esa masa no forma parte del estado de este prototipo. La variación del
coeficiente disipativo no cambia una energía elástica almacenada.

## CC67: área de contacto lateral, con trabajo de parámetros

`ShiftedHammers` conserva las tres cuerdas por altura, sus afinaciones,
la masa del martillo, velocidad de lanzamiento, geometría longitudinal y
ley no lineal del fieltro. El frente lateral supuesto cubre tres tiras de
un paso; su desplazamiento máximo es un paso. El solapamiento geométrico
produce áreas relativas `[1−u,1,1]`, desde tres contactos hasta dos.
La rigidez de cada contacto se multiplica por su área, sin redistribuir
la rigidez de la cuerda excluida ni alterar la ganancia del audio.

La primera cuerda sigue vibrando y acoplándose al puente aunque su área de
impacto sea cero. Los contactos inactivos se excluyen del test de compresión
y de la posición inicial del martillo al reiterar. No se reinician los
estados de cuerdas, tabla, cabeza o memoria acústica al mover el pedal.

El cambio de área con una compresión existente cambia la energía potencial:

`W_param = Σ (k_nuevo−k_anterior) max(δ,0)^(p+1)/(p+1)`

Se registra ese trabajo **con signo**, separado del lanzamiento y del calor.
La acción prescrita se evalúa al punto medio y al final del paso, con cambios
a coordenadas fijas antes/después de integrar el contacto. El balance es:

`H_final − W_lanzamientos − W_param + Q_fieltro + Q_material + Q_apagadores + W_puerto_aire`.

Esto no demuestra pasividad del actuador externo ni un certificado total de
energía del piano: el desplazamiento se prescribe, no se integra su masa.
La suma exacta de trabajo de parámetros no demuestra convergencia temporal.

**Caché de fuerzas:** el solver anterior copiaba las rigideces al construir
el contacto. `ShiftedContact` las actualiza según la revisión del área;
las direcciones y factores mecánicos/acústicos comunes se reutilizan.
`PedalControl.bind` rechaza el solver anterior para impedir una acción
visualmente móvil con fuerzas congeladas. El reproductor sin control nuevo
conserva la trayectoria binaria anterior como referencia.

Este banco sólo admite tres cuerdas por tecla. Quedan pendientes geometría
de coros de una/dos cuerdas, graves entorchados, variación lateral de dureza
y surcos del fieltro, fuerzas tangenciales, inercia de acción y registro real
sin apagadores en el extremo agudo. No se implementa sostenuto activo.
La selección de programa sigue siendo explícita para el único programa físico.

## Pruebas y ensayo

Seis pruebas nuevas verifican continuidad de posición/reorientación, valores
intermedios, retorno/solapamiento de teclas, solapamiento geométrico, trabajo
de área reversible a compresión fija, caché de fuerzas, exclusión de cuerda
inactiva y reducción del pre-roll frío. La trayectoria de dos teclas incluye
movimiento durante contacto y medios pedales: cambia el tamaño del buffer
sin cambiar ningún estado final ni pérdidas/trabajo.

Dos regresiones adicionales contrastan el reproductor anterior con su
trayectoria congelada y la escritura WAV en archivos temporales. Las **ocho
pruebas pasan en 41,522 s**, mientras se ejecutaba en paralelo el ensayo
grande; no es una medida de rendimiento exclusivo del solver. El lector WAV
advierte sobre un chunk no-data de libsndfile; formato, duración y muestras
se comprueban y pasan. Las dos sondas del banco de 606 modos también pasan
los controles locales; no son una validación global ni una escucha musical.

Productor: `tools/measure-concert-grand-pedal-action.py`. Única carpeta de
evidencia: `medidas-P6-pedales-continuos-2026-10-11/`.
Se reutiliza el mismo banco D#4/D#5/D#6/D#7, sus doce cuerdas y los 606 modos
de tabla, junto a la única R y reducción previas. Dos sondas de 6 ms:

1. Primer ataque 63/28, con todos los controladores originales anteriores al
   ataque. Se resuelve analíticamente el silencio inicial frío: las posiciones
   de pedal avanzan por sus eventos reales y las coordenadas mecánicas/aire
   permanecen en reposo. No es un recorte de una cola o un estado resonante.
2. Mismo ataque y contexto, con dos eventos sintéticos para medir movimiento
   de CC67 durante contacto y retorno de CC64 a 64. Es un diagnóstico del
   ledger, no una interpretación de la pieza original.

No se entrega un WAV de 6 ms como escucha musical. Se guardan sólo presión
contraída y audio derivado para verificación, sin otra historia B/V o R.

| Medida | Pedales originales estables | Movimiento diagnóstico |
| --- | ---: | ---: |
| Trayectoria de 6 ms, segundos de cálculo | 44,956 | 46,914 |
| Pasos contacto / coast | 792 / 372 | 790 / 373 |
| Trabajo de lanzamiento, J | 2,87838609e−4 | 2,87838609e−4 |
| Trabajo lateral con signo, J | 0 | +1,54472077e−6 |
| Calor de apagadores, J | 0 | 2,83660e−13 |
| Residuo mecánico, J | −3,05560e−14 | −3,06281e−14 |
| Residuo relativo | 1,06157e−10 | 1,05839e−10 |
| Criterio local de trabajo | ≤ 1e−8: pasa | ≤ 1e−8: pasa |
| Pico de audio a ganancia fija | 0,0139886 | 0,0140360 |

En ambos casos el reloj cierra en **1536 ticks**, sin retries, y los tres
bloques capturan 768 intervalos. El pre-roll frío dura 1,026043 s del MIDI
original y se resuelve sin integrar estados nulos. En el caso estable los
dos pedales han completado su viaje antes del primer ataque: CC64=127 y
CC67=127. El movimiento de CC67 en el segundo caso termina en u=0,899969;
los apagadores no sostenidos tienen engagement 0,00374249. Esa pequeña
pérdida no mide una cola de release completa: son sólo 6 ms con la tecla
atacada aún pulsada.

Total de la ejecución completa con preparación y hashes: **182,43 s**.
Hay un intento anterior incompleto por serializar un `numpy.bool` en JSON;
se conserva su hash y el motivo en el informe. Se repitió tras convertir
explícitamente el escalar, sin cambiar la física ni duplicar informes.

La presión y el audio contraídos ocupan **50 167 bytes** en `presion.npz`.
`tools/verify-concert-grand-pedal-action.py` comprueba independientemente
hashes de fuentes/plan/MIDI/archivo, reloj, formato/finitud, resampleo SciPy,
pico/RMS, pérdidas no negativas y aritmética del trabajo con signo. Las dos
sondas pasan; `verificacion.json` conserva alcance y resultados. No vuelve
a reconstruir estados modales ni prueba la convergencia temporal de la
acción lateral. El productor autentica que la única R/tabla siguen iguales.

## Siguiente paso

Renderizar un motivo corto del MIDI original con sus retornos de tecla y
pedales completos. Presupuestar tiempo usando ataque y coast; la salida y
los pedales ya están conectados, pero persiste el coste de integración.
Para una pieza entera sigue pendiente sustituir las matrices mecánicas
globales densas. Mantener visibles el fallo espacial de 9,2021 %, los fallos
temporales/Hann de cola y el último control de malla espectral, sin aprobar
el modelo completo por cerrar el ledger local. Audios de escucha sólo SHARED.

Las evidencias anteriores conservan hashes de las fuentes medidas antes de
esta extensión: sus productores/verificador se reproducen desde `983552b7`.
No se reescriben esos resultados históricos con hashes nuevos.
