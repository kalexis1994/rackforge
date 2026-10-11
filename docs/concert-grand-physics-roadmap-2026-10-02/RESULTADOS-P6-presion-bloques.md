# P6 — Presión y WAV continuos por bloques

Fecha: 2026-10-11. La entrada MIDI/banco musical queda guardada en `fd3344bf`.

## Propósito

Cerrar la salida de audio del reproductor vivo sin almacenar B/V de toda una
pieza ni concatenar FFTs independientes. Se mantienen el Green de las celdas,
las 606 coordenadas de tabla, la ganancia de 0,25 por Pa y los estados físicos
compartidos. La trayectoria no recibe EQ, envolventes ni otra realimentación.

## Integración de la aceleración

En cada intervalo uniforme h, la transformación continua Hermite anterior es:

`A(f) = DFT(ΔV) J0 + DFT(C) 12(0,5 J0 − J1)`

con `C = ΔB/h − (V0+V1)/2`, `J0 = ∫ exp(−i2πfhx) dx` y
`J1 = ∫ x exp(−i2πfhx) dx`, integrales en x de 0 a 1.
La fuente sigue siendo aceleración física; no se diferencia un desplazamiento
con ventana ni se añaden impulsos al terminar el bloque.

La versión acotada integra exactamente en racionales los polinomios de
Lagrange en **12 nodos enteros −5…6**. Resultan dos filtros finitos: integrales
de L_n y de `12(0,5−x)L_n`. Aproximan los dos momentos continuos, con error
medido. No son nuevos modos de caja, un cambio del fieltro ni una selección
de armónicos. Todos los modos de tabla conservan la misma transferencia Green.

La propagación del kernel debe tener suficientes muestras iniciales nulas
para absorber los nodos de anticipación; se rechazan kernels demasiado cortos.
La cola finita de interpolación se conserva completa y se declara su extensión.
No se cambia el reloj del audio para alinear artificialmente una comparación.

## Convolución continua

`HermitePressureStream` contrae los 606 modos antes de guardar la cola de
overlap-add. Memoria entre bloques: dos canales de presión, una frontera B/V
y la transferencia FFT del bloque. La transferencia se reutiliza con tamaño
fijo; las entradas largas se subdividen internamente. No se guarda una historia
modal de varios minutos. La frontera temporal y B/V se comprueban antes de avanzar.

`PolyphaseStream` conserva fase e historia de la FIR por toda la trayectoria.
Reproduce el filtro Kaiser y la compensación de retardo únicos de
`scipy.signal.resample_poly(...,3,8)` con padding constante. No hay padding ni
reinicio en fronteras internas. Se emiten exactamente los frames finales
`ceil(N×3/8)`, incluida la cola de conversión del último bloque.

`PhysicalWavStream` escribe directamente FLOAT32 estéreo a 48 kHz. Añade una
vez 100 ms de silencio, aplica una ganancia fija y rechaza audio no finito o
fuera del margen de pico. Al finalizar vacía presión y resampler y cierra
el encabezado. Si falla la trayectoria, `abort()` cierra el mismo archivo
parcial sin inventar una cola. No hay raw auxiliar, normalización o fade.

## Pruebas

Seis pruebas pasan en **0,081 s**: momentos polinómicos/quadratura continua,
overlap-add frente a convolución directa completa, bloques de tamaño desigual,
resampleo frente a SciPy con entradas de 1/2/7/17/100/2049/4097 frames, duración
y formato WAV, fronteras inválidas y contraste con el observador analítico.
El caso de fuente sintética en audio exige menos de 0,1 % de diferencia global.

Una prueba física adicional pasa en **6,641 s**: dos teclas con contacto,
repetición, note-off, pedal y cuatro bloques. Los estados finales son
**idénticos** al productor anterior; el WAV coincide con la convolución directa
del mismo trazado y la conversión completa. Los archivos de esa prueba son
temporales y no se copian a SHARED. El aviso del lector SciPy sobre un chunk
no-data de libsndfile no afecta muestras, tipo, tasa ni duración comprobados.

## Ensayo de 606 modos

Completado. Productor `measure-concert-grand-pressure-stream.py`;
informe `medidas-P6-presion-bloques-2026-10-11/presion.json`, vectores
contraídos en `presion.npz`. La misma sonda de ataque de 6 ms, cuatro alturas
y doce cuerdas, primero 63/28 y sustain abierto. CC67 no se aplica en esta
sonda de coste/observación: no representa el MIDI con pedal suave.

Se compara presión por bandas contra el espectro continuo del **mismo B/V**
y kernel, y WAV contra la reconstrucción anterior a ganancia/tasa iguales.
La ventana corta de B/V sólo existe en RAM para el oracle; no se guarda
otra cápsula, R o historia larga. El resampler y el escritor de WAV se
verifican también por separado para atribuir los errores de interpolación.

Resultados locales (errores relativos de norma; no cambio de ganancia):

| Control | Resultado | Criterio |
| --- | ---: | ---: |
| Presión total frente al observador continuo | 6,1136e−11 | Bandas ≤ 1 % |
| Presión 60–800 Hz | 2,4871e−15 | ≤ 1 % |
| Presión 800–2500 Hz | 7,7731e−14 | ≤ 1 % |
| Presión 2500–6000 Hz | 1,8935e−12 | ≤ 1 % |
| Presión 6000–12000 Hz | 8,1401e−9 | ≤ 1 % |
| Audio frente a FFT/resampleo completos | 1,1226e−7 (0,00001123 %) | ≤ 1 % |
| Momentos de fuente, 60–12000 Hz, máximo | 5,4585e−8 | ≤ 1e−6 |
| Cierre mecánico relativo del productor | 2,8094e−11 | Misma trayectoria previa |

Se comparan **455 frames comunes** a 48 kHz. La salida nueva conserva cinco
frames nativos adicionales de la interpolación finita (39,06 µs); produce
457 frames a 48 kHz. No se recorta esa cola ni se desplaza el audio para
reducir el error. Entre 12 y 24 kHz se registra un error máximo de momento
de 0,01548 %; no se extiende a esa banda el criterio de momentos de 1e−6.

La trayectoria tarda **43,94 s para 6 ms físicos**. La salida por bloques
consume 0,079 s durante las llamadas de captura; el total con preparación,
oracles y comprobaciones es **128,75 s**. Esto resuelve almacenamiento y
continuidad, pero no acelera la integración mecánica. La transferencia FFT
reutilizada ocupa 14,43 MB; la cola tiene 450 frames estéreo y el resampler
retiene 56 frames. La captura máxima es 257 nudos (2,49 MB nominales, sin
contar mecánica ni temporales). No se escribe otra historia modal o caché R.

`verify-concert-grand-pressure-stream.py` verifica hashes, forma/finitud,
DTFT directa, normas por bandas, resampleo SciPy y aritmética del ledger
a partir del único archivo contraído de **172 398 bytes**. Los valores
mecánicos, excepto tiempos, coinciden exactamente con la sonda previa.
La diferencia máxima de resampleo independiente es 1,388e−17.
El espectro analítico es el resultado archivado del observador congelado;
esta revisión no regenera otra trayectoria física ni vuelve a construir B/V.
Registro: `medidas-P6-presion-bloques-2026-10-11/verificacion.json`.

**Control de malla pendiente:** 12,5→6,25 Hz cambia la energía de la banda
800–2500 Hz **0,17854 %**, por encima del límite de 0,1 %. Las otras tres
bandas pasan energía; todas pasan el cambio de error absoluto de 1e−4.
No se declara convergencia de energía por bandas ni aprobación global.
Los controles locales de equivalencia/interpolación pasan en las mallas
medidas; el fallo de integración espectral queda separado y visible.

No se entrega audio de escucha de esta sonda de 6 ms: no constituye una nota
audible evaluable. El escritor se ejercitó en archivos temporales eliminados
al terminar las pruebas; esta etapa no deja nuevos WAV en SHARED.

## Alcance y siguiente paso

Este trabajo cierra la continuidad de salida con controles locales aprobados.
No demuestra convergencia temporal de una pieza, precisión global del operador
espacial ni identificación de parámetros del piano. Los fallos anteriores
Hann/cola y el 9,2021 % espacial permanecen registrados. No mejora el coste
de integrar cuerdas/tabla/aire.

Después: CC64 continuo y CC67 suave con una acción y trabajo explícitos,
manteniendo los valores originales del MIDI. Luego fragmento de La Campanella
con D#4/D#5/D#6/D#7, con presupuesto de tiempo medido y audio sólo en SHARED.
