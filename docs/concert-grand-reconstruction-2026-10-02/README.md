# Control de reconstrucción causal del Concert Grand

## Resultado

Se puede reconstruir la observación del modelo actual conservando el ataque
con una respuesta causal de su propia cadena. El control pasa los criterios
de ingeniería declarados en los 22 pares de ajuste, 20 alturas reservadas,
63 intensidades reservadas y 24 notas de escucha. No es una mejora del piano
ni una identificación del cuerpo o la fase del Steinway.

El ajuste libre de FIRs a las notas actuales no logró esa generalización.
La respuesta obtenida mediante excitación independiente de la cadena conocida
sí la logra, al incluir la señal auxiliar y el estado de las teclas. Por eso
no basta con cambiar el ajuste anterior de magnitudes por uno de fase libre.

## Qué faltaba en la observación anterior

La captura de 16 puntos del puente, después del bed, cubría el input de la
tabla. La cadena también tiene otra excitación, anterior al bed, que alimenta
longitudes no amortiguadas, registro abierto y halo. Esa señal no estaba
capturada por separado. Además, el motor silencia la alimentación de las
longitudes propias de una nota mientras su voz está activa. La observación
completa depende de esa máscara MIDI; no es un único filtro fijo de 16 inputs.

El nuevo harness guarda `auxiliary-drive.f32` además de los archivos anteriores.
Es exclusivamente `cfg(test)`: no añade capturas al plugin ni cambia su DSP.
Las 129 nuevas capturas reproducen **exactamente los bytes** del audio y de
los 16 inputs anteriores. La nueva señal utiliza unidades calibradas del motor,
sin atribuirles unidades de fuerza medidas en el instrumento real.

Se midió la respuesta impulsiva de la cadena nativa con:

- 16 impulsos independientes en los puntos del puente, después del bed.
- Un impulso independiente en la excitación auxiliar anterior al bed.
- La misma excitación auxiliar bajo cada una de las 88 máscaras de propietario
  MIDI del teclado; son compuertas existentes, no ganancias ni EQs ajustadas por nota.
- Programa Concert Grand, 44,1 kHz, warmup real de 200 ms; tabla, rim, sala,
  propagación, proximidad y sensibilidad de salida originales.
- Impulso de 10⁻⁶, con pico de salida menor que 10⁻⁶: se conserva el tramo lineal.
  Respuestas causales de 3 s, sin alinear ni normalizar las notas exportadas.

La sala/reflexiones tiene un nivel comparable al de la tabla en C6/124 desde
20 ms: en 20–50 ms, su energía izquierda está 0,357 dB por encima de la ruta
directa de tabla, antes de la propagación/proximidad del directo. Son energías
de componentes correlacionados; no equivalen a porcentajes aditivos de mezcla.
La traza vuelve a producir el WAV anterior exactamente. El dato ayuda a
entender por qué reducir toda la observación a color espectral puede perder
el desarrollo temporal.

El primer probe usado durante el desarrollo leyó spans de 256 frames, cuando
la planificación interna se escribe en spans de 128. Ese diagnóstico quedó
marcado `INVALID.txt`, se corrigió y se repitió antes de usar sus resultados.
Los kernels válidos están en `oracle-masks`; el error no afectó al renderer
normal, al plugin ni a las capturas musicales originales.

## Reconstrucción y límites

| Con máscaras nativas, respecto a nuestro audio | Error mediano de onda | P95 de onda | Error medio de forma del ataque |
| --- | ---: | ---: | ---: |
| Ajuste, 22 pares | −71,955 dB | −33,907 dB | 0,00554 dB |
| Alturas reservadas, 20 pares | −70,822 dB | −35,171 dB | 0,00130 dB |
| Intensidades reservadas, 63 pares | −71,664 dB | −34,133 dB | 0,00348 dB |
| Escucha, 24 notas completas | −44,761 dB | −33,442 dB | 0,00541 dB |

Error de onda = 10 log10(energía de la diferencia / energía del original),
sin desplazar muestras. Las tolerancias declaradas son criterios de control,
no umbrales de percepción. El peor conjunto sigue pasando los controles de
ataque, crecimiento y onda; no se afirma igualdad bit a bit de la convolución.

C6/124, en el audio completo de cuatro segundos:

- Crecimiento 0–5 → 5–20 ms: actual 3,557891 dB, control 3,557953 dB.
- Error de onda: −53,794 dB. Tiempo del máximo RMS inicial: igual.
- El cuerpo identificado anterior daba 0,918 dB de crecimiento y fue rechazado.

La duración finita del kernel, el cálculo f32 del motor frente a la
convolución offline y la máscara mantenida durante el release dejan residuos.
Las máscaras estáticas de este control corresponden a una tecla aislada;
una pieza arbitraria requiere conservar las compuertas nativas en el tiempo.
Los kernels no se ofrecen como un nuevo sampler para MIDI completo.

## Ajuste causal libre a las propias notas

Se añadió un operador de convolución causal con su adjunto para mínimos
cuadrados. La pérdida sólo observa el tramo capturado: no exige que el audio
se vuelva cero al terminar el archivo y no usa convolución circular.
Los registros tienen pesos iguales en la pérdida; esos pesos no se aplican
como ganancias por nota a la exportación. El escalado global de columnas se
deshace en los coeficientes. Los puntos nunca excitados quedan sin identificar.

| FIR aprendido de las notas actuales | Mediana de onda, ajuste | Mediana, alturas reservadas |
| --- | ---: | ---: |
| 3 regiones, 8192 taps, 120 iteraciones | −20,501 dB | −7,309 dB |
| 16 puntos, 32768 taps, 400 iteraciones | −27,128 dB | −8,840 dB |

Ambos alcanzan el límite de iteraciones del solver (código 7), sin confirmar
convergencia. Estos resultados no prueban imposibilidad de identificar un FIR
con notas ni un límite de la física. Revelan un problema de generalización y
condicionamiento del ajuste libre con este corpus. La respuesta conocida se
obtiene con excitación independiente y conserva información espacial, causal
y de compuertas que el ajuste libre todavía no identifica correctamente.

## Primer refit sobre la observación causal conservada

Después de pasar el control, se probó una corrección empírica relativa sobre
la respuesta causal original. Se conserva la excitación auxiliar bajo su
compuerta nativa. La fase mínima se elige sólo para la corrección relativa:
no sustituye la fase ni los retardos de la cadena completa. La corrección
no identifica propiedades de madera, fase real del Steinway ni nueva carga
energética sobre las cuerdas.

Se ajustan tres curvas regionales suaves de 64 nodos con regularización 3,
usando sólo las 22 notas de ajuste. Se limita la corrección relativa a ±12 dB.
Se prueban seis intensidades globales de corrección, de 0 a 1, en cuatro folds
por altura; ambas intensidades de cada altura quedan juntas. La elección se
congela antes de evaluar las reservas. No hay ganancia ajustada por nota.

| Intensidad relativa | Error medio CV | Peor par CV | Error medio del registro agudo |
| --- | ---: | ---: | ---: |
| 0, control | 5,587 | 9,090 | 6,736 |
| 0,125 | 5,545 | 9,361 | 6,828 |
| 0,25 | 5,514 | 9,717 | 6,941 |
| 0,5 | 5,496 | 10,461 | 7,186 |
| 0,75 | 5,549 | 11,241 | 7,476 |
| 1 | 5,653 | 12,055 | 7,773 |

Las seis pasan las guardas temporales. Se elige **0**, porque el objetivo
incluye el peor registro, la cola de errores y las regresiones individuales:
la mejor media no compensa empeorar los agudos y los casos más débiles.
No hay candidato nuevo aprobado. La carpeta de observación causal contiene
el control seleccionado, no un piano mejorado. El pequeño margen medio del
refit tampoco tiene una validación perceptiva.

El resultado limita este ensayo de corrección de magnitudes sobre una fase
conservada. El siguiente ajuste necesita estimar la evolución de las
resonancias y separar las pérdidas/carga de las cuerdas de la memoria del
cuerpo. La discrepancia temporal de C4 ya documentada es un diagnóstico útil,
sin atribuirla todavía a un único mecanismo. La respuesta causal conocida
queda como control de partida y las rutas auxiliares deben permanecer explícitas.

## Verificación y archivos

- Dos pruebas independientes del operador causal: convolución directa y
  propiedad adjunta; recuperación de filtros estéreo conocidos con retardos
  sobre excitación reservada. Aprobadas.
- Dos pruebas de la corrección relativa: identidad conserva el retardo y la
  rama auxiliar; ganancia constante modifica sólo la observación. Aprobadas.
- Las guardas temporales toleran 10⁻⁹ dB de redondeo, para no rechazar una
  convolución identidad por ruido numérico. No es tolerancia perceptiva.
- Probe nativo, captura que conserva audio y dos huellas nativas: aprobados.
  Huellas `232b085ed4248398` y `4371ab4adc1542ab` originales.
- Se comprobó la implementación rápida de observación contra las convoluciones
  independientes: diferencia absoluta menor que 10⁻¹².
- No se hizo commit, no se cambió la fábrica ni se reemplazó el `.rfplugin`.

En `C:/SHARED/Control-reconstruccion-2026-10-02`: `Actual-intensidades.wav`,
`Cadena-conocida-intensidades.wav`, notas individuales y `oracle-report.json`.
En `C:/SHARED/Observacion-causal-2026-10-02`: selección, coeficientes, informe
y secuencias del control seleccionado (intensidad relativa cero).

Ambas secuencias tienen F#2/C3/C4/C5/C6/F#6 a 28/60/92/124; 3 s por golpe.
Ganancia unidad, sin transposición, normalización por nota ni ruido añadido.

Captura SHA256: `73c48629a32ee96f4bd85410db6eedb3f25c46b08b219d94dab2e2181bf48d9b`.
Probe SHA256: `0c5860b8f33e0a70bd688ddced4f2396bd0f683538afd6d1411c55c218e009d1`.
Kernels SHA256: `e1ac9a250e44294d6d2a340282a1834664a6aa3e45bde8817e298386baadef97`.

```powershell
python tools/audit-concert-grand-observer-control.py --binary local/concert-grand-reconstruction/build/renderer.exe --capture-binary local/concert-grand-reconstruction/build/capture-renderer.exe --oracle local/concert-grand-reconstruction/oracle-masks --original local/concert-grand-identified-body --work local/concert-grand-reconstruction --output C:/SHARED/Control-reconstruccion-2026-10-02
python tools/fit-concert-grand-causal-observation.py --original local/concert-grand-identified-body --control local/concert-grand-reconstruction --oracle local/concert-grand-reconstruction/oracle-masks --oracle-audit C:/SHARED/Control-reconstruccion-2026-10-02/oracle-report.json --audition-reference C:/SHARED/Cuerpo-identificado-2026-10-02 --output C:/SHARED/Observacion-causal-2026-10-02
python -m unittest discover -s tools/tests -p test_concert_reconstruction.py
python -m unittest discover -s tools/tests -p test_concert_relative_observation.py
```
