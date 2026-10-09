# Integración de cuerdas vivas y resonancia compartida — 2026-10-08

El usuario rechazó la maqueta MIDI por mal sonido y falta de resonancia.
La prioridad pasa a integrar los mecanismos existentes en una trayectoria
polifónica viva, con salida física, antes de ampliar la escucha musical.

## Construcción

`tools/concert_grand_shared_strings.py` reúne varios puertos de cuerda sobre
**una sola tabla**. Se suman las masas de levantamiento de sus extremos y
las inercias cruzadas cuerda/tabla; la masa y rigidez de la tabla, su
amortiguamiento material y el aire se incluyen una sola vez. Un cambio en
la tabla devuelve fuerza a todas las cuerdas conectadas, incluso sin golpe.
No se suman trayectorias independientes ni se reproduce audio muestreado.

Se conserva el potencial extensible de cada cuerda, sus dos direcciones
transversales y el movimiento axial. La normalización de masa es una
transformación invertible. Los bloques invariantes del Hamiltoniano
cuadrático conservan todos los modos. El gradiente discreto integra el
resto cúbico/cuártico sin agregar pérdidas para corregir el timbre.

Cada tecla tiene un martillo común y un contacto unilateral por cuerda.
La rigidez de fieltro supuesta se reparte entre sus tres contactos; esa
distribución no está identificada experimentalmente. Al reiterar se
relanza únicamente la cabeza separada, registrando su trabajo externo:
las cuerdas, la tabla y el aire conservan sus estados. No se modelan la
acción ni el backcheck. Los apagadores aplican fricción viscosa positiva
distribuida a las cuerdas de cada tecla, con levantamiento por tecla/pedal.
El descenso de 30 ms sigue siendo una condición supuesta.

El enfoque de acoplar martillo, coro de cuerdas, tabla y aire se describe en
[Chabassier, Chaigne y Joly, modelo temporal del piano](https://researchportal.ip-paris.fr/en/publications/time-domain-simulation-of-a-piano-part-1-model-description/).
La publicación no valida los parámetros ni las simplificaciones de este ensayo.

## Ensayo previsto y alcance

`tools/render-concert-grand-shared-phrase.py` integra una secuencia C4–G4–C5
con reiteración de C4. Hay nueve cuerdas, tres por tecla, cada una con
96 modos por dirección transversal y 192 axiales; una tabla de 606
coordenadas y un único estado acústico. Las pequeñas irregularidades de
tensión equivalentes a −0,4/0/+0,6 cents son explícitas y constantes,
no fluctuaciones de afinación añadidas al audio. Se comparan pedal y
ausencia de pedal con los mismos golpes y la misma ganancia de 0,25/Pa.

La primera formulación de contacto usó una matriz densa de 4065 coordenadas
y se interrumpió por coste, antes de producir audio. Su informe es
`medidas-P6-compartida-2026-10-08/frase-96.json`. Se sustituye la resolución
del precondicionador por eliminación exacta de sus bloques invariantes:
axiales y horizontales diagonales, verticales/tabla en un único factor,
cabezas libres fuera del potencial cuadrático. El residuo no lineal sigue
usando todos los campos. La referencia densa se conserva para verificar
equivalencia; no se declara una reducción modal.

Ocho pruebas pasan en `tools/tests/test_concert_shared_strings.py`:

- Recuperación del modelo previo de una cuerda, incluido el contacto.
- Energía cinética contra integrales espaciales independientes y una tabla.
- Derivadas del potencial y trabajo exacto del gradiente discreto.
- Transferencia a una cuerda sin golpear; al retirar su puerto desaparece.
- Relanzamiento que conserva la vibración y pérdidas positivas de apagadores.
- Balance de energía con golpes simultáneos y continuación libre.
- Tres contactos distintos que comparten una única cabeza de martillo.
- Trayectorias del solver por bloques contra la resolución densa completa.

## Límites pendientes

### Primer WAV físico terminado

La variante con pedal terminó su trayectoria en **1748,30 s (29,14 min)**.
`Frase-fisica-pedal.wav` dura 2,103479 s, es estéreo float32 a 48 kHz y
contiene cuatro golpes a velocidad prescrita 84: C4, G4, C5 y reiteración
de C4. La subida del pedal ocurre a 1,05 s de tiempo físico. Se aplica
únicamente la ganancia común 0,25/Pa; no hay bucle, EQ ni fade de salida.

Pico 0,331593, RMS durante la superposición 0,089194 y RMS de los últimos
100 ms 3,434e-8. La comprobación independiente del formato, finitud,
clipping y hash coincide con el informe. Las fuentes del productor coincidían
al verificar este WAV; la corrección posterior del controlador queda trazada
en `correccion-control.patch` y `control-procedencia.json`. Invertir el parche
reconstruye el productor original con su SHA256, sin duplicar la fuente. La norma de coeficientes transversales físicos de G4 antes de
su primer golpe alcanza 27,97 µm; es una medida de movimiento intrínseco
de esa cuerda, no un desplazamiento puntual del puente o de un micrófono.

El trabajo externo neto de las cuatro cabezas es 0,027409 J. Se registran
0,004734 J de fieltro, 0,001439 J de apagadores y 0,005839 J de material de
tabla. El ledger radiativo completo sigue pendiente. La energía mecánica
final incluye las cabezas separadas en vuelo, sin modelar el backcheck;
no equivale a la energía sonora de la cola. El usuario escuchó esta frase
y percibió **más cuerpo y resonancia**. Se conserva como referencia audible
y como dirección de integración; esto no acepta toda la regulación ni
certifica un instrumento completo. La variante sin pedal original se
interrumpió antes del evento redundante de pedal, sin escribir un WAV. Informe en `medidas-P6-compartida-2026-10-08/frase-bloques-96.json`.

### Alternativa de coste medida y no seleccionada

`tools/concert_grand_port_coast.py` propone portadoras separadas de cuerda
y tabla, con todo el acoplamiento cuadrático resuelto en el resto AVF
mediante un Schur del tamaño de la tabla. Tres pruebas pasan: reconstrucción
del Hamiltoniano/Schur frente a matrices densas, conservación de energía
no lineal y convergencia frente a una exponencial independiente en un sistema
pequeño. **Conservar energía no basta para seleccionarla por fidelidad.**

El ensayo mecánico de 606 coordenadas/nueve cuerdas, desde un desplazamiento
fundamental y durante 2 ms, da errores de estado final de 24,59/18,77/13,76 %
a 16/32/64 kHz frente a la portadora global a 128 kHz. El error energético
es menor que 4e-13; la dispersión sigue siendo excesiva. No se usa esta
alternativa en los WAV. No es un ensayo de presión con aire ni un estado
posterior al golpe. Datos en
`medidas-P6-compartida-2026-10-08/port-coast-mecanica.json`.

El problema de escalado sigue abierto: la portadora global necesita una
diagonalización conjunta que crece con las cuerdas. El contacto sí admite
eliminación por bloques, pero eso no resuelve por sí solo el coste del
sostenido ni permite prometer un teclado completo en tiempo real.

Esto integra la física disponible de un registro reducido; **no es toda
la física de un piano ni un nuevo plugin listo para tocar**. Siguen pendientes:

1. Medir la convergencia temporal del sistema de nueve cuerdas y su release.
   El control temporal anterior de una cuerda no se transfiere automáticamente.
2. Mejorar la reducción acústica: conserva pasividad y límites, pero hereda
   un error máximo de presión del 9,202 % frente al límite del 5 %. Esa medida
   es del ensayo anterior; no certifica los nuevos puertos mecánicos cargados.
3. Incorporar las otras teclas y su regulación física, cuerdas entorchadas,
   resonancia del conjunto sin golpear y respuesta del medio pedal.
4. Pérdidas internas de las cuerdas y su arrastre aéreo: hoy la cuerda
   aislada es conservativa; pierde energía por tabla/aire y por apagadores.
   El amortiguamiento material registrado en este render corresponde a la
   tabla, no al acero. Identificar también geometría, unísonos y fieltro
   contra grabaciones. Hay parámetros supuestos, no identificación Steinway.
5. Movilidad axial y horizontal del puente/tabla, mezcla de polarizaciones,
   rigidez Timoshenko y geometría real del instrumento; acústica de sala.
6. Coste de un teclado completo, MIDI continuo y traslado al plugin.

El certificado global continúa falso. Los informes se conservan sólo en
el repositorio; los WAV solicitados se destinan a
`C:/SHARED/Piano-fisica-compartida-2026-10-08`. No se duplica R ni se
modifica el paquete de fábrica.

### Corrección del control sin pedal

Los apagadores de teclas libres ahora parten cerrados en reposo con pedal
arriba. Una subida de pedal cuando éste ya está arriba no reinicia el
descenso ni libera artificialmente las cuerdas. La variante original tenía
ambos problemas; se interrumpió antes del segundo, sin publicar audio.

Cuatro pruebas de `tools/tests/test_concert_damper_control.py` cubren reposo,
release de tecla, subida redundante, subida real con tecla mantenida y
la igualdad del controlador anterior/nuevo en la secuencia con pedal.
La trayectoria del WAV con pedal conserva sus controles; no se rerenderiza
ni se sobrescribe. `--control-only` genera únicamente el nuevo control.
Su informe es `medidas-P6-compartida-2026-10-08/frase-control-corregido-96.json`;
la comparación corregida terminó en **1672,60 s (27,88 min)** de trayectoria.
`Frase-fisica-sin-pedal.wav` dura 2,103479 s, estéreo float32 a 48 kHz,
con pico 0,268821, RMS del tramo 0,5–1 s de archivo 0,068816 y RMS final
3,397e-8. La comprobación independiente verifica ambos archivos, sus hashes,
la finitud, el formato y la ausencia de clipping. Fuentes del control y
caché conservan sus hashes. Datos en `verificacion-wavs.json`.

A la misma ganancia, el RMS después del primer note-off (tiempo físico
0,18–0,24 s) es 0,083444 con pedal y 0,023536 sin pedal, razón 3,545.
En las otras ventanas tras note-off, las razones son 18,053 (G4), 41,321
(C5) y 4,645 (reiteración C4). Son ventanas de esta frase, que incluyen
superposición de notas y resonancia; no son constantes de decaimiento de
cuerdas aisladas ni certifican el timbre de un Steinway. El primer tramo
mantenido da RMS 0,115883/0,116007; no se normalizan las variantes.

La disipación por apagadores pasa de 0,001439 J con pedal a 0,004403 J
sin pedal. La norma intrínseca de G4 antes del golpe pasa de 27,97 a
4,18 µm al mantener su apagador cerrado. Estas diferencias son coherentes
con la resonancia simpática compartida, pero no sustituyen una medición
real ni la convergencia temporal del sistema completo.

El control emitió la advertencia ya existente de `log1p(-1)` en el secante
de fieltro cuando un cociente de compresiones redondea a uno: el límite
intermedio es infinito negativo y `expm1` devuelve -1, dando fuerza finita.
No se cambió esa primitiva congelada. La trayectoria terminó y el WAV
es finito; queda pendiente limpiar esa advertencia sin cambiar la ley.

## Continuación desde la referencia audible

1. Conservar la frase con pedal y sus parámetros como control de escucha.
2. Verificar refinamiento del transitorio compartido, especialmente ataque,
   reiteración y apagadores; el error acústico heredado sigue abierto.
3. Reducir coste contra este control antes de ampliar el banco de cuerdas.
   La alternativa mecánica ensayada arriba no pasa fidelidad y no se adopta.
4. Ampliar primero el registro medio con cuerdas vivas y tabla/aire únicos.
   Graves entorchados y agudos requieren su regulación, no transponer audio.
5. Integrar eventos MIDI continuos cuando el banco y su coste lo permitan.

Los dos WAV están sólo en `C:/SHARED/Piano-fisica-compartida-2026-10-08`.
Informes y fuentes permanecen en este repositorio. No se crea un paquete
nuevo ni se declara completada la física del piano.


## Aceleración algebraica y refinamiento local — 2026-10-08

`tools/concert_grand_shared_kernels.py` agrega un backend que conserva los
mismos modos y ecuaciones: transformadas coseno por lotes para las cuerdas,
puertos agrupados y permutaciones exactas en portadoras diagonales. Mantiene
las irregularidades, el fieltro, el aire y las pérdidas. El productor de los
WAV y sus fuentes congeladas permanecen intactos; no se genera otro audio
idéntico ni se modifica la referencia que gustó.

Tres pruebas en `tools/tests/test_concert_shared_kernels.py` pasan: mapas y
trabajo no lineal con cuerdas desiguales, trayectoria completa de contacto y
coast con memoria acústica/pérdidas, e inversa de las portadoras diagonales.
El primer intento del fixture usó factores acústicos aleatorios sin pareja
recíproca y fue rechazado por el constructor; el fixture se corrigió para
usar factores pareados. Esto no modificó el modelo acústico.

El ensayo grande de `tools/measure-concert-grand-shared-kernels.py` reproduce
la tabla de 606 coordenadas, nueve cuerdas 96/192 y el aire congelado de
rango 192. El contacto de 0,5 ms tarda 18,542/16,729 s, razón **1,108**;
la presión cambia 5,016e-10 y el mayor error relativo de un estado es
6,059e-10. El coast de 2 ms tarda 2,595/1,667 s, razón **1,557**; la presión
cambia 3,040e-14. Todos los estados mecánicos/acústicos están presentes.
Estos tiempos corresponden a una ejecución de prefijos, incluyendo la
preparación que cada solver necesita en ese tramo; no prometen ese factor
para una pieza completa ni para el plugin.

### Refinamiento medido

La presión es la contribución del **intervalo finito de fuente ensayado**,
incluida su cola retardada. En los casos que parten de un estado vivo no es
la historia completa del micrófono previa a ese intervalo. Se usa la misma
interpolación Hermite a 128 kHz y el mismo observador de celdas para todos
los pasos. El estado mecánico se compara en norma de desplazamiento
ponderado por frecuencia de las portadoras y velocidad, excluyendo el
vuelo libre de los martillos; los estados de onda y relajación se comparan
por separado en el JSON. No se suman normas de magnitudes con unidades
incompatibles ni se normaliza el audio por variante.

| Tramo | Pasos comparados | Cambio relativo de presión | Cambio de estado mecánico |
| --- | --- | ---: | ---: |
| Ataque desde reposo, 2 ms | 768 → 1536 kHz | 0,010868 % | 0,003340 % |
| Coast cargado, 10 ms | 16 → 32 kHz | 0,365842 % | 1,364642 % |
| Coast cargado, 10 ms | 32 → 64 kHz | 0,069720 % | 0,045074 % |
| Reiteración viva, 2 ms | 768 → 1536 kHz | 0,009486 % | 1,282604 % |
| Subida de pedal, 40 ms | 16 → 32 kHz | 0,657102 % | 5,521261 % |
| Subida de pedal, 40 ms | 32 → 64 kHz | 0,138100 % | 0,738819 % |

La cabeza se relanza tras 40 ms, preservando cuerdas, tabla y aire. Ese
estado común proviene de contacto a 1536 kHz y 38 ms de coast a 16 kHz,
con apagadores abiertos. No se afirma que esa historia previa esté
convergida. El release comienza desde la misma cápsula en memoria y
recorre los 30 ms completos de descenso del pedal. No se guardan cápsulas
ni otro R en disco. Los golpes corresponden sólo a velocidad prescrita 84.

Todas las comparaciones locales pasan el criterio de presión del 1 %;
**los estados internos aún difieren**, sobre todo en reiteración y release.
Una presión cercana durante un prefijo no garantiza fase correcta en
cuerdas/aire ni intercambios posteriores. Se conserva el certificado
global falso y no se adopta una tasa para todo el teclado con estos datos.
El ensayo terminó en 820,72 s (13,68 min), verificando hashes de fuentes y
tabla. Informe único: `medidas-P6-compartida-2026-10-08/kernels-convergencia.json`.

### Próxima decisión matemática

La aceleración por lotes puede usarse para ensayos nuevos con las mismas
ecuaciones. Antes de ampliar el banco conviene atacar la dispersión del
contacto: actualmente el bloque cuadrático se integra por midpoint durante
el golpe, aunque el coast utiliza portadoras exactas. Una hipótesis concreta
es extender esas portadoras al contacto y mantener el fieltro en un gradiente
discreto implícito, conservando el trabajo y las pérdidas pasivas. Se debe
probar primero contra contacto fino independiente: fuerza, separación,
presión, estado mecánico/acústico y balance de energía, incluidos unísonos
con compresiones distintas y reiteración. No está implementado ni validado
en esta etapa; no equivale a ablandar el martillo o cambiar la física.

También hay que refinar el release y el sostenido largo: la comparación
16/32 kHz local deja diferencias internas del 5,52 % al apagar. Después
se incorporará F4 al banco vivo, conservando esta referencia, y se ampliará
la escucha musical. El error acústico heredado del 9,202 % sigue pendiente.
