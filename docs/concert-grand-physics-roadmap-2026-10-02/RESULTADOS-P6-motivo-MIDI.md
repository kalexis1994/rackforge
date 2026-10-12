# P6 — Primer motivo original con pedales y salida física continua

Fecha: 2026-10-11. Etapa de pedales guardada en `daba4ff5`.

## Interpretación

Primeros cuatro ataques de La Campanella, con tiempos, pitches, velocidades,
note-off y controladores originales. Banco compartido D#4/D#5/D#6/D#7,
doce cuerdas, cuatro martillos, 606 coordenadas de tabla y la misma carga
acústica reducida/racional existente. Contacto 256 kHz, coast/captura 128 kHz.
No se mezclan notas muestreadas o renderizadas separadamente.

La entrada fría original tiene programa 0, CC64=127 y CC67=127. Se conserva
la elección explícita del único programa del piloto. Se reduce analíticamente
el silencio de 1,026043 s anterior al ataque; las acciones de pedal avanzan
por esos eventos, con todos los estados mecánicos y acústicos en reposo.
El primer ataque se ubica tras un silencio de escucha único de 100 ms.

| Tiempo desde primer ataque | Nota | Velocidad |
| --- | --- | ---: |
| 0 s | D#4 (63) | 28 |
| 0,020832031 s | D#5 (75) | 14 |
| 0,519531250 s | D#5 (75) | 33 |
| 0,555988281 s | D#4 (63) | 21 |

Los valores finales se cotejan con el plan conservado; esta tabla no reemplaza
los ticks enteros del informe. El corte se elige a **0,72 s**, después de
todos los note-off originales del fragmento. No se fuerzan note-off nuevos.
El resto de La Campanella no se interpreta en esta etapa.

## Cola declarada

Se agrega **un evento sintético CC64=0 a 0,72 s**, exclusivamente para cerrar
el fragmento y escuchar el apagado. No se presenta como un evento del MIDI
original. CC67 permanece donde lo deja el MIDI. Se integran otros **0,24 s**
de la misma trayectoria, con la bajada de soporte y pérdidas distribuidas
del apagador; no se aplica fade de audio ni normalización.

`PhysicalWavStream` mantiene una única convolución/resampleo por todo el
motivo. Bloques físicos de 8 ms y sub-bloques de fuente de 1024 intervalos;
se retienen sólo fronteras B/V, transferencia fija, cola de presión y fase
FIR, sin otra historia modal o R en disco. La ganancia continúa en **0,25/Pa**.
Salida FLOAT32 estéreo a 48 kHz, sólo en:

`C:\SHARED\Piano-motivo-MIDI-2026-10-11\Campanella-inicio-fisico.wav`

## Controles y resultados

Dos pruebas de planificación pasan: eventos/pedales/pre-roll/tail explícitos,
sin cambio de pitches, velocidades o timestamps relativos; cortes con teclas
sostenidas, notas fuera del banco y prefijos no fríos se rechazan.
Los componentes de dinámica/pedales/WAV conservan sus pruebas anteriores.

El primer intento se detuvo a la frontera de 80 ms: el reproductor volvía
a evaluar una spline sobre muestras que ya estaban en la grilla uniforme.
La resta de tiempos absolutos perturbaba por redondeo la frontera B/V, y el
observador correctamente la rechazó. Se elimina esa re-evaluación redundante
y se entregan los nudos nativos. No se relaja la tolerancia de continuidad.
Una regresión de **120 bloques** exige fronteras idénticas bit a bit; junto
a los dos controles de planificación y las regresiones físicas/de WAV,
**cinco pruebas pasan en 20,422 s**. Los estados mecánicos no cambian.
El informe conserva el hash/fallo/progreso del intento interrumpido; sólo
ese WAV parcial se reemplaza en su misma ruta. No hay audios duplicados.

Productor: `tools/render-concert-grand-physical-midi.py`.
Informe único: `medidas-P6-motivo-MIDI-2026-10-11/render.json`.
Verificador: `tools/verify-concert-grand-physical-midi.py`; autentica fuentes,
plan/MIDI/WAV, coteja cada evento y lanzamiento, reloj y frames, formato,
finitud, margen de pico, calor no negativo, aritmética de trabajo y cola.
No vuelve a integrar los estados modales. **Render terminado y verificado.**

| Medida | Resultado |
| --- | ---: |
| Integración física | 0,96 s; 3604 pasos de contacto y 121078 de coast |
| Tiempo de dinámica | 5268,552 s (87,809 min) |
| Tiempo del productor, preparación y cierre incluidos | 5355,479 s (89,258 min) |
| Bloques físicos / máximo de nudos retenidos | 120 / 1025 |
| WAV | 51049 frames; 1,063520833 s; FLOAT32 estéreo 48 kHz |
| Pico absoluto | 0,0395429134 |
| RMS de los últimos 20 ms | 2,010187789e−6; −85,87664 dB respecto al pico |
| Última muestra / pico | 4,359696070e−12 |
| Residuo mecánico / trabajo aplicado absoluto | 1,699678042e−13 (límite 1e−8) |
| Residuo mecánico | 1,082821509e−16 J |
| Tamaño del único WAV | 408480 bytes |

La verificación independiente en `medidas-P6-motivo-MIDI-2026-10-11/verificacion.json`
autentica las fuentes medidas, plan/MIDI y audio, coteja los cuatro lanzamientos
y eventos originales, reloj, frames, formato, finitud y aritmética del trabajo.
Pasan el margen de pico, el control de cola −40 dB y el balance escalar.
Las cuatro teclas terminan liberadas y los cuatro apagadores modelados llegan
a precarga completa. Fuente, tabla y R conservan sus hashes; estados finitos.
No se modifica el plugin instalado ni se aprueba el modelo global.

SHA-256 del audio:
`73c808a08b6220ac2f15e595451061fcdf6bc3e4768805c13f50a2163a103d9e`.

El cuarto lanzamiento registra **−1,227777406e−4 J** de trabajo externo al
prescribir otra velocidad al martillo que ya tiene estado vivo; se conserva
su signo en el ledger. No se interpreta como calor negativo. El piloto sigue
sin resolver una acción completa de escape/rearme del martillo.

## Alcance

Escucha diagnóstica de cuatro ataques suaves, no una pieza completa, una
identificación del Steinway ni aprobación por oído del nuevo mecanismo.
Los 30 ms de acciones/retorno y la geometría del fieltro/apagador son supuestos.
Siguen los pendientes de registro real sin apagadores agudos, coros de graves,
inercia/variación lateral del fieltro y selección de programas físicos.
Se mantienen el gate espacial fallido de 9,2021 %, los fallos temporales/Hann
de cola y la malla espectral anterior. No se declaran convergencia temporal
o un certificado global por cerrar el trabajo mecánico escalar.

Después de escuchar: evaluar definición, cuerpo y cola del motivo antes de
cambiar otra física. El tiempo total equivale a unas **5579 veces la duración
simulada** en este banco. Antes de extender duración/teclado: perfilar un tramo
representativo, localizar el costo de integración/matrices globales densas y
verificar cualquier aceleración contra la misma entrada y los resultados medidos. No
extrapolar una aprobación de piezas completas a partir de estos cuatro ataques.
