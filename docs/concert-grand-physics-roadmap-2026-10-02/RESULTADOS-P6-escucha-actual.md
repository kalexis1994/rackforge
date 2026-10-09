# P6 — Escucha del prototipo actual, 2026-10-08

## Objetivo y alcance

Llegar a notas audibles del modelo actual, con ataque, sostenido y apagador.
No esperar a cerrar todos los certificados del operador de aire completo.
No sustituir el plugin instalado ni presentar el prototipo como un Steinway
identificado. El estado global de aprobación sigue siendo **falso**.

Se conservan las **606 coordenadas de tabla**, **96 modos transversales por
polarización** y **192 axiales**, incluidos el acoplamiento inercial y la
deformación geométrica no lineal. Hay **una cuerda por nota**, sin unísono.
Longitud de C4 de 0,62 m, densidad lineal de 0,0081 kg/m y tensión de 850 N;
la longitud cambia con la altura. Son condiciones asumidas del prototipo,
no una regulación identificada de todo el teclado. Fieltro: 2 g,
7,5e8, exponente 2,6, posición 0,125, ancho 0,02 y dependencia de tasa 0,1.

## Reducción acústica explícita

El aire completo tiene 786 588 estados de propagación y una correlación de
1,91 GB que se lee en cada paso. Para la escucha se reduce **sólo ese puerto**.
No se reutiliza la cápsula completa como si correspondiera al operador nuevo:
cada golpe empieza con tabla, cuerda y aire fríos.

La congruencia del factor positivo conserva reciprocidad y pasividad del
operador. La variante elegida proyecta en la métrica de la masa estática de la
onda y conserva el 99,9 % de la relajación original. Una segunda rama positiva
completa la masa omitida con el 0,1 % restante de resistencia. Masa estática
y límite resistivo se mantienen; **eso no demuestra equivalencia de timbre**.
Las dos ramas poseen almacenamiento positivo y disipación no negativa.

La base usa respuestas de movimiento y fuerza del operador completo en
48 frecuencias entre 70 y 12 000 Hz para C4, F4, G4 y C5. El control usa
nueve frecuencias diferentes entre 110 y 9 100 Hz y los dos micrófonos.
Con 192 coordenadas acústicas, el peor error de presión es **9,2021 %**.
**Falla el criterio declarado del 5 %**. Se permite la escucha mediante
`--diagnostic-rank 192`, registrando el fallo; no se cambia ese umbral ni se
otorga un certificado completo. Los primeros intentos y sus errores siguen
en `medidas-P6-escucha-2026-10-08/`.

## Duración, observación y salida

El contacto usa 768 kHz hasta la separación del martillo, sobre una grilla
común a las tasas posteriores. Todos los estados continúan vivos. Se comparan
coasts a 16/32/64/128 kHz en una trayectoria corta de C4 a 5 m/s, contra
128 kHz, con límite de presión espectral del 5 % entre 60 y 6 000 Hz.
El render usa la menor tasa que pasa este control local. **No es una prueba
de convergencia de un segundo, de todas las notas o de la banda superior**.

El apagador aplica tracción viscosa distribuida y positiva a ambas
polarizaciones, con descenso suave de 30 ms. No se ponen a cero las
vibraciones ni se aplica un fade al WAV. La presión se obtiene del observador
geométrico de celdas con retardo y aceleración de Hermite; se comprueba su
transferencia contra la integral continua. Resample a 48 kHz, estéreo,
float32. Una única ganancia para todas las notas, sin EQ ni normalización
individual. El informe registra picos, RMS final/sostenido y calor disipado.

Informes y fuentes en el repositorio; únicamente WAV en
`C:/SHARED/Piano-actual-2026-10-08`. Matrices e historias reducidas sólo en RAM.
Se conserva una única R completa y una única cápsula completa.

## Verificación y próximos pasos

### Resultado audible

La pasada final terminó en **2112,36 s (35,21 min)**, con pico de memoria
de **7,43 GiB**. Se produjeron los cuatro WAV individuales y
`Notas-modelo-actual.wav` (5,214 s), todos a velocidad prescrita 84.
La tecla se sostiene 350 ms y luego se simulan 650 ms de release.

| Nota | Pico digital | RMS final frente al sostenido | Ataque + coast, segundos de cálculo |
| --- | ---: | ---: | ---: |
| C4 | 0,177285 | −100,32 dB | 48,68 + 344,35 |
| F4 | 0,206838 | −124,39 dB | 45,80 + 348,35 |
| G4 | 0,114082 | −105,52 dB | 47,53 + 361,94 |
| C5 | 0,134784 | −106,42 dB | 48,31 + 362,49 |

Comprobación independiente de los cinco archivos: SHA-256 conforme al
informe, dos canales float32 a 48 kHz, datos finitos, sin clipping;
el WAV conjunto es exactamente la concatenación de los cuatro individuales.
La ganancia común es 0,25 unidades digitales/Pa. No se cambian ganancias
entre alturas. Tabla, R completa, manifiesto y fuentes autenticados sin cambios.
Datos y límites en `medidas-P6-escucha-2026-10-08/escucha-pasos-medidos.json`.
**El certificado global sigue siendo falso y el plugin no se ha sustituido.**

Tres pruebas verifican equivalencia al conservar todo el rango, los dos
límites, reciprocidad/pasividad en puntos de Laplace y balance de trabajo
de las ramas de relajación. Se autentican las entradas físicas y se comprueba
que R, su manifiesto y la tabla no cambiaron al terminar.

Pendiente: reducir el error acústico y verificar duración/release con tasa
fina; ledger angular completo; convergencia de carga/banda superior; unísono
físico, regulación del teclado e identificación contra las muestras. La
escucha permite decidir el próximo avance con evidencia sonora, sin borrar
los fallos numéricos anteriores.

Antes de una pieza MIDI del nuevo sistema habrá que incorporar varias
cuerdas y notas a **una tabla compartida**, con estados persistentes y
sustain de los apagadores. Sumar WAV de pianos independientes no verifica
esa física. La presente escucha son golpes aislados desde reposo.

## Maquetas MIDI solicitadas — 2026-10-08

Se renderizan completos los MIDI conservados de Nocturno op. 9 n.º 2 y
La Campanella mediante `tools/render-concert-grand-audition-midi.py`.
Las fuentes son exclusivamente los cuatro WAV físicos anteriores, a
velocidad 84; no se usan muestras del Steinway. **Son maquetas mediante
muestreo y transposición, no una simulación física polifónica del nuevo sistema.**

| Pieza | Ataques | Ataques en alturas físicas exactas | Duración del WAV, con cola |
| --- | ---: | ---: | ---: |
| Nocturno | 1498 | 185 | 234,564 s |
| La Campanella | 4249 | 136 | 285,159 s |

Se conservan los tiempos y velocidades MIDI. Las demás alturas se asignan
a la raíz más cercana, con extrapolaciones de hasta 33 semitonos. La
velocidad modifica sólo la ganancia, sin cambiar la física del fieltro.
El sostenido utiliza un bucle con cruce de fase y decaimiento escalar
medido; el release usa una envolvente escalar estimada del apagador.
CC64/66 usan umbral 64, sin medio pedal físico; CC67 reduce la ganancia
de nuevos ataques hasta 3 dB, sin simular una corda. Se contabilizan todos
los valores originales de los controladores. No hay tabla compartida,
resonancia simpática ni interacción física entre ataques. Esta escucha
no valida el timbre del teclado completo ni su respuesta dinámica.

Los dos archivos finales están en `C:/SHARED/Piano-actual-2026-10-08/`:
`Maqueta-Nocturno.wav` y `Maqueta-La-Campanella.wav`. Formato estéreo PCM24
a 48 kHz, sin EQ ni compresión, con una misma ganancia de mezcla 0,534354.
Picos verificados 0,352554 y 0,850000, respectivamente; datos finitos y
sin clipping. Los hashes de productor, fuentes, MIDI y WAV coinciden.

La primera pasada con interpolación lineal fue sustituida en los mismos
dos destinos por transposición con FIR polifásico Kaiser-9 y antialias.
El informe vigente es `medidas-P6-escucha-2026-10-08/maqueta-midi-antialias.json`;
`maqueta-midi.json` conserva el intento anterior, cuyos WAV fueron sustituidos.
La pasada final tardó 63,22 s. El máximo error de afinación de las razones
racionales es 0,01248 cents. Tres pruebas pasan: supresión de aliasing,
reiteraciones con sustain y captura selectiva del sostenuto.
La comprobación independiente queda en `verificacion-maqueta-midi.json`.
El certificado físico global sigue falso y el plugin permanece sin sustituir.
