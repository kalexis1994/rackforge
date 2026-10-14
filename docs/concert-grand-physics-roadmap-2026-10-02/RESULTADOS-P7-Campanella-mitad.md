# Primera mitad de La Campanella — escucha offline

Fecha: 2026-10-14.

El usuario escucha muy similares los fragmentos de ocho segundos con 8 y 12
modos no lineales, pero pide más música antes de evaluar el timbre general.
Se renderiza la primera mitad temporal del MIDI original con la variante de
8 modos. La selección por oído sigue pendiente; la similitud del fragmento
corto no certifica la calidad del instrumento en toda la obra.

## Música y modelo

El corte corresponde al 50 % de los 275,158854 segundos del archivo original.
Se retiran 1,026043 segundos de espera inicial: quedan **136,553384 segundos
de música**, **1.621 ataques** y **51 alturas**, entre MIDI 44 y 102.
Se conservan los tiempos, velocidades y controles originales, incluidos los
valores intermedios de CC64 y los cambios de CC67. Al corte se sueltan las
teclas y el pedal de sostén, y se calculan dos segundos de cola. Se anteponen
100 ms de silencio al WAV.

Cada golpe se calcula con fieltro nuevo sobre el estado vivo de las tres
cuerdas. Se mantienen los 96 modos transversales y 192 axiales por cuerda;
en vuelo sólo ocho modos transversales participan del potencial no lineal.
Los portadores lineales agudos siguen presentes. La fuerza de todas las
alturas excita una única tabla de 161 modos y una lectura estéreo continua.
No se utilizan notas muestreadas, ecualización ni compresión.

La tabla recibe las fuerzas en un solo sentido: su movimiento aún no vuelve
a las cuerdas. Las pérdidas naturales y el apagador usan las aproximaciones
diagonales ya declaradas en la [escucha corta](RESULTADOS-P7-escucha-MIDI-hibrida.md).
Esta escucha no equivale al piano recíproco completo ni a ejecución en tiempo
real; conserva las limitaciones de fidelidad previamente medidas en la tabla.

## Reducción del costo para esta escucha

Las entradas se construyen sin formar el banco denso de matrices Python.
Hasta ocho procesos calculan alturas independientes; las fuerzas se suman
en un orden fijo antes de procesar la tabla común. Las trazas densas de
vuelo se autentican y se eliminan después de obtener su fuerza a 48 kHz;
se conservan esas fuerzas compactas y los segmentos finos de contacto.

El reloj de vuelo se limita a 64 kHz. El contacto conserva el reloj fino
original de cada altura, incluso cuando se reduce su reloj de vuelo.
Cuando una tecla suelta y amortiguada baja de 1e−18 J de energía transversal,
se descarta únicamente ese residuo transversal y se salta hasta el siguiente
evento. Se conservan la fase axial, el vuelo del martillo y los controles.

Un control de ocho segundos con este ajuste tardó **33,071 s**, frente a
**86,998 s** del fragmento entregado anteriormente. La diferencia RMS de
las ondas fue **0,055435 %**, sin ajustar ganancia ni tiempos; esta cifra
compara los dos productores del fragmento, no su error frente al modelo
completo. El resultado y sus fuentes exactas están en **d736c9c9**.

- [Control de reloj y reposo](medidas-P7-cuerdas-nativas-2026-10-14/probe-pieza-reloj64.json).
- [Comparación con la escucha anterior](medidas-P7-cuerdas-nativas-2026-10-14/comparacion-reloj64.json).
- [Verificación del control](medidas-P7-cuerdas-nativas-2026-10-14/verificacion-probe-reloj64.json).

Los siete controles Python del productor MIDI y la preparación ligera pasan.
Incluyen identidad binaria frente a la entrada densa y reposo con controles
intermedios y reiteración, conservando las fases axiales y la cota de energía
descartada. El resto de las pruebas físicas previas permanece documentado
en la escucha corta.

## Resultado entregado

El render termina sin reintentos ni fallos del solver. Las fuentes de la
simulación permanecen idénticas a **d736c9c9** durante toda la ejecución.

| Medida | Resultado |
| --- | ---: |
| Audio estéreo FLOAT32, 48 kHz | 138,653375 s |
| Ataques / alturas | 1.621 / 51 |
| Render de cuerdas, suma y presión | 893,728 s (14 min 53,7 s) |
| Tiempo total, incluida preparación | 901,717 s (15 min 1,7 s) |
| Ganancia uniforme | 0,25 / Pa |
| Pico / RMS del WAV | 0,549960 / 0,030435 |
| Reposos / energía transversal descartada acumulada | 1.036 / 9,6603e−16 J |

No se activa la reducción de ganancia por picos. Los últimos 100 ms tienen
RMS 3,37e−17 y la última muestra tiene magnitud máxima 1,46e−17; no se agrega
un fade. El WAV ocupa 53.242.954 bytes y se guarda únicamente en
`C:/SHARED/Piano-Campanella-mitad-2026-10-14/Campanella-hibrido-NL8.wav`.

- [Informe del render](medidas-P7-cuerdas-nativas-2026-10-14/campanella-mitad.json).
- [Verificación del MIDI, artefactos conservados y WAV](medidas-P7-cuerdas-nativas-2026-10-14/verificacion-campanella-mitad.json).
- Productor: `tools/render-concert-grand-hybrid-piece.py`.
- Artefactos de trabajo: `local/concert-grand-physics/P7-Campanella-mitad-2026-10-14/`.

La verificación comprueba los SHA256 de todas las fuentes medidas, entradas,
segmentos de contacto, fuerzas compactas, presión y WAV. Coteja los eventos
por altura y los 1.621 golpes originales, verifica estados finales finitos y
pérdida no negativa en el fieltro, y reproduce exactamente las muestras del
WAV desde la presión y la ganancia declaradas. Los tiempos del archivo de
eventos se cotejan con la representación decimal `.16g` usada por el productor;
el antiguo margen absoluto de 1e−14 s era menor que el redondeo decimal en
tiempos superiores a 100 s. No se modificó la simulación para esta corrección
del verificador.

La duración final se redondea a muestras de 48 kHz. El tiempo de esta primera
mitad no se extrapola automáticamente a la segunda, que tiene otra densidad
de ataques y otro registro. La próxima decisión sobre timbre se toma después
de escuchar este archivo más largo.
