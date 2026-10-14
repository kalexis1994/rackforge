# P7 — Primera escucha MIDI del modelo híbrido

Fecha: 2026-10-14. El usuario prioriza un render offline escuchable y acepta
evaluar con el oído las simplificaciones físicas. El plazo de tiempo real
y los errores de estado/bandas siguen siendo diagnósticos; no son requisitos
para entregar esta escucha.

Base del experimento de reducción: **f42618a8**,
[modos lineales completos con no linealidad reducida](RESULTADOS-P7-hibrido-lineal-no-lineal.md).

## Qué se escucha

Primeros **ocho segundos desde el primer ataque de La Campanella** en el
MIDI original preservado, en dos versiones: **8 y 12 modos transversales
no lineales por dirección**, con 16/24 axiales no lineales. Cada altura
conserva tres cuerdas con 96 modos por dirección y 192 axiales durante
todo el fragmento. Se calculan ataques nuevos; no hay notas muestreadas,
transposición de WAVs ni reemplazo por otra biblioteca.

Se retira el silencio frío de 1,026043 s anterior al primer ataque y se
agregan 100 ms de silencio para la escucha. Los controladores de ese
prefijo se aplican como condiciones iniciales del piloto. Se mantienen las
notas, velocidades, orden y tiempos relativos de los eventos originales
anteriores al corte. Los eventos se redondean al paso de cada voz, con
error máximo de medio paso. El programa 0 usa los parámetros congelados
del prototipo; no selecciona otro instrumento.

Al corte se agregan explícitamente los note-off de las teclas aún sostenidas
y CC64=0, seguidos de un segundo de apagado/cola. Esos eventos de cierre
son sintéticos y están separados de los originales en el informe. Las
salidas duran **9,1 s**, estéreo FLOAT32 a 48 kHz.

## Ataque, repetición y apagado

Cada altura mantiene su propio estado vivo. Se conserva la ley anterior de
fieltro, velocidad de lanzamiento, huella y tres cuerdas levemente desafinadas.
El contacto se calcula con los 96/192 modos completos. Al separarse el
martillo, q/v pasan íntegros al solucionador híbrido. En una repetición se
reutilizan las cuerdas vibrantes y el martillo continúa su vuelo libre;
no se relanza una nota desde reposo ni se suma una grabación independiente.

Durante el vuelo, el potencial no lineal depende sólo del prefijo retenido.
Los modos restantes usan portadores lineales con pérdidas diagonales.
Los relojes del coast son 32/64/128/256 kHz para D#4/D#5/D#6/D#7, con
colocación de orden 16. El contacto usa 16 veces esos relojes. La captura
de fuerzas trabaja a 128 kHz o al reloj del coast si es mayor.

CC67 modifica áreas de contacto `[1-u,1,1]`, sin reemplazarlo por una
reducción de velocidad o volumen. El MIDI de este fragmento comienza con
CC67=127: la primera cuerda queda sin golpear, permanece en el estado y
se verifica que siga exactamente en reposo. Este piloto utiliza cambios
prescritos instantáneos del desplazamiento lateral; no integra el actuador.

CC64 y los contadores de teclas controlan un apagador diagonal aproximado:
se toma la diagonal del arrastre distribuido sobre una franja de 0,08L
centrada en 0,3L, con coeficiente 0,6 N s/m. El engagement tiene rampa de
30 ms. La aplicación del prefijo frío inicia esa rampa en t=0 del piloto;
no reconstruye la historia del actuador durante el silencio retirado.
No se aplica fade al audio. Esta diagonal omite las mezclas entre modos
del apagador distribuido completo.

## Tabla y presión: aproximación de esta escucha

Las fuerzas de las cuerdas excitan la tabla cargada de 161 modos y el
observador estéreo nativo existentes, autenticados contra el modelo
`residual-161.json` y los coeficientes `estados-DC.json` a 48 kHz.
La misma tabla y sus memorias de propagación procesan continuamente la
suma de todas las voces. No se reinicia la tabla entre notas o bloques.
No hay EQ ni compresor.

El acoplamiento es **en un solo sentido**. La tabla no devuelve movimiento
del puente a las cuerdas. Como aproximación del drenaje natural, se usa un
arrastre modal positivo derivado de la movilidad de entrada de esa misma
tabla: `sigma_n = cross_n² omega_n² Re(Y_tecla(omega_n))`. No es un cierre
recíproco ni una certificación de trabajo del instrumento completo. Las
otras alturas simpáticas del teclado tampoco están presentes.

La fuerza de puente combina la reacción inercial de las coordenadas y el
gradiente no lineal de borde. Durante el contacto se captura al reloj fino.
Durante el coast se reconstruye la aceleración baja con Hermite cúbico,
se interpola el gradiente de borde y se evalúan analíticamente los portadores
altos. La observación de los portadores altos se limita a 20 kHz; sus
coordenadas completas siguen vivas. Los segmentos de contacto se remuestrean
con antialias y colas FIR conservadas antes de sumarse al coast y efectuar
el remuestreo final. No se usan las nueve sondas mecánicas como audio.

Cambiar de contacto completo a coast reducido cambia el potencial elegido,
aunque se conserve q/v. Se mantienen también los fallos de fidelidad del
cuerpo reducido anterior y las hipótesis de escala de cuerdas/fieltro.
Esta escucha permite juzgar musicalmente el piloto; no aísla por sí sola
el error de reducción respecto de un piano completo ya integrado.

## Archivos y medición

Los dos WAVs se guardan únicamente en
`C:/SHARED/Piano-hibrido-MIDI-2026-10-14/`, con una ganancia común para ambos
para conservar las diferencias de nivel. Los estados finales, fuerzas,
segmentos de contacto y ejecutables quedan en
`local/concert-grand-physics/P7-audicion-midi-2026-10-14/`.

- [Informe de render](medidas-P7-cuerdas-nativas-2026-10-14/audicion-midi.json).
- [Verificación de procedencia y WAVs](medidas-P7-cuerdas-nativas-2026-10-14/verificacion-audicion-midi.json).
- Productor: `tools/render-concert-grand-hybrid-midi.py`.
- Verificador: `tools/verify-concert-grand-hybrid-midi.py`.

Las fuentes, coeficientes, MIDI, plan, entradas por altura y resultados se
autentican con SHA256. La verificación coteja cada evento entregado a las
voces, cuenta los golpes, comprueba finitud/calor de fieltro y reproduce
exactamente el WAV FLOAT32 desde la presión y la ganancia declaradas.
La decisión sobre calidad audible queda para el usuario.

Se aprobaron **66 pruebas Python y dos controles Rust** entre las suites
de fases, híbrido, cuerdas nativas, presión y MIDI. Los seis controles
nuevos cubren conservación de eventos y cierre sintético, rechazo de alias
ultrasónico con colas de filtro, repetición con cuerda viva y cuerda sin
golpear, presión continua frente al consumidor Python, portador amortiguado
y gradiente de borde frente a diferencias del potencial.
