# Seguimiento de física pendiente — Concert Grand

Actualizado: 2026-10-13. El usuario autoriza investigar y probar los mecanismos
pendientes en orden, documentando resultados. Las muestras disponibles permiten
comparar el resultado audible; no identifican de forma única cada mecanismo real.

## Ubicación de los archivos

Por pedido del usuario del 2026-10-04, los informes, planes, resúmenes y fuentes
se mantienen en el repositorio como única copia. No duplicarlos en `C:\SHARED`
ni crear allí ZIP de fuentes que ya conserva Git. Los cálculos nuevos se
guardan en `local/concert-grand-physics/`, ignorado por Git. `SHARED` conserva
los datos anteriores necesarios para reproducir controles, el paquete actual
y, cuando se soliciten, los audios de escucha. No duplicar factores acústicos.

El usuario pidió eliminar los audios existentes de `SHARED` y las copias
idénticas de informes ya versionados: se eliminaron 2361 archivos de audio y
134 informes/resúmenes duplicados. Las rutas históricas de escucha ya no
contienen esos audios; se conservan en el repo sus parámetros y resultados.
Los datos del ensayo de contacto compacto en curso permanecen en `SHARED`.

El 2026-10-05 se eliminan además los siete WAVs recientes del piloto, seis
paquetes obsoletos y un informe idéntico al del repo: 14 archivos, 28.432.210
bytes. [Manifiesto de limpieza](limpieza-2026-10-05/shared-manifest.json).
Las rutas de escucha registradas más abajo también son históricas; sus datos
numéricos y resultados siguen disponibles. No se generan nuevos WAVs en esta
etapa de cobertura estructural.

## Base y reglas de comparación

- Fábrica y paquete actuales: 0.172.0. El paquete en `C:\SHARED` conserva
  SHA256 `ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
- El control A sin drenaje conserva un sostenido que gustó al usuario. Es una
  ablación resistiva, **no una nueva calibración física aceptada o instalada**.
- B/piloto I fue rechazada por efecto parecido a flanger. No usarla como base.
- Una hipótesis por etapa, con control congelado, mismos eventos y ganancia.
  No aplicar otra vez los 25,120884 dB ya incorporados en el renderer actual.
- Mantener un control con carga física. Los controles sin drenaje sólo sirven
  para atribuir efectos. No normalizar individualmente las notas para ocultar
  regresiones ni aceptar automáticamente una mejora del score.
- Prototipos nativos fuera de la calibración de fábrica. Antes de trasladar
  algo al plugin: estabilidad, uso de pedales/reiteraciones, coste WASM y escucha.
- Conservar resultados rechazados y sus causas. No repetir búsquedas sin una
  hipótesis distinta. El avance acumulado quedó en el commit 346b6c8 autorizado
  el 2026-10-03; no se selecciona ni empaqueta automáticamente un piloto.

## Orden y estado

### Prioridad actual: destilar el maestro a un instrumento realtime

El usuario pide avanzar hacia una prueba tocable. El
[cuerpo con presión estéreo nativa](RESULTADOS-P7-presion-realtime.md)
ya conserva los 161 modos, las pérdidas y los puertos recíprocos; conversión
digital máxima **0,2988/0,2947 %** a 44,1/48 kHz. Once pruebas unitarias y
replay independiente mediante exponencial real pasan. El consumidor final,
con presión incluida y cero asignaciones, mide P99 **0,9935/1,0198 ms**;
la repetición conserva **0,9811/1,1387 ms**, fallando reserva a 48 kHz.
La ruta portable también falla reserva. Fidelidad al maestro sigue fallando:
presión ~5,780 % RMS y movilidad máxima ~13,45/13,23 %. Se impone presión
cero ante carga estática y primer momento nulo. La interfaz de movilidad
media ya permite preparar el cierre de puertos, pero **no hay aún cuerdas,
contacto o pedales nativos integrados ni instrumento tocable**. Sigue esa
integración conservativa y su costo completo antes del ejecutable MIDI;
no se instala una variante ni se modifica el paquete de fábrica.

El [coro no lineal nativo a puente fijo](RESULTADOS-P7-cuerdas-nativas.md)
ya conserva tres cuerdas 96/192, contacto compartido y reiteración viva.
Composición exacta de portadores, Newton reducido y bucles contiguos bajan
el máximo de 148,95 ms a 5,96 ms por 256 frames a 44,1 kHz; aún falla plazo,
sin cuerpo/presión. El bloque rápido pasa estado a 6 ms pero **falla a 250 ms**:
12,01% a 44,1 kHz y 5,86% a 128 kHz. Con 16 pasos a 128 kHz el estado largo
pasa 0,324%, pero el máximo cuesta 14,70 ms frente a 2 ms. Diez pruebas pasan,
ledger largo <8e−13; eso no aprueba la trayectoria rechazada. Sigue resolver
temporal/costo antes del cierre con inercia compartida y del host tocable.

La [continuación de vuelo/kernels](RESULTADOS-P7-vuelo-kernels.md) compone
también el movimiento vertical sólo con ausencia de contacto certificada,
usa FFT real N/2, kernels SIMD/portable y reutiliza gradientes de extremos
aceptados. A 128 kHz/16 conserva estado largo **0,324171%**, ledger <7e−13;
14 pruebas pasan. SIMD: media **4,7170 ms**, P99 **5,7415 ms**, máximo/primer
ataque **10,3108 ms** frente a 2 ms; portable máximo **15,1163 ms**. Ambos
siguen fallando CPU sin cuerpo/presión. Se registra el costo del ataque
sin excluirlo y se reutiliza la referencia larga autenticada del mismo
fixture. No hay todavía motor integrado tocable.

La [eliminación dinámica de incógnitas axiales](RESULTADOS-P7-axial-endpoint.md)
conserva las 1153 coordenadas del coro y determina la dirección de la
corrección de trabajo; el candidato ambiguo rechazado queda registrado.
Contacto reutilizado y vuelo escalar: media **2,2314 ms**, P99 **3,7293 ms**,
máximo **3,9336 ms**, primer ataque **3,7293 ms**; cero asignaciones.
El control vectorial mide media 2,1953 ms: no se demuestra ventaja del vuelo
escalar. **18 pruebas pasan**; estado a 250 ms con reiteración **0,3241655%**,
ledger <3e−12. Todas las rutas siguen fallando los **2 ms** a 128 kHz/16,
sin cuerpo/presión/pedales ni polifonía. Sigue reducir costo de campos y
validar el bloque temporal antes de integrar el instrumento completo.

La [continuación de invariantes exactos y FFT](RESULTADOS-P7-invariantes-fft.md)
omite sólo incógnitas horizontales exactamente nulas del vuelo, sin retirar
modos, y fusiona pasadas de la FFT. C4 limpia: 577→289 incógnitas, media
**1,9194 ms**, P99 **2,3351 ms**, máximo/ataque **3,7864 ms**. Repeticiones
contra el ejecutable congelado: mediana de medias **2,1308→1,9725 ms** (7,4%).
**21 pruebas pasan**; mismo estado largo **0,3241655%**, cero asignaciones.
P99/ataque aún fallan 2 ms. Con ambos ejes excitados se mantienen 577
incógnitas: 250 ms con reiteración reproducen el control exactamente, pero
el benchmark sostenido sin reiteración falla en ambas versiones a 254,875 ms.
Se conservan ese fallo y los rechazos de cache/disposición de coeficientes
y bloques temporales mayores. Sigue refinar el bloque al fallar el solver,
antes de aprobar duración y cerrar el instrumento completo.

El [refinamiento transaccional del bloque](RESULTADOS-P7-refinamiento-transaccional.md)
recupera el rechazo a 254,875 ms con dos hijos de 8, sin relajar tolerancias,
y restaura todo el estado/cache si falla incluso un paso. **24 pruebas pasan**.
C4 limpia mantiene estado largo **0,3241655%**, media **1,9533 ms**, P99
**2,8390 ms** y máximo/ataque **3,7407 ms**; cero asignaciones, sin refinamientos.
Con ambos ejes perturbados termina 1,024 s, pero cuesta 11,0319 ms medios y
20,4251 ms máximos. La precisión de ese caso **no pasa**: a 256 ms, N16/N8/N4/N1
difieren 136,99–142,23% del monolítico a 128 kHz, que también falla ledger
(8,50e−8 frente a 1e−9). Sigue refinar/autenticar la referencia temporal y
estudiar fuerzas/corrección de trabajo antes de ampliar aprobación o integrar
el instrumento. Se conservan los fallos y el control sin recuperación.

El ajuste conjunto anterior está en **747f3917**. La
[proyección variable de dinámica/observador](RESULTADOS-P7-proyeccion-variable.md)
mantiene 161 modos y regularización fija. Movilidad 4,545 % RMS/14,533 % máximo;
presión 6,730 % RMS, o **5,937 % RMS/12,003 % máximo** con observador sobre las
1189 frecuencias. Dos pruebas nuevas y replay pasan. Siguen fallando fidelidad
y reserva de cuerpo (P99 1,2318/1,1923 ms). Sigue enriquecer/pesar las peores
sondas del entrenamiento, sin seleccionar sobre la validación.

La primera etapa P7 queda guardada en **6b5cb0a8**. El
[ajuste conjunto de movilidad y presión](RESULTADOS-P7-ajuste-conjunto.md)
mantiene 161 modos, matrices positivas y momentos, e incluye presión en la
dinámica. Movilidad 4,702 % RMS/14,896 % máximo; presión 11,198 % RMS, o
9,988 % RMS/24,036 % máximo al recalcular el observador. Siguen fallando gates.
Tres pruebas nuevas y replay independiente pasan; transformación por bloques
reduce el defecto a ~1e−12. Costo nativo a 44,1 kHz varía y falla reserva de
cuerpo, a 48 kHz pasa; se conservan ambas corridas. Sigue ajuste de dinámica
y observador con proyección variable, después transitorios y cierre instrumental.

El usuario solicita conservar la física en un modelo ejecutable en tiempo real.
El motivo anterior queda guardado en **f0190e32**. La [primera etapa de
destilación y sus pendientes](RESULTADOS-P7-destilacion-realtime.md) implementa
un núcleo Rust de tabla seca, con preparación fuera del callback y puertos
recíprocos. Compresión espacial 88→20 conserva los 606 modos y las pérdidas
cruzadas; error complejo de movilidad 1,092 % RMS/8,527 % máximo.
44,1 kHz/256: P99 **2,5914 ms**, plazo 5,805 ms; sin asignaciones durante
el callback medido. **Falla la reserva de 20 %** para el cuerpo: consume
44,64 % del plazo. Se conserva sólo como diagnóstico, fuera del plugin.
Ocho pruebas y 72 controles nativos/Python; hashes/gates autenticados.
No hay WAV nuevo ni aprobación global. Las reducciones de estados y pruebas
de SIMD que no cumplieron se registran, sin ocultar sus fallos.

Ya está en curso la reducción de **tabla + carga acústica causal + presión**:
[maestro cargado, ajustes positivos y límites](RESULTADOS-P7-cuerpo-cargado.md).
El diagnóstico de 114 modos pasa la reserva de cuerpo (0,7736 ms P99 a
44,1 kHz/256), pero falla fidelidad de movilidad y presión. El banco de
161 modos pasa reserva de cuerpo (0,9581 ms P99), pero falla fidelidad:
movilidad 4,074 % RMS/13,056 % máximo, presión 13,763 % RMS/28,611 % máximo.
La identificación de pérdidas se acelera 5,023×
con Woodbury/adjuntos directos, conservando su objetivo y gradiente.
21 pruebas pasan; replay de seis movilidades/cinco observadores y cuatro
controles nativos registrados. Sigue seleccionar estados considerando
conjuntamente movilidad y presión, cerrar su observación temporal/digital, después
cuerdas/contacto/pedales vivos, cierre de puertos y presupuesto total/WASM.
La reducción seca aislada no sustituye el modelo del piano. Ver el orden,
criterios y alcance en el informe P7 antes de iniciar otra búsqueda.

El usuario rechazó la maqueta MIDI de cuatro muestras transpuestas por su
sonido y falta de resonancia. Se prioriza una trayectoria de cuerdas vivas
sobre una sola tabla y un solo aire, con unísonos, reiteración y apagadores.
Ver [integración y límites](RESULTADOS-P6-tabla-compartida.md). Las escuchas
de golpes aislados y esa maqueta no validan un instrumento polifónico completo.
La frase compartida C4–G4–C5 ya produjo un WAV físico: el usuario percibió
más cuerpo y resonancia. Se conserva como referencia para ampliar el banco
de cuerdas. El control sin pedal se corrige para iniciar con apagadores
cerrados y evitar que una subida redundante los abra; la comparación ya
terminó y ambos WAV pasan formato/finitud/clipping y hashes. Se conservan
en `C:\SHARED\Piano-fisica-compartida-2026-10-08` con la misma ganancia.
El backend por lotes conserva las ecuaciones y pasa equivalencia contra el
solver original: el prefijo de coast medido corre 1,557 veces más rápido.
El refinamiento local de presión pasa el 1 %, pero los estados internos
cambian hasta 5,52 % en release a 16/32 kHz. Sigue resolver dispersión del
contacto y refinar el sostenido/release antes de ampliar el banco con F4.
Ver medidas, alcance y próxima hipótesis en el mismo informe de integración.
La [extensión de portadoras al contacto](RESULTADOS-P6-contacto-portadoras.md)
pasa pruebas de trabajo/pérdidas y contraste continuo independiente. En
prefijos C4/84, el contacto a 128 kHz corre 12,56/18,87 veces más rápido
que las referencias finas de ataque/reiteración, con errores pequeños de
presión y estados. El release 32/64 kHz aún cambia 2,105 % en la onda;
64/128 kHz baja a 0,527 %. Son ensayos locales, no certificado global ni
aceleración del coste completo. Siguen C5/velocidades altas, fuerza y primera
separación, transición entre solvers y frase completa antes de ampliar con F4.
El [control de fuerza y registros](RESULTADOS-P6-contacto-registros.md) termina
en 17,91 min: C5/124, C4/124 y C5/84 pasan presión/estados/curva/impulso al
1 % local con contacto a 128 kHz; presión máxima 0,007094 %, onda máxima
0,105577 %. Los intervalos nativos de separación solapan sus referencias,
sin recontactos observados. La transferencia de 2 ms a coast conserva los
arrays bajo 1e-8. El descenso inicial de apagador 64/128 kHz pasa, pero la
onda cambia 0,917074 % en sólo 8 ms: poco margen, no cola completa aprobada.
Sigue integración y control de la frase completa, presión por bandas/coste;
después F4. La selección audible y el gate acústico global siguen pendientes.
La [integración completa de la frase](RESULTADOS-P6-frase-integrada.md) termina
en 33,24 min, con contacto 128 kHz, coast 16 kHz y estados vivos conservados.
Cuatro pruebas nuevas pasan. La contracción exacta del aire con un layout
contiguo compartido en RAM mejora 9 % los prefijos de coast, sin otra R al
disco. Las frases con/sin pedal tardan 15,79/15,22 min frente a 29,15/27,88:
**1,845/1,831× en el recorrido completo**. WAVs verificados en
`C:\SHARED\Piano-contacto-integrado-2026-10-09`.
El error global 0,036646/0,026404 % pasa el 1 %, pero las bandas altas fallan:
2,047/2,623 % en 2,5–6 kHz y 10,108/8,463 % en 6–12 kHz, de energía pequeña.
Se conserva el fallo sin cambiar el umbral. Sigue atribución contra referencias
temporales/capturas más finas, luego banco con F4; no retocar timbre a partir
del error contra un control grueso. Gate acústico y temporal global pendientes.
La [atribución por bandas nativas](RESULTADOS-P6-bandas-nativas.md) completa
contactos C4/C5/84 y un coast común de 8 ms. La referencia de contacto
768 kHz cambia 2,004/1,170 % en 6–12 kHz Hann frente a 3072 kHz;
portadoras 128 kHz bajan a 0,296/0,252 %. La captura 128 kHz del contacto
fino aporta sólo 0,00344/0,00196 %. Los controles de contacto pasan.
La primera grilla del coast falla y se preserva. El refinamiento a
6,25 Hz y referencia 512 kHz pasa todos los controles de medición:
coast 16/32/64 kHz cambia 63,937/4,852/2,001 % en la banda superior Hann;
128 kHz baja a 0,453 %. Esa banda tiene poca energía. A 16 kHz hay tanto
error dinámico (onda 7,054 %) como de reconstrucción Hermite; interpolar
a 128 no recupera lo perdido. Corregir sólo la captura no basta.
Cuatro pruebas y verificación independiente de datos/hashes pasan.
Los dos ensayos completados suman 31,04 min; no se generan WAVs nuevos.
Un único estado C4/84 de 2 ms, autenticado y compacto, queda en `local/`.
Sigue aislar el error dinámico del coast con un control cuadrático que
separe residuo no lineal y composición mecánica–aire; luego ensayar el
operador responsable y fuente continua contra las referencias, con coste
y balance de trabajo. Coast 128 pasa sólo 8 ms y cuesta 6,42× frente a 16
en ese prefijo. No hay aprobación global ni cambio de timbre/fábrica.
Después duración/release y banco con F4 para escucha.
El [control cuadrático del coast](RESULTADOS-P6-coast-cuadratico.md) termina
en 4,98 min: el error de onda 16 kHz persiste en 7,056 % sin residuo
no lineal. Refinar sólo el aire lo empeora a 10,303 % (macro 16) y 6,751 %
(macro 32). El flujo mecánico libre coincide en extremos, pero su fuente
Hermite cambia 18,155 % en la banda superior Hann. Los controles de
medición y trabajo pasan; las ablaciones no se seleccionan como piano.
Se conserva un primer fallo del script por nombre de referencia, corregido
y repetido con el mismo estado, fuentes reconstruibles y umbrales intactos.
El [paso de puerto conjunto](RESULTADOS-P6-puerto-acoplado.md) añade trabajo
acoplado y mantiene toda la física. Cuatro pruebas nuevas pasan entre
ambas etapas, incluida energía acústica y ODE independiente del método.
Su ensayo físico termina en 3,53 min: 128 kHz falla 6–12 kHz Hann con
1,825 %; 256 kHz pasa localmente con 0,378 %, pero cuesta 1,697× frente
al backend anterior de 128 kHz. Se rechaza como aceleración del renderer.
Todos los controles y hashes se verifican independientemente; no hay
WAVs nuevos. Sigue contrastar el puerto conjunto cuadrático para separar
la precisión de su residuo AVF del puerto; comparar el AVF de tramo
completo con las dos mitades anteriores es una hipótesis, no una causa
probada. Después mejorar la evaluación dentro del paso con controles de
trabajo/campo/coste, y volver a duración/release y F4. Gates globales pendientes.

El [control cuadrático del puerto conjunto](RESULTADOS-P6-puerto-cuadratico.md)
termina en 2,87 min: sin residuo no lineal, 128 kHz todavía falla la banda
superior Hann con 1,481 %. La explicación exclusiva por la no linealidad
queda descartada. Replay exacto, medición y trabajo pasan; ablación sólo
diagnóstica, con el cambio de potencial registrado aparte de las pérdidas.
La [composición de dos intervalos no lineales](RESULTADOS-P6-puerto-dos-intervalos.md)
añade tres pruebas que pasan (energía, ODE independiente y equivalencia
cuadrática). Ensayo físico de 3,92 min: mejora el error mecánico acotado
32 kHz de 1,266 a 0,053 %, pero 128 kHz todavía falla Hann con 1,500 %.
A 256 pasa localmente con 0,365 %, tardando 2,337× el splitting previo
128 de la misma ejecución; se rechaza como aceleración del renderer.
Fuentes/datos/grillas/ledger/cotas verificados independientemente, sin
WAV ni duplicados de R/estado. Sigue respuesta de fase del sistema lineal
acoplado frente a matriz/ODE exacta y puerto de mayor orden en banco pequeño;
después trabajo/coste en banco grande, duración/release y F4 para escucha.
No retocar física para compensar error numérico. Gates globales pendientes.

El [análisis de fase lineal y dispersión del aire](RESULTADOS-P6-fase-lineal.md)
añade tres pruebas que pasan y una referencia exponencial contrastada con ODE.
El banco pequeño no reproduce el fallo grande; Gauss en el bloque acoplado
empeora la fase de las portadoras y se rechaza. Se mide entonces el mismo
operador acústico real de rango 192/606 coordenadas, sin ataque nuevo:
a 128 kHz la matriz de carga armónica cambia 7,909 % a 6 kHz y 5,084 %
a 10 kHz por la discretización midpoint. No son errores de micrófono.
Ocho frecuencias/cuatro tasas y hashes/normas verificados; informes sólo
repo, matrices reutilizables sólo caché local, ninguna otra R ni audio.
Sigue implementar dos etapas Gauss sólo en el aire con una rama compleja
y su conjugada, fuerza media y radiación de ambas etapas; conservar las
portadoras mecánicas y trabajo de puerto. Derivación documentada, todavía
sin selección: equivalencia/pasividad/coste primero, luego coast auténtico,
duración/release y F4. Gates globales y precisión acústica siguen pendientes.

El [paso Gauss sólo del aire](RESULTADOS-P6-aire-Gauss.md) ya se implementa
con un único GEMV real de residuos y dos paquetes independientes de radiación.
Cuatro pruebas pasan: etapas reales/complejas con memoria, respuesta armónica,
trabajo completo y ODE no lineal. Ensayo grande 4,59 min: 128 kHz mejora Hann
alto a 1,268 %, pero sigue fallando y empeora onda a una cota 2,200 %
(inferior 1,985 %). A 256 pasa sólo el tramo con 0,327 % y cuesta 3,076×
el splitting 128 de la misma ejecución; rechazar como aceleración.
Respuesta real armónica en 1,05 min: a 128 la carga 6/10 kHz mejora a
0,693/2,185 % frente a 7,909/5,084 %, pero 12 kHz empeora a 3,399 %.
Corregir el operador aislado no basta para el estado acoplado completo.
Fuentes/datos/ledger/cotas verificadas, sin R/estado/audio duplicado. Sigue
dos velocidades y fuerzas de puerto emparejadas con las mitades mecánicas,
equivalencia/work/coste primero; luego mismo coast, duración/release y F4.
Hipótesis registrada, no causa exclusiva demostrada. Gates globales falsos.

El [puerto con dos fuerzas y velocidades distintas](RESULTADOS-P6-puerto-dos-fuerzas.md)
ya se implementa conservando todo el potencial y la memoria del aire.
Cuatro pruebas independientes pasan. Ensayo grande 7,44 min: a 128 kHz
Hann alto mejora **1,268 → 0,335 %** y cota de onda **2,200 → 0,407 %**
frente al puerto constante anterior. A 32/64 falla presión alta;
a 128/256 pasan los controles locales, pero cuestan **3,595/6,628×**
el splitting actual 128. Se rechaza como aceleración y se conserva como
control. Fuentes, datos, ledger, cuadraturas y cotas verificados; misma
R/estado y ningún audio duplicado. Se cierra esta hipótesis de integración.
Sigue **nota completa C4/84 con apagador y escucha**, usando el método
actual, antes de otra cadena de diagnósticos pequeños. Revisar la ruta
contacto/coast para que igualar tasas no mande todo al solver de contacto;
protocolo concreto en el informe. Después duración/release, frase y F4.
Sin selección de timbre ni cambios al plugin instalado; gates globales falsos.

La [C4/84 completa con release físico](RESULTADOS-P6-nota-release.md) ya está
renderizada a contacto 256/coast 128 kHz: 400 ms físicos, 0,503 s de WAV,
cola a −43,5 dB sin fade. Trayectoria 31,80 min; total con controles 36,56 min.
Reproducción del release a la misma tasa: estados 1,84e−14. Cierre de trabajo
mecánico 1,01e−12 relativo. Fuentes, R, tabla, audio y bandas autenticados;
ninguna historia o caché grande adicional. Audio sólo en SHARED.
64/128 pasan bandas rectangulares del tramo, pero **fallan Hann 6–12 kHz**
(263,445/51,758 %; referencia de energía muy pequeña, gate conservado).
Malla Hann pasa; malla rectangular falla energía (0,12454 % frente a 0,1 %).
Escucha diagnóstica, sin aprobación global. Sigue frase corta/F4 y conservar
los pendientes de precisión; evaluar el audio antes de cambiar más física.

La [entrada musical y demanda de las piezas](RESULTADOS-P6-entrada-MIDI.md)
ya conserva tempo, notas, velocidades y controladores de los MIDI originales.
Dos parsers coinciden en 6028/11382 eventos; cuantización máxima 1,3021 µs.
Cinco pruebas pasan: repetición/pedal/release por bloques conserva estados
idénticos, y el nuevo constructor reproduce las matrices del banco anterior.
Nocturno/Campanella necesitan 70/75 alturas; una matriz mecánica actual
ocuparía 49,18/56,40 GiB. Captura monolítica: 259,56/318,04 GiB de B/V.
Los primeros diez segundos de Campanella sólo usan D#4/D#5/D#6/D#7:
se construye ese banco de 5218 coordenadas y doce cuerdas. Una sonda real
de ataque de 6 ms pasa reloj/captura/ledger mecánico (2,809e−11 relativo),
con buffer de 257 nudos; tarda 42,516 s y conserva el coste alto.
Misma tabla/R, sin audio ni cachés nuevos. **Pendientes para escuchar ese
fragmento:** CC64 continuo y pedal suave CC67 (ambos aparecen en el MIDI).
La salida continua queda verificada en la etapa siguiente. Después ampliar duración con coste medido;
para la pieza entera, eliminar matrices globales densas. Gates globales falsos.

La [presión y escritura WAV por bloques](RESULTADOS-P6-presion-bloques.md)
ya mantiene convolución, fase de resampleo y cola sin capturar toda la pieza.
Siete pruebas pasan, incluida reproducción física con repetición y pedal;
los estados finales no cambian. La misma sonda de cuatro teclas/606 modos
coincide con el observador continuo (máximo por banda 8,1401e−9) y el audio
anterior (0,00001123 %). Transferencia reutilizada 14,43 MB, cola estéreo de
450 frames; no hay nueva caché grande o WAV de escucha de ese ataque de 6 ms.
Verificación independiente de archivos/DTFT/resampleo/ledger aprobada.
La malla 12,5→6,25 Hz **falla energía en 800–2500 Hz** (0,17854 %, límite
0,1 %); se conserva, sin aprobación global ni temporal de una pieza.
La trayectoria de 6 ms tarda 43,94 s: permanece el costo mecánico.
La etapa de CC64 continuo/CC67 se registra a continuación; luego fragmento musical.

La [acción continua de pedales](RESULTADOS-P6-pedales-continuos.md) conserva
los valores CC64/67 y los lleva a posiciones prescritas, con supuestos de
geometría y 30 ms de viaje. CC64 regula la precarga del parche disipativo;
CC67 desplaza el área de contacto de tres cuerdas a dos, conservando las
tres cuerdas y la ley local del fieltro. La acción de CC67 registra trabajo
con signo cuando hay compresión. El solver actualiza sus coeficientes en
caché; el control rechaza un solver sin esa sincronización.
Ocho pruebas pasan: posiciones/medio pedal, contacto/trabajo, pre-roll frío,
estados idénticos con distintos bloques y regresiones del reproductor/WAV.
La geometría lateral y el apagador son supuestos; no se identifica el
Steinway ni se modelan masas de los actuadores, fieltro lateral o sostenuto.
Las dos sondas del banco de cuatro teclas/606 modos pasan trabajo local
(residuo máximo 1,062e−10 relativo; límite 1e−8); fuente/plan/MIDI/arrays,
resampleo y aritmética se verifican independientemente. Trayectorias de
6 ms: 44,96/46,91 s. La acción móvil transfiere +1,545e−6 J, registrado
por separado; no se aprueba el timbre o la convergencia de una pieza.
Sin nuevas R ni WAVs diagnósticos de 6 ms en SHARED. Sigue un motivo corto del MIDI original,
con presupuesto medido de ataque/coast y conservando los gates pendientes.

El [primer motivo original con pedales](RESULTADOS-P6-motivo-MIDI.md) ya se
puede escuchar: cuatro ataques suaves de La Campanella, tiempos/velocidades y
note-off originales, CC64/67 originales y un cierre CC64=0 declarado a 0,72 s.
Se integran 0,96 s con estados compartidos; WAV único de 1,064 s en
`C:/SHARED/Piano-motivo-MIDI-2026-10-11/Campanella-inicio-fisico.wav`.
La primera ejecución detecta una frontera B/V perturbada por una spline
redundante; pasar los nudos uniformes nativos conserva fronteras bit a bit.
Cinco pruebas pasan, incluida la regresión de 120 bloques, sin relajar el
criterio de continuidad. Sólo se reemplaza el WAV parcial de ese intento.
Verificación de fuentes/MIDI/audio, eventos, reloj, frames y balance escalar
aprobada: residuo relativo 1,700e−13, cola final −85,877 dB respecto al pico,
sin fade ni normalización. Todos los apagadores modelados terminan apoyados.
Dinámica 87,809 min; productor completo 89,258 min. Ese costo exige perfilar
y acelerar la integración antes de extender el fragmento. No hay aprobación
global, identificación del Steinway ni nuevo paquete; los gates de malla,
temporales y espaciales anteriores siguen pendientes.

La primera frase SI fue rechazada por timbre oscuro y cortes secos. El
refinamiento temporal no validó la cobertura modal de la tabla hasta 6 kHz.
Se prioriza esa cobertura y su carga acústica antes de otra escucha musical
del modelo. Se agregó un apagador resistivo distribuido como control de
release, con geometría/pérdidas supuestas; no resuelve el timbre.
Ver [diagnóstico, medidas y criterios](RESULTADOS-P6-release-y-banda.md).
La [ampliación estructural](RESULTADOS-P6-banda-alta.md) pasa el refinamiento
de admitancia/presión hasta 6 kHz en el sector vertical simétrico de la tabla
supuesta. La [proyección pasiva](RESULTADOS-P6-proyeccion-pasiva.md) conserva
esa flexibilidad con 606 coordenadas y pasa puertos/frecuencias reservados.
La carga recíproca de 606 pasa la grilla armónica/Laplace y prefijos cortos.
Faltan transitorios completos y presión temporal antes de volver a escuchar
el modelo completo.

El [control de coste y propagación](RESULTADOS-P6-propagacion-coste.md)
reproduce el contacto y extiende el recorrido fino a 3,5 ms. La aplicación
de la carga completa reduce el coste de dinámica 15–17%; el operador de los
dos micrófonos se prepara unas 6,55 veces más rápido y pasa equivalencia
y Green continuo. El balance angular independiente y la presión del coast
corto ya se midieron en la etapa siguiente; eso no selecciona una tasa
para una nota audible completa.

El [balance angular y presión del coast](RESULTADOS-P6-coast-campo.md)
cierra independientemente 0,5 ms después de ese ataque: trabajo fino cambia
0,04966%, presión 0,00122% y captura 0,00000111%. Un diagnóstico interpolado
de orden falla 0,37184; se conserva. La reproducción exacta en tiempos
nativos comunes recupera 0,32125, con el límite 0,35 original. Pasan los
controles vinculados de ese tramo. Antes de escuchar quedan duración,
coste del certificado y resolución de cuerda: este banco usa tres modos
transversales por dirección y seis axiales, no el banco audible amplio.

El [barrido de cuerda](RESULTADOS-P6-resolucion-cuerda.md) amplía a 48 modos
por dirección y pasa 11/13 controles. La fuerza máxima cambia con la base
sin modificar el fieltro. Fallan 24→48 en presión alta (1,506794%, límite 1%)
y contacto 768→1536 kHz (0,101373%, límite 0,1%); el banco aún no se adopta.
El [ensayo dirigido](RESULTADOS-P6-resolucion-dirigida.md) separa ahora las
dos resoluciones en una matriz 48/96 modos × 1536/3072 kHz, conservando
la carga completa y todos los límites. Terminó **14/14 controles**: presión
alta espacial máxima 0,038670% (<1%), temporal 0,025344% (<0,1%). Se usa el
caso fino como referencia temprana para validar banda, coast/campo y duración;
todavía no cambia el paquete de fábrica ni se aprueba una nota completa.
El [ensayo de campo refinado](RESULTADOS-P6-coast-refinado.md) reconstruye
ese contacto de referencia y mide independientemente el coast de 96/192:
64/128/256 kHz desde el mismo extremo vivo, con comparaciones nativas de
tabla, presión y certificado angular completo. Sondas espectrales separadas
comprueban campo complejo y flujo sin sustituir la ruta temporal establecida.
Termina **15/16**: pasa presión (máximo fino 0,00311481%), pero falla el
refinamiento de radiación (0,274024% con cotas, límite 0,2%). Ese fallo
motivó añadir 512 kHz antes de ampliar duración. El prototipo espectral pasa
fase/flujo en dos bloques; no demuestra ahorro de tiempo ni se adopta.
El [refinamiento a 512 kHz](RESULTADOS-P6-coast-512.md) cierra **15/15**:
flujo 256→512 **0,0690548%** con cotas (<0,2%), presión fina máxima
**0,000761170%**. Se autentica y reutiliza un único estado completo de
6,31 MB, evitando repetir 16,99 min de contacto. Sólo se valida este
coast de 0,5 ms; siguen banda superior de fuente/carga y balance angular
del contacto completo antes de ampliar duración. 128→512 sigue fallando.

| ID | Mecanismo | Representación actual / falta | Estado |
| --- | --- | --- | --- |
| P1a | Admitancia direccional del puente | La horizontal usa la misma familia de modos con un factor fijo; falta una segunda proyección mecánica independiente. | Piloto medido: pasivo, sin mejora consistente; escucha pendiente |
| P1b | Dos polarizaciones por cuerda | Fábrica: tres verticales y una horizontal agregada. Prototipo: una horizontal independiente por cuerda, con proyección equivalente. Falta identificar propiedades direccionales reales. | Piloto medido: equivalencia verificada, sin mejora consistente |
| P2 | Tabla ortótropa con costillas y puente | Piloto anterior: modos físicos bajo 800 Hz y complemento alto heredado. Ensayo SI actual: tabla física proyectada a 606 coordenadas, compartida por nueve cuerdas. Geometría y acústica aún sin identificar. | Integración audible parcial; no seleccionada para fábrica |
| P3 | Intercambio no lineal transversal/longitudinal | Núcleo SI Euler extensible, dos direcciones transversales y movimiento axial por cuerda. Ensayo actual: nueve cuerdas, contacto sobre movimiento vivo y soporte vertical recíproco compartido. | Primera frase física C4/G4/C5 audible; banco y convergencia polifónica pendientes |
| P3l | Pérdidas propias de las cuerdas | El núcleo SI disipa por tabla/aire, fieltro y apagadores; no tiene pérdidas internas del acero ni arrastre aéreo directo de cuerda. | Pendiente; parámetros por identificar |
| P4 | Dispersión Timoshenko | Escalera rígida simplificada; faltan cizallamiento e inercia de rotación. | Pendiente |
| P5 | Contacto integrado con el puente | Fábrica: sin devolución de movimiento durante el contacto. Ensayo SI: tres contactos de cuerda por cabeza, tres teclas y una tabla, con inercia compartida y trabajo de relanzamiento registrado. Faltan las demás direcciones y el teclado completo. | Parcial: unísonos/tabla vertical compartida audibles; identificación pendiente |
| P6 | Radiación y carga acústica del aire | Tabla 132×112 y proyección 7392→606 verificadas hasta 6 kHz; Padé2/derivada cuarta pasa masa, 83 frecuencias y tres puntos Laplace. Contacto de 3 ms y coast de 0,5 ms de C4 supuesto conservan todos los estados. El barrido 3→48 pasa 11/13; la matriz dirigida 48/96 × 1536/3072 kHz pasa 14/14. Coast 128→256 falla flujo 0,274024%; el ensayo 128/256/512 pasa 15/15: flujo fino 0,0690548% con cotas frente a 0,2%. Estado físico único autenticado reutilizable. Faltan banda superior de fuente/carga, balance angular del contacto completo, duración, repetición/release, identificación y ambas caras/borde. | Coast de 0,5 ms a 256 frente a 512 convergente; banda superior, nota completa y timbre pendientes |

El orden es una prioridad de investigación, no una afirmación de causalidad.

La escucha actual de dos segundos utiliza una reducción acústica pasiva a
192 direcciones de estado, con error de presión heredado de 9,202 % frente
al límite de 5 %; no reemplaza los certificados del operador completo ni
valida la presión de todos los nuevos estados cargados. Véanse el
[ensayo compartido](RESULTADOS-P6-tabla-compartida.md) y sus límites. El
estado histórico P6 de la tabla anterior describe el operador completo.
P1 se relaciona con el sostenido/unísono percibido; P2 puede afectar el cuerpo
y la irregularidad del teclado; P3 la evolución con intensidad; P4 principalmente
la dispersión de parciales altos. Cada efecto debe verificarse al implementarlo.

## P1: diseño del ensayo

### P1a: dirección antes de aumentar estados

Conservar inicialmente la horizontal agregada permite aislar el cambio de
admitancia sin cambiar a la vez afinación, reparto del martillo y pérdidas.
El resultado **no completa P1b** ni representa seis estados físicos independientes.

Se prueba una hipótesis de puente rígido elevado sobre una superficie flexible:
la fuerza lateral en el punto elevado ejerce un momento sobre la superficie.
Para un modo de tabla, su desplazamiento local y su pendiente dan dos
proyecciones distintas. Cada contribución de admitancia se construye como
producto exterior de esas proyecciones y una masa positiva compartida; la
parte disipativa permanece semidefinida positiva. La radiación debe recibir
la misma combinación de fuerza y momento que usa la carga.

La geometría de puente, orientación lateral y formas modales son supuestas.
Una altura de ensayo no es una altura medida del Steinway. Se conserva la
proyección horizontal heredada y se añade una contribución de palanca para
medir sensibilidad. No se presenta como identificación completa del puente.

Controles necesarios:

1. Interruptor apagado: huellas de fábrica preservadas.
2. Palanca nula en el cálculo: equivalencia con la carga escalar heredada.
3. Admitancia contra una exponencial matricial independiente y balance de energía.
4. Misma proyección de momento en carga y excitación de tabla.
5. C4/C5/C6/F#6 a velocidades medias y altas, sin detune adicional; ataque,
   cuerpo y movimiento de color durante el sostenido. Escucha con referencia.
6. Después de un resultado útil: extender registros, velocidades y pedales.

### P1b: estados de cada cuerda

Separar las horizontales conservando energía y fuerza en el límite equivalente.
Los parámetros deben referirse a cada cuerda completa, no a detune independiente
por parcial. Comparar separación de estados sola y admitancia direccional sola
antes de combinarlas. Comprobar qué configuraciones oscuras se excitan y cómo
radian. No asumir que agregar estados mejora el sonido por sí mismo.

## Ensayos previos que condicionan el plan

- [B/piloto I](../concert-grand-unison-flanger-2026-10-02/README.md): separación
  de afinación más pérdidas secundarias menores produce movimiento de balance
  entre parciales; rechazado por oído. El conteo de mínimos tardíos no bastaba.
- [Carga y memoria](../concert-grand-decay-loading-2026-10-02/README.md): quitar
  drenaje recupera sostenido en C4 pero es diagnóstico. La memoria pasiva con
  la misma tabla no recuperó el déficit grande; no repetirla como solución única.
- [Unísono sutil](../concert-grand-subtle-unison-2026-10-02/README.md): el 2×2
  espacial probado era fuerza común y momento por separación de pines; **no**
  dos direcciones de vibración. No confundir ese ensayo con P1a/P1b.
- [Tabla y radiación](../concert-grand-joint-radiation-2026-10-01/README.md):
  ajustes conjuntos de la base modal sintética no generalizaron. Cambiar su
  geometría física en P2 es una hipótesis distinta, no otra búsqueda del mismo banco.
- [Cuerpo identificado](../concert-grand-identified-body-2026-10-02/README.md):
  el control anterior suavizó el ataque y fue rechazado. Conservar el ataque
  como criterio audible, además del sostenido y score por armónicos.

## Fuentes primarias

- [Bensa y colaboradores, ICMC 1999, Resynthesis of Piano Strings Vibrations](https://quod.lib.umich.edu/cgi/p/pod/dod-idx/resynthesis-of-piano-strings-vibrations-based-on-physical.pdf?c=icmc%3Bidno%3Dbbp2372.1999.335%3Bformat%3Dpdf): acoplamiento entre polarizaciones.
- [Bank, admitancia matricial pasiva](https://home.mit.bme.hu/~bank/publist/dafx10adm/index.html): construcción pasiva multidimensional. Su ejemplo es guitarra; aquí se usa el principio mecánico, no se afirma haber reproducido ese instrumento.
- [Ege y Boutillon, descripciones global/local de tabla](https://arxiv.org/abs/1210.5109): transición a comportamiento localizado entre costillas.
- [Chabassier, Chaigne y Joly, piano completo](https://perso.ensta-paris.fr/~touze/PDF/Batwoman/chabassier-jasa.pdf): cuerdas no lineales Timoshenko, tabla ortótropa y acoplamientos.

## Registro de etapas

### 2026-10-02 — Inicio P1a

Auditoría del código y definición del control direccional. No se ha aprobado
un sonido nuevo.

### 2026-10-02 — P1a medido

[Resultados y reproducción](RESULTADOS-P1a.md). Se probaron 10/25/40 mm de
palanca supuesta en 16 casos y cuatro sostenidos largos. Pasividad, equivalencia
de palanca nula, proyección de momento y oráculo matricial independientes pasan.
El control apagado coincide exactamente con los 16 audios de fábrica congelados.

La variante de 10 mm cambia muy poco el error medio y empeora el peor caso.
25/40 mm empeoran el error medio. El ataque cambia poco y no se recupera
consistentemente el cuerpo. Aumentar la palanca introduce más evolución de
balance entre parciales agudos. **Sin mejora demostrada; ninguna seleccionada.**
Esto no descarta la admitancia direccional real: el ensayo conserva tabla
sintética y horizontal agregada, con geometría supuesta. Tampoco verifica P1b.

Audios y copia del plan: `C:\SHARED\Fisica-pendiente-2026-10-02`.
Los comparadores alternan base / palanca 25 mm / Steinway a velocidades
28/60/92/124. La escucha del usuario permanece pendiente. No se instaló ni
empaquetó el prototipo.

### Diseño aplicado — P1b

1. Crear dos estados transversales por cuerda, con masa/energía explícitas.
2. Levantar la horizontal agregada al subespacio común de las horizontales
   individuales. Con afinación, pérdidas y excitación iguales, preservar la
   fuerza y el sonido del control; no duplicar energía al copiar estados.
3. Dar a cada cuerda una sola afinación/rigidez para toda su escalera. Mantener
   primero los valores de fábrica, sin añadir dispersión por parcial.
4. Separar ensayos de excitación diferencial y de carga direccional. Registrar
   lo que puede radiar y lo que permanece oscuro. Si sólo cambia el número de
   estados y el sonido es idéntico, registrarlo como equivalencia, no como mejora.
5. Repetir intensidades y sostenidos C4/C5/C6/F#6; observar energía, ataque,
   cuerpo y movimiento de color. Sólo ampliar o recalibrar tras un resultado útil.

### 2026-10-02 — P1b medido

[Resultados y reproducción](RESULTADOS-P1b.md). Prototipo nativo con una
horizontal por cuerda y ensayos separados de reparto de martillo, afinación
mínima y palanca. Se conservó la energía horizontal inicial del modelo y su
proyección conjugada, no se copió la energía tres veces.

El control equivalente tiene error RMS relativo máximo de −70,756 dB en
16 casos; el control de una cuerda es idéntico, el de dos cuerdas difiere
−72,079 dB y el de reiteraciones/pedal/medio pedal −74,050 dB. La fábrica
apagada conserva 16 audios exactos y sus huellas. Cinco pruebas nuevas pasan.

El reparto de martillo cambia el score medio de 6,83418 a 6,83538; no recupera
consistentemente cuerpo ni ataque. Añadir afinación mínima produce evolución
de color principalmente por la afinación; la palanca conserva la regresión de
P1a. **Sin mejora demostrada; ninguna seleccionada.** Audios y metadatos en
`C:\SHARED\Fisica-pendiente-2026-10-02\P1b-polarizaciones`.

Con proyecciones idénticas y frecuencias horizontales coincidentes, las
diferencias entre horizontales permanecen oscuras: tener más estados no basta
para hacerlas audibles. Los estados ya permiten ensayar una tabla y geometría
distintas en P2. El piloto no identifica masas, admitancia lateral ni contacto
tridimensional real; tampoco sustituye el intercambio no lineal pendiente en P3.

### Diseño aplicado — P2

Construir primero una placa ortótropa con costillas discretas en coordenadas
modales. Verificar masa y rigidez positivas, convergencia de frecuencias,
formas y proyecciones, y reciprocidad entre carga y excitación. Registrar
dimensiones, material, apoyos y orientación supuestos. No afirmar geometría
identificada del Steinway a partir de WAVs. Conservar una placa sin costillas
como control analítico antes de conectarla al renderer.

### 2026-10-02 — P2 estructural y acústico medidos

[Resultados y reproducción](RESULTADOS-P2.md). Placa rectangular ortótropa,
costillas discretas y puente como vigas. Masa/rigidez positivas, comparación
analítica, reciprocidad y pasividad verificadas. Convergencia por enriquecimiento
Ritz del banco con costillas: cambio máximo de frecuencias 0,3415% para 256 modos
y de movilidad 0,51% entre 250 y 1000 Hz en los puntos probados. Estos controles
no identifican geometría del Steinway ni resuelven la carga acústica.

Se conectaron tres variantes bajo 800 Hz, conservando los 121 modos altos de
fábrica, con proyecciones compartidas de carga, excitación y superficie.
Dieciséis ataques y cuatro sostenidos por variante; controles apagados exactos.
Además, cuatro ataques fuertes por variante con el observador acústico causal.
La mejora de C4 con placa y puente en el observador habitual no persiste en ese
control causal. La variante con costillas recupera parte del fundamental
sostenido de C4, pero empeora C5 y el cuerpo del ataque. **Sin mejora global
demostrada; ninguna seleccionada.** No se cambió el paquete instalado.

Audios en `C:\SHARED\Fisica-pendiente-2026-10-02\P2-tabla-audio` y
`P2-tabla-causal`. Reportes originales, hashes y resumen conservados.

### Decisiones de seguimiento dentro de P2

1. Convergencia del banco bajo verificada en el ensayo siguiente; pendiente
   el complemento alto y la identificación del observador acústico completo.
2. Ensayar geometría global de puente y apoyos con controles separados.
   El puente recto centrado conserva simetrías y deja modos sin excitación;
   el puente real curvo puede cambiar esas proyecciones. Es una hipótesis
   pendiente, no la causa ya demostrada del déficit de C5.
3. Separar errores de carga mecánica de errores de proyección al micrófono.
   No elegir una tabla sólo porque aumenta graves o baja el score medio.
4. Antes de promover: extender registros, pedales/reiteraciones, estados durante
   cambios de programa, headroom interno y coste en tiempo real/WASM.
5. Mantener P3/P4/P5 en el seguimiento. P3 debe auditar los términos cruzados
   anotados debajo; no confundir esa corrección pendiente con el resultado P2.

### 2026-10-02 — P2 resolución y curva global medidas

[Resultados y reproducción](RESULTADOS-P2-geometria.md). Refinar la cuadratura
baja cambia poco la presión; 16 puntos de puente producen hasta ≈0,806 dB
de error en los casos armónicos probados, insuficiente para explicar el
déficit tardío de C5. No se retocó la fábrica por este resultado.

El puente recto centrado deja 16/35 modos bajos casi sin excitación. Se probó
una curva global supuesta de 10 cm con masa, rigidez y formas correspondientes,
y un control que mueve sólo la trayectoria. Con observador causal en todas
las variantes, la curva consistente mejora frente al recto, pero C4/C5 siguen
peor que la base causal; el promedio 7,75351 no supera la fábrica habitual
(6,83418). **Sin selección.** Tres nuevas verificaciones nativas pasan; los
controles fuertes anteriores son exactos. Audios en SHARED.

La separación de rutas identifica en C5 una carga de puente 5,361 veces la base,
además de una transferencia de presión 4,265 dB menor por fuerza fija. No es
una medición del Steinway. Pendiente P2: masa/geometría y rango de validez de
la aproximación de carga débil a frecuencia congelada, especialmente cerca
de picos. No reducir pérdidas arbitrariamente para cubrir esos casos.

Siguiente etapa del orden: **P3**, auditoría de energía y normalización por
cuerda. Los pendientes de P2 permanecen registrados; no se declara tabla
completa o identificada antes de seguir con otros mecanismos.

### Incertidumbre adicional a auditar en P3

El comentario de `Voice::tension_step` describe suma de pendientes cuadradas
por cuerda, pero el cálculo actual eleva al cuadrado una suma de estados.
Eso contiene términos cruzados. Auditar su interpretación, normalización y
balance de energía dentro de P3; no modificarlo como arreglo aislado sin control.

### Diseño del siguiente ensayo — P3

1. Explicitar unidades y energía de estados de cuerda, masa modal, tensión,
   fuerza de puente y coordenadas de tabla. La norma modal heredada de P1b
   no basta para afirmar conservación en julios.
2. Derivar por cuerda el incremento de longitud: suma de energías de pendiente,
   separando direcciones/polarizaciones. Comprobar qué términos cruzados son
   físicos dentro de una cuerda y cuáles aparecen sólo al sumar cuerdas distintas.
3. Auditar la tensión común actual y el paso de actualización cada 32 muestras;
   identificar el efecto del suavizado antes de sustituirlo.
4. Ensayo conservativo aislado sin pérdidas: balance energético con referencia
   independiente y convergencia temporal. Después, pérdidas positivas y
   intercambio transversal/longitudinal bidireccional.
5. Controles congelados de ataque, sostenido y evolución con intensidad en varios
   registros. No mezclar a la vez una nueva afinación, geometría de tabla y contacto.
   El cambio de energía debe explicar una variación audible antes de hacer refit.

### 2026-10-02 — P3 energía, geometría y precisión medidas

[Resultados y reproducción](RESULTADOS-P3.md). Se construyó un núcleo aislado
con dos direcciones transversales y movimiento axial, derivado de un potencial
compartido en julios. Conserva energía y contabiliza las pérdidas positivas.
La convergencia contra un integrador independiente muestra que conservar energía
no basta: a 44,1 kHz el caso resonante conserva mal la fase; a 32 veces esa tasa
el error final de estado ponderado por energía baja a 0,6884% en 12 ms.
Ese núcleo todavía no está integrado ni identificado con el Steinway.

Los controles nativos de estiramiento y fuente longitudinal por cuerda no
mejoran las diez notas/intensidades medidas. El inspector encuentra tensión
solicitada sin cambio representable de polos en C5/C6/F#6 fuertes durante los
primeros 120 ms. C4 tiene cambios sólo en algunos parciales, hasta 0,0004343
cents; F#2 llega a 0,01527 cents. Es una limitación medida del cálculo actual,
no prueba de que la no linealidad física real sea despreciable.

**Ninguna variante seleccionada.** Fábrica y paquete preservados. Cuatro pruebas
algebraicas, balance/convergencia físicos y ambas huellas nativas pasan.
Reportes, CSV y audios en `C:\SHARED\Fisica-pendiente-2026-10-02\P3-*`.

Siguiente trabajo dentro de P3:

1. Derivar y medir la conversión del contacto SI a desplazamiento/velocidad por
   cuerda y fuerza de puente. Hoy hay normalización empírica entre esas etapas.
2. Mantener una frecuencia de referencia explícita y un desplazamiento acumulado
   representable; evitar perder cada incremento pequeño al redondear el polo.
   Validar con una trayectoria prescrita antes de activar la no linealidad.
3. Elegir integración con energía y fase verificadas a coste viable; no trasladar
   directamente el ensayo implícito a 32× al callback.
4. Conectar intercambio axial/transversal bidireccional y carga conjugada con
   controles de potencia, contacto, reiteraciones y pedal. Retener los pendientes
   de P2 sobre carga débil cerca de resonancias.
5. Repetir comparación por registro/intensidad; sólo después considerar refit.

Los crudos intermedios propios de P2 se archivaron de forma reversible en
`local/concert-grand-ritz/P2-native-raw-cache.zip`, con hashes individuales
verificados en `P2-native-raw-cache.json`. Los WAV, reportes, fuentes y binarios
continúan en sus ubicaciones; no regenerar todo P2 sin necesidad.

### 2026-10-02 — P3 contacto y referencia acumulada medidos

[Resultados y reproducción](RESULTADOS-P3-contacto.md). El nuevo control conserva
una referencia de frecuencia y radio en doble precisión; no descarta cada
incremento por redondeo del polo. Dos verificaciones nuevas pasan, incluyendo
volver exactamente a la base, respetar la pérdida del apagador y no reiniciar
polos retirados. Los coeficientes finales siguen cuantizados a `f32`.

Veinte controles apagados son exactos frente a los audios P3 anteriores. La
referencia acumulada cambia muy poco el sonido: score medio 6,51316 frente
a 6,51306 de fábrica; la geometría por cuerda con referencia llega a 6,51312.
**Sin selección.** La corrección numérica sola no recupera cuerpo.

El contacto SI termina por separación en los diez casos, sin alcanzar el cap
de 20 ms. Su conversión a estados audibles varía con nota, parcial y velocidad:
en el fundamental de F#6, pasa de 6407 a 443 unidades nativas por metro entre
velocidades 60 y 124. El piloto dispone ahora de coordenadas de masa, energía,
incremento de longitud y fuerza de tensión en SI con controles espaciales y
de trabajo. Todavía no se utiliza para controlar la síntesis audible.

Se identificó además una subida inicial de afinación dibujada en graves que
sigue activa: F#2 fuerte arranca 3,402 cents por encima del caso suave. El
contacto deriva la masa lineal de ese `f0` ya elevado, reduciendo su masa
modal 0,3923% al cambiar velocidad. Los contadores de descenso están en cero;
no es el intercambio no lineal nuevo. Corregir la representación física usando
la escala de referencia y evitar duplicar esa subida al integrar P3.

Siguiente ensayo de integración: conservar estados físicos del contacto antes
de la normalización, separar su conversión al micrófono y contabilizar el
trabajo de inicialización no lineal. El contacto actual es lineal en las
cuerdas: añadirle energía de estiramiento o un estado axial arbitrario al
terminar puede crear un ataque artificial. Validar ese paso antes de comparar
otro candidato completo o promover la referencia de polos.

### 2026-10-03 — P3 contacto acoplado desde el inicio

[Resultados y reproducción](RESULTADOS-P3-contacto-acoplado.md). El martillo,
fieltro y movimiento transversal/axial comparten energía desde primer contacto.
Al separarse se conserva el estado sin normalización ni energía añadida.
C4/C5 a velocidades 60/124 pasan balance, pérdidas positivas y convergencia
contra DOP853: error final de estado entre 0,00274% y 0,06952% a 705600 Hz,
en 6 ms. Al refinar el paso temporal, el error cae aproximadamente cuatro veces.

La resolución modal permanece abierta: de 2 a 8 modos axiales, la energía
de cuerda cambia +1,26%/+2,26% en C4/C5 fuertes y su reparto axial cambia más.
No identificar el mecanismo real a partir de un truncamiento pequeño.
No se produjo una voz audible nueva ni se modificó el paquete. Seis pruebas
Rust, exportación SI y dos huellas de fábrica pasan. Datos y seguimiento en
`C:\SHARED\Fisica-pendiente-2026-10-03`.

Siguiente paso: resolución y dispersión del contacto, reiteración desde un
estado físico existente y transferencia SI de potencia al puente/salida,
con masa independiente de velocidad. Conservar el control y una ganancia
común antes de comparar por oído o intentar otro refit. P2/P4/P5 continúan abiertos.

### 2026-10-03 — P3 resolución y golpes sucesivos

[Resultados y reproducción](RESULTADOS-P3-resolucion-reiteracion.md). Ocho
casos C4/C5/C6/F#6 a velocidades 60/124 pasan criterios previos de convergencia
de energía, impulso, tiempo, fuerza y estado en el piloto de extremos fijos
con flexión Euler. Se necesitan 2N axiales para cubrir la fuente de N modos
transversales; dos axiales no bastaban. Últimos refinamientos: 24→48 para
siete casos, 48→72 para C4 fuerte. Esto no completa Timoshenko ni identifica el piano.

Dieciséis controles conservan la cuerda ya vibrando e incluyen pérdidas
positivas. Dos muestran separación temporal con martillo aún avanzando;
seguir hasta el regreso cambia el intercambio de energía. La mecánica de
captura real sigue pendiente. Ocho golpes rápidos cierran el presupuesto
acumulado a 3,01e−14 relativo. El exportador nativo confirma que los diez
contactos de fábrica desde reposo terminan con martillo retrocediendo.

Se verificó una distinción en la reiteración nativa: la fase de la voz se
conserva, pero `simulate_strike` calcula el nuevo contacto con q/v inicialmente
cero. Su reacción al movimiento existente no está integrada durante el golpe.
Cinco pruebas Python, seis Rust, exportación y ambas huellas pasan; fábrica
y paquete preservados. Datos en `C:\SHARED\Fisica-pendiente-2026-10-03`.

Siguiente trabajo: conservar estado SI por cuerda y acoplar el puente móvil
con reacción recíproca, masa y potencia consistentes. La reacción calculada
con extremo fijo sirve de diagnóstico; excitar la tabla con ella sin reacción
no paga su energía. Validar la ruta de observación y usar ganancia común antes
de comparar sonidos. P2/P4/P5, backcheck, pedal y coste siguen pendientes.

### 2026-10-03 — P3/P5 puente móvil recíproco

[Resultados y reproducción](RESULTADOS-P3-puente-movil.md). La cuerda, el
martillo y el soporte vertical comparten masa y potencial. Se incluye la
inercia cruzada de mover el extremo de cuerda; la fuerza y velocidad de
interfaz verifican el trabajo que pasa al soporte. Ocho pruebas pasan.

Un soporte elemental y los 35 modos físicos bajo 800 Hz de P2 cierran
energía y pérdidas. En C4 fuerte, 24→48 aún falla el criterio de energía y
tiempo; 48→72 pasa. El error de velocidad del puente baja a 0,00560% RMS
con el oscilador y 0,00824% con P2. La resolución del resto del teclado
con extremo móvil no está verificada. Balance máximo: 1,423e-13 relativo.

La ablación de soporte excitado sin reacción recibe trabajo externo, que
debe contabilizarse aparte; no es una medición del error de la fábrica.
El soporte recibe poca energía durante el contacto en este golpe. No hay
evidencia de mejora audible ni un nuevo refit. Paquete preservado.

Siguiente etapa: observación de presión desde el movimiento SI de tabla,
con superficie/formas consistentes y ganancia común. Luego comparación por
registro/intensidad. Siguen abiertos banco alto, geometría real, unísono,
backcheck/pedal, masa de graves, Timoshenko e implementación en tiempo real.
Datos y copia del seguimiento en `C:\SHARED\Fisica-pendiente-2026-10-03`.

### 2026-10-03 — Commit y observación SI de superficie

Investigación acumulada del piano y sus herramientas/documentación guardada
en **346b6c8**, por pedido del usuario. El trabajo posterior del observador
continúa como etapa independiente.

[Resultados y reproducción](RESULTADOS-P3-observacion-SI.md). Observador de
presión desde las formas SI de la tabla, con retardos punto a punto y sin
ganancias ajustadas por nota. Seis pruebas independientes pasan, incluyendo
pistón analítico, temporal retardado, simetría y potencia radiada positiva.

La captura C4/124 de 4 ms conserva exactamente los estados congelados de la
etapa anterior. La presión en 60–800 Hz cambia 0,01020% RMS complejo al
refinar 48→72 modos de cuerda. Superficie 64×48 y retardo a 192 kHz pasan
los criterios declarados. No se produjo ni seleccionó un piano audible.

Se agrega **P6** para seguir la carga del aire: en el banco bajo P2 supuesto,
su radiación representa 16,78–70,31% de la disipación modelada bajo las cuatro
excitaciones armónicas de prueba. No es una medición del Steinway ni de la
fábrica. El observador lee movimiento, pero aún no devuelve carga acústica.
El 0,046 existente puede incluir radiación; separar pérdidas antes de añadir
resistencia, e incluir la parte reactiva para conservar energía del aire.

Siguiente trabajo: ensayo reducido recíproco tabla/aire con un control
analítico de pasividad/energía, y después fuente SI de mayor duración/banda
para escuchar. Banco alto, unísono, masa de graves, Timoshenko, backcheck/pedal
y coste siguen registrados. Datos en `C:\SHARED\Fisica-pendiente-2026-10-03\P3-observacion-SI`.

### 2026-10-03 — P6 carga reactiva y radiación recíprocas

La observación SI anterior quedó en **aaa520d**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-carga-aire.md). Se implementó la
impedancia exacta de una esfera pulsante como control analítico, con fuerza
recíproca, almacenamiento inercial y canal de onda saliente. Ocho pruebas
pasan, incluyendo solución de campo, pasividad, exponencial matricial y
contacto no lineal contra DOP853 independiente.

El soporte lineal supuesto cambia su pico de velocidad de 180 a 158,2 Hz al
cargarlo con ese puerto. Una resistencia constante sólo cambia amplitud en
ese control. No se trasladan esas frecuencias ni la geometría esférica al piano.
El elemento inercial puede devolver energía sin hacer negativa la radiación.

C4/124 con soporte elemental y esfera pasa resolución 48→72: error de
velocidad de soporte 0,00597% RMS, radiación 0,000717% y trabajo 0,00118%.
El control sin aire reproduce exactamente el estado congelado. El presupuesto
total recíproco cierra a 8,280e-15 relativo; una ablación sin reacción paga
su energía con un driver externo contabilizado. Fábrica/paquete preservados.

Siguiente: impedancia **compleja modal de P2**, autointeracción de superficie
y realización causal/pasiva con términos entre modos. No reemplazar la placa
por esferas por nota, ni duplicar masa/pérdida ya incluida en datos medidos.
Conservar estado acústico global entre golpes. Después ampliar banda/duración
para escuchar. Pendientes previos siguen abiertos. Datos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-carga-aire`.

### 2026-10-03 — P6 impedancia compleja de la superficie P2

El control de carga acústica anterior quedó en **3bbb170**, por pedido del
usuario. [Resultados y reproducción](RESULTADOS-P6-impedancia-tabla.md).
Resistencia y reactancia se calculan juntas desde el kernel saliente de
Rayleigh, conservando todos los términos entre los 35 modos físicos bajos.
Se integra la autointeracción y la interacción singular entre celdas vecinas.
Siete pruebas pasan, incluido un pistón circular con solución compleja analítica.

En seis frecuencias de 60–800 Hz, la resistencia coincide con un integral
angular independiente de la misma superficie a 1,470e-15 relativo. La malla
fina aproxima la superficie Ritz continua con error máximo de 0,07351%.
192×144→256×192 cambia la resistencia hasta 0,05718% y la reactancia hasta
0,11058%. La potencia armónica cierra a 6,093e-16 relativo. No se recortan
autovalores para hacer pasar pasividad. Son controles en puntos declarados,
no una prueba temporal global ni una medición del Steinway.

Siguiente: representación causal y pasiva de la matriz con estados acústicos,
validada en una banda más densa y contra la matriz compleja, manteniendo
interacción entre modos y energía almacenada. Separar pérdidas estructurales
de radiación antes de integrar contactos largos. Después ampliar banco,
unísono y pedales para escuchar con ganancia común. La fábrica y el paquete
continúan preservados. Datos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-impedancia-tabla`.

### 2026-10-03 — P6 estados acústicos y tabla cargada en el tiempo

La matriz compleja anterior quedó en **ec03076**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-estados-acusticos.md). Realización
espectral mediante canales oscilatorios recíprocos y masa de cola positiva.
Su identidad de energía establece pasividad; seis pruebas independientes
pasan. En 187 frecuencias de 60–800 Hz, el error máximo de impedancia compleja
es 0,22332% y el de velocidad de tabla cargada 0,20272% frente al control
Rayleigh de la misma superficie. La absorción del baño es numérica y se
documenta separadamente de pérdidas físicas y ondas almacenadas.

Dos impulsos separados 8 ms conservan el aire. El ensayo de 24 ms necesita
192→384 kHz para pasar el criterio temporal de 0,1%: velocidad cambia
0,03535% y desplazamiento 0,00867%. El balance total cierra a 6,168e−13
relativo, incluyendo trabajo instantáneo de la masa de cola. Hay devolución
de energía al puerto con absorción no negativa. No se promovió audio/paquete.

Siguiente: contacto no lineal SI con esta referencia, incluyendo la cola
inercial en la masa compartida. Luego reducir coste y verificar horizontes
largos y recurrencias antes de escuchar. El banco tiene 352026 estados de
aire y no es una implementación lista para tiempo real. Partición de pérdidas,
segunda cara/borde, banco alto, unísono, Timoshenko, graves y acción/pedal
siguen abiertos. Datos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-estados-acusticos`.

### 2026-10-03 — P6 contacto no lineal y aire persistente

Los estados acústicos anteriores quedaron en **e244450**, por pedido del
usuario. [Resultados y reproducción](RESULTADOS-P6-contacto-aire.md). El
contacto SI de cuerda/martillo/puente recibe la matriz completa del aire;
la masa de cola entra en la inercia compartida y se verifica el trabajo
entre cuerda, tabla y aire. Seis pruebas pasan, incluida una ODE independiente
en coordenadas físicas y el límite desacoplado contra el contacto previo.

En C4/124, 4 ms: 384→768 kHz cambia la velocidad de puente ≈0,01112% RMS,
y 48→72 modos de cuerda ≈0,00827%. Energías, trabajo, fuerza, impulso y
duración pasan sus criterios previos. El control sin aire aproxima la
velocidad final DOP853 congelada con error 0,04511% RMS. Un segundo martillo
prescrito conserva exactamente los estados vivos; sus balances cierran,
pero su refinamiento de precisión sigue pendiente.

La carga reduce 5,66307% la energía de tabla a 4 ms respecto del control
sin aire, mientras el primer contacto permanece prácticamente igual.
Esto no demuestra resolver el ataque fino. La referencia conserva una
cuerda, tabla hasta 800 Hz y una cara en bafle; no es audio de fábrica.
Datos y fuentes en `C:\SHARED\Fisica-pendiente-2026-10-03\P6-contacto-aire`.

Siguiente: reducción pasiva contra este banco completo, precisión de
contactos reiterados y horizontes largos. Luego ampliar banda, registros,
unísono y observación SI para escucha con ganancia común. La partición de
pérdidas, P4/Timoshenko, acción y pedal continúan pendientes. No se cambia
el paquete por la validación de esta referencia.

### 2026-10-03 — P6 reducción y rechazo por ecos tardíos

El contacto con aire anterior quedó en **6ebf47e**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-reduccion-aire.md). Cuatro pruebas
independientes pasan. Pesos de residuos PSD no negativos y cola PSD conservan
pasividad y masa estática. El entrenamiento usa sólo impedancias de la
referencia física, con verificación fuera del entrenamiento.

La variante de 17119 canales (10,28× menos) pasa frecuencia, tabla de 24 ms
y dos contactos C4 de 4 ms. Pero el control acústico de 64 ms con idéntica
velocidad prescrita detecta un eco a 29,0625 ms: error de fuerza 49,39123%
RMS y pico tardío 55,30072% del pico inicial. **Rechazada.** La absorción
alta baja el error pero también falla. No se cambiaron límites para aceptarla.

Conservar la grilla/Γ y descartar sólo direcciones espaciales débiles pasa
frecuencia y memoria: 0,29763% RMS, pico tardío 0,06273%. Reduce 30,29% los
canales; no cumple el objetivo de 10× ni tiene aún todos los contactos.
Ninguna variante se selecciona. Datos, fuentes y rechazos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-reduccion-aire`.

Siguiente: aproximación racional matricial con polos compartidos/adaptados,
certificación de pasividad y estos mismos controles de memoria. Contrastar
también la referencia finita con el kernel geométrico causal al ampliar el
horizonte. No volver a adelgazar la grilla suponiendo que pasividad o una
curva de frecuencia buena resuelven la memoria. Fábrica y pendientes de
banda alta, unísono, P4, pérdidas, acción/pedal continúan en el plan.

### 2026-10-04 — P6 carga racional y límite transitorio

La reducción anterior quedó en **77f00db**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-carga-racional.md). Un factor de
ondas de la superficie, con pares angulares invertidos y transformación de
Cayley, genera una carga recíproca pasiva con almacenamiento verificable.
Seis pruebas nuevas pasan; las 22 de esta etapa y sus referencias pasan.
No se implementa Vector Fitting ni se ajustan muestras/programas.

Una cola de masa ya PSD permite 5740 estados frente a 352026 (61,33× menos).
Z/R/X/Laplace, masa y velocidad cargada pasan los límites previos bajo 800 Hz.
El balance del factor físico cierra por debajo de 7,22e−16 a tres pasos.
La memoria de 64 ms ya no tiene el eco recurrente anterior. El RMS de fuerza
v1 de 5,29358% quedó invalidado por una doble resta de cola y una convención
de derivadas inconsistente; la etapa siguiente corrige el comparador. La
memoria prescrita no es una nota larga acoplada. No se promovió la variante.

Fuera de la banda baja, el diagnóstico geométrico detecta error complejo
20,63944% a 6 kHz en esa variante. La cola inercial tampoco tiene el límite
resistivo finito del Rayleigh físico; una corrección positiva aditiva no
siempre es posible. Esto explica limitaciones del método, sin atribuir todavía
todo el error a un único mecanismo. No basta aumentar factor_rate.

Siguiente: contrastar el transitorio de ambos modelos con el kernel causal
geométrico directo y completar la banda alta respetando masa, resistencia
límite y pasividad. Después retomar tabla/contactos reiterados, precisión
del segundo contacto, demás registros y escucha. No se modifica fábrica;
coste real, unísono, P4, pérdidas, acción/pedal y ambas caras siguen abiertos.
Datos en `C:\SHARED\Fisica-pendiente-2026-10-03\P6-carga-racional`.

### 2026-10-04 — P6 kernel geométrico y comparador corregido

La carga racional anterior quedó en **de2cb2e**, por pedido del usuario.
[Resultados, corrección y reproducción](RESULTADOS-P6-kernel-geometrico.md).
Se integran directamente los solapamientos de celdas sobre círculos de radio
ct: referencia continua de Rayleigh, términos locales y soporte físico de
5,0159331 ms. Masa, Z/Laplace, límite resistivo y refinamientos temporales
se verifican independientemente. Pasan 22 tests de esta etapa y referencias.

La fuerza del baño ya incluía su cola inercial regular: el comparador v1 la
descontaba otra vez. Se corrige a fuerza total con derivada izquierda en
los nudos, sin las deltas de parada. El mejor RMS contra el baño pasa a
3,58061%; los coeficientes y certificados previos conservan su validez.
Los archivos originales quedan congelados y los informes advierten la corrección.

El baño finito difiere 3,97163% de la referencia geométrica, principalmente
tras la parada brusca. La racional fina difiere 1,94783% con celdas 128×96,
1,98033% con 192×144 y 1,99520% con 256×192. El último cambio de referencia
es 0,05533%, mayor que el margen final del candidato hasta 2%.
**No se declara un pase robusto ni se selecciona para fábrica.**

Siguiente: mejorar el margen de la carga compacta, respetando masa, límite
alto y pasividad; después repetir tabla/contactos con estados persistentes.
El kernel geométrico es un evaluador de puerto prescrito, aún sin integrador
acoplado certificado. Banco alto, unísono, P4, pérdidas, acción/pedal y ambas
caras/borde siguen abiertos. No se modificó audio ni paquete instalado.
Datos en `C:\SHARED\Fisica-pendiente-2026-10-04\P6-kernel-geometrico`.

### 2026-10-04 — P6 carga limitada y tabla persistente

La referencia geométrica anterior quedó en **c353719**, por pedido del usuario.
[Resultados, construcción y reproducción](RESULTADOS-P6-carga-limitada.md).
Un promedio de taps conserva el factor positivo y anula su extremo de Nyquist.
Una relajación matricial de residuos ya positivos completa masa DC y
resistencia infinita sin cola inercial ilimitada. No hay ajuste de muestras
ni reparación de eigenvalores. Cuatro tests nuevos comprueban límites,
parada contra ODE, trabajo de estados arbitrarios y tabla/aire acoplados
contra una ODE física independiente; las 15 pruebas de esta etapa pasan.

El candidato fino tiene 5810 estados. El RMS de fuerza frente a geometría
pasa de 1,94783–1,99520% a **0,88913–1,01116%** en las tres grillas, con
normalizador y umbrales congelados. El margen ahora supera el cambio espacial
medido. Las variantes angulares gruesas siguen rechazadas. La masa/límite
son completados algebraicos; no bastan solos para aceptar la transferencia.

El error complejo a 6 kHz baja de 20,63944% a 5,54673%, pero sigue fallando
el control de 2%; también falla 4,5 kHz (2,31845%). La tabla se prueba con
dos impulsos a 0/8 ms durante 24 ms, a 192/384 kHz, preservando estados vivos;
los balances y refinamiento se publican en el informe y su resumen.
No es todavía el contacto no lineal SI ni un banco de notas de fábrica.

Siguiente: refinar representación temporal/angular respetando masa/límite,
pasividad y memoria, resolver transferencia alta y llevarla al contacto SI
persistente. Fábrica, pérdidas identificadas, ambas caras/borde, unísono,
P4 y acción/pedal siguen abiertos. Datos y fuentes en
`C:\SHARED\Fisica-pendiente-2026-10-04\P6-carga-limitada`.

### 2026-10-04 — P6 resolución polar y transferencia hasta 6 kHz

La carga limitada anterior quedó en **5834c39**, por pedido del usuario.
[Resultados, rechazos y reproducción](RESULTADOS-P6-resolucion-aire.md).
Un generador sparse conserva la integral de retardos y reproduce el factor
denso congelado con error 1,22437e−14. El control de generación tarda ~7 s;
facilita refinamientos sin modificar blancos físicos ni ajustar samples.
La evaluación hemisférica por bloques también coincide con la densa.

Duplicar sólo la tasa a 64 kHz con polar 48 **falla**: 2,11772% de memoria
y 12,64950% complejo a 6 kHz. Se conserva el rechazo. Una integral angular
independiente muestra que duplicar azimut no resuelve el error resistivo;
duplicar puntos polares lo baja de 12,575874% a 0,000554258% en ese control.

Con 64 kHz / polar 96 / azimut 96, 11445 estados conservan masa, límite y
certificado de pasividad. La memoria pasa en tres grillas: **0,51510–0,59509%**.
Un barrido de 61 frecuencias entre 1–6 kHz da máximo complejo **1,12750%**;
los seis puntos altos en las grillas 192×144 y 256×192 también pasan. Bajo
800 Hz pasan Z/R/X/Laplace y velocidad cargada. Pasan **24 tests**.

La banda hasta 6 kHz es la transferencia del aire para las **mismas 35 formas
P2 bajo 800 Hz**; todavía no se agregaron los modos estructurales altos.
El banco de tabla previo conserva su factor de 32 kHz y no se presenta como
una trayectoria nueva del candidato de 64 kHz. No se cambia audio/paquete.

Siguiente: contacto SI con este puerto, contacto frío y reiterado preservando
estados, balances y refinamientos con criterios previos. Después ampliar
modos de tabla, notas/velocidades y observación SI para escucha. Pérdidas,
ambas caras/borde, unísono, P4, graves y acción/pedal siguen abiertos.
Datos y fuentes en `C:\SHARED\Fisica-pendiente-2026-10-04\P6-resolucion-aire`.

### 2026-10-04 — P6 contacto compacto: implementación y control inicial

La resolución polar anterior quedó en **919a3c5**, por pedido del usuario.
[Construcción, control escalar y ensayo pendiente](RESULTADOS-P6-contacto-compacto.md).
Se implementa el contacto SI con el puerto de 11445 estados y relanzamiento
que conserva cuerda, tabla y aire. Pasan cinco tests nuevos y los once de
las dos cargas de aire: 16 en total. El primer C4/124 frío a 384 kHz cierra
el balance global a 2,22e−15 en los controles de almacenamiento cada 0,5 ms.

La medición angular agrupada conserva estados y fuerzas en el fixture;
su equivalencia en C4 y los refinamientos frío/reiterado del banco grande
se interrumpieron al limpiar el disco y se retoman a continuación. No se
declara selección audible ni se cambia el paquete. Informes y resúmenes
quedan sólo en el repositorio, con trayectorias NPZ únicas en `SHARED`.

### 2026-10-04 — P6 contacto compacto: refinamientos completos

La implementación y control inicial quedaron en **a526d50**, por pedido del
usuario. [Resultados completos](RESULTADOS-P6-contacto-compacto.md) y
`resumen-P6-contacto-compacto.json` quedan sólo en el repo. Se completan los
cuatro ensayos, frío y reiterado a 384/768 kHz; todos pasan los umbrales
previos. El observador agrupado conserva estados/dinámica bit a bit frente
al control escalar y cambia la radiación sólo por redondeo, 8,24e−16 RMS.

La velocidad RMS del puente cambia 0,011121% / 0,028135%; el mayor cambio de
energía/trabajo es 0,043529% / 0,005334%. Los estados vivos se conservan al
relanzar y el balance de las dos colisiones cierra a 5,14e−13 en la tasa fina.
Máximo error global de los cuatro ensayos: 5,79e−13. Se mantienen los controles
de almacenamiento cada 0,5 ms y los tests de identidad por paso del fixture.

Siguiente: ampliar la base P2 de 35 modos bajo 800 Hz a 59 bajo 1200 Hz como
primer escalón, recalculando su carga acústica y verificando los mismos
controles. El asset exportado permite hasta 256 modos/2965,08 Hz; los 6 kHz
de transferencia del aire no son modos mecánicos adicionales. Luego observación
SI, banco de notas/velocidades y escucha. Fábrica, pérdidas identificadas,
ambas caras/borde, unísono, P4, graves y acción/pedal siguen abiertos.

### 2026-10-04 — P6 carga ampliada a 59 modos bajo 1200 Hz

Los refinamientos del contacto compacto quedaron en **d35cb4a**, por pedido del
usuario. [Construcción y resultados](RESULTADOS-P6-tabla-1200.md). Se recalcula
el puerto completo para 59 formas P2, hasta 1193,51 Hz, con 19293 estados de
aire. El subespacio anterior del factor y de la masa estática coincide
exactamente; la relajación ampliada conserva residuos positivos sin clipping.

Pasan los 11 gates. Máximo Z/R/X bajo 1200 Hz: 0,764700%; complejo en el
barrido hasta 6 kHz: 0,989030%, o 1,321738% en seis puntos de la grilla fina.
Error de velocidad cargada: 0,234164%. La excitación central de C4 tiene
29 modos casi nodales; un ensayo distribuido activa los 59, incluidos los
24 nuevos. Su RMS de fuerza pasa en ambas grillas: 0,641665% / 1,089959%.
Balance de estados arbitrarios: 3,16e−16. Pasan 16 tests de aire/kernel.

Los 6 kHz siguen siendo transferencia del aire para estas formas; la tabla
no contiene modos mecánicos hasta esa frecuencia. Los contactos SI anteriores
siguen siendo de 35 modos. Siguiente: contacto frío/reiterado con los 59 modos,
balances y refinamientos; luego observación SI, banco de notas y escucha.
Paquete sin cambios. Informe y resumen únicos en el repo; sólo tres NPZ de
coeficientes/mediciones en `C:\SHARED\Fisica-pendiente-2026-10-04\P6-tabla-1200`.

### 2026-10-04 — P6 contacto compacto con 59 modos

La carga ampliada quedó en **1bfad0a**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-contacto-compacto-1200.md).
Se conectan los 59 modos/19293 estados al mismo contacto vertical C4/124.
Los cuatro casos frío/reiterado a 384/768 kHz pasan energía, refinamientos y
secuencia, con los umbrales anteriores; también pasa el prefijo del observador.
La velocidad RMS del puente cambia 0,015692% /
0,037503% al refinar; máximo de energía/trabajo 0,070808% / 0,005345%.
Balance global máximo 5,82e−13; secuencia fina acumulada 5,14e−13.

El prefijo de 64 pasos con memoria viva conserva estados y trabajos bit a
bit entre observadores; la radiación difiere 3,78e−15. Su alcance no se
presenta como una trayectoria escalar completa. Pasan 16 tests dirigidos.
Frente a 35 modos, a tasa fina, la velocidad del puente difiere 19,361732% /
16,732672%, y la energía final de tabla aumenta 2,477252% / 2,518698%.
No es un error temporal ni una mejora audible ya seleccionada.

Siguiente: observación SI de las dos bases con igual geometría y ganancia,
para medir cuánto del cambio local llega al micrófono. Después resolver
convergencia estructural y ampliar notas/intensidades antes de escucha.
Fábrica, pérdidas identificadas, ambas caras/borde, unísono, P4, graves y
acción/pedal siguen abiertos. Informes únicos en el repo; cuatro NPZ en
`C:\SHARED\Fisica-pendiente-2026-10-04\P6-contacto-compacto-1200`, sin WAV ni
copias adicionales del coeficiente.

### 2026-10-04 — P6 presión SI de las bases 35/59

El banco de contactos ampliado quedó en **42e0b3c**, por pedido del usuario.
[Resultados y límites](RESULTADOS-P6-observacion-1200.md). Se lee presión SI
con retardos completos e integral dentro de las mismas celdas de su carga.
Las dos cápsulas cercanas reutilizan la geometría P3; otras dos a altura 2 m
sirven de control. Ganancia 1, misma geometría entre bases, sin normalización.

Pasan ocho gates y 13 tests. Errores máximos de presión: captura 0,000286%,
tasa mecánica 0,313885%, resolución espacial 0,114618%. La fuente Hermite
conserva la unión de los contactos sin impulsos artificiales, error 6,67e−15.
Las transformadas son Pa·s de ventanas de 4/8 ms, no SPL ni notas sostenidas.

La diferencia global de presión del par cercano es 16,992% / 10,342% para
los dos golpes y 11,826% en la secuencia, con apenas +1,10% de norma global
en ésta. Las contribuciones de magnitud y fase son comparables. La diferencia
baja a 4,408% en mono; a altura 2 m es 13,185%. No es sólo ganancia ni una
medida perceptiva de anchura. El aporte principal viene de los modos añadidos.

Siguiente: 90 modos bajo 1500 Hz, con carga/contactos/observación recalculados,
para medir convergencia estructural antes de elegir escucha. Permanecen abiertos
fuente sostenida, banco de notas/intensidades, geometría/pérdidas identificadas,
ambas caras/borde, unísono, P4, graves y acción/pedal. Paquete sin cambios.
Sólo un NPZ derivado de 5,20 MB en `C:\SHARED\Fisica-pendiente-2026-10-04\P6-observacion-1200`;
informes y fuentes tienen su copia única en el repo.

### 2026-10-05 — P6: 90 modos y corrección de resolución acústica

La comparación de presión anterior quedó en **68a508a**, por pedido del usuario.
[Resultados y diagnóstico](RESULTADOS-P6-tabla-1500.md). Al ampliar a 90 modos,
la carga 128×96/64 kHz pasa nueve de once controles: falla el máximo complejo
alto, 2,243874% con límite 2%, y el refinamiento espacial. Se conserva el
ensayo rechazado. El diagnóstico independiente localiza cambios espaciales
de 1,369258% a 2416,667 Hz y una conversión temporal apreciable; doblar
cuadratura angular apenas afecta ese punto. No se cambian umbrales.

El candidato 192×144/128 kHz pasa los once controles, comparado con una
referencia 256×192 y ocho puntos espaciales, incluido el fallido. Máximo Z/R/X
bajo 1500 Hz: 0,328458%; máximo complejo alto: 0,571678%; contra grilla fina:
0,861880%. Cambio máximo de referencia espacial: 0,485578%. La fuerza distribuida
activa los 90 modos y da RMS 0,237616% / 0,455971%. FrozenC4 es el control
más exigente, 1,691295% / 1,692054%, dentro del 2%. Balance vivo: 2,14e−16.
Pasan 34 tests dirigidos. El puerto refinado tiene 58320 estados: sigue siendo
un ensayo offline, sin selección ni modificación del paquete.

La comparación de contactos/presión se posterga para usar **59 y 90 modos
con la misma resolución acústica validada**. El cambio de grilla/tasa afecta
también a los modos comunes; mezclarlo con el cambio de base impediría atribuir
la diferencia. Siguiente: construir/verificar el puerto de 59 a esa resolución,
repetir contacto frío/reiterado a 384/768 kHz en ambas bases y luego observación
SI. Siguen abiertos sostenido, banco de notas/intensidades, identificación,
ambas caras/borde, unísono, P4, graves, acción/pedal y coste de ejecución.
Informes únicos en el repo, datos únicos en las rutas SHARED del informe,
sin WAV ni copias de fuentes.

### 2026-10-05 — P6: carga de 59 a resolución común e inicializaciones

La corrección numérica de 90 modos quedó en **90c7cff**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-tabla-1200-refinada.md). Se seleccionan
las primeras 59 columnas/targets del archivo padre de 90 y se reconstruye el
completado pasivo en 59 coordenadas. No se duplica el factor ni se recortan
estados de la carga mayor. Los targets coinciden con la superficie independiente
y el subespacio de control coincide exactamente.

Pasan los once controles de carga con los umbrales previos. Máximo Z/R/X
bajo 1200 Hz: 0,191491%; complejo alto: 0,599796%; contra grilla fina:
0,590889%; cambio espacial de referencia: 0,276164%. Fuerza distribuida RMS:
0,161068% / 0,321370%; FrozenC4: 1,602188% / 1,603161%. Balance de estados
vivos máximo: 6,44e−16. El puerto tiene 38232 estados.

El banco de contacto acepta la grilla refinada y la selección del archivo
padre, valida su carga y rechaza comparar bases con resoluciones diferentes.
Los preflights de ambas bases pasan con energía inicial idéntica
0,04693371904404984 J; comprueban inicialización, sin avanzar tiempo ni escribir
trayectorias. Pasan 20 tests dirigidos, incluidos cuatro nuevos de selección.

Siguiente: cuatro contactos temporales de cada base a 384/768 kHz, balances,
continuidad y observador vivo; después presión SI con la superficie refinada.
El observador anterior aún requiere generalizar su base/grilla antes de usar
estos bancos. Paquete sin cambios; sólo dos NPZ nuevos de medición en
`C:\SHARED\Fisica-pendiente-2026-10-05\P6-tabla-1200-refinada`. Informes y
fuentes tienen única copia en el repo.

### 2026-10-05 — P6: banco de contactos refinados de 59 modos

La carga seleccionada y ambos preflights quedaron en **f124550**, por pedido
del usuario. [Resultados y alcance](RESULTADOS-P6-contacto-1200-refinado.md).
Se completan cuatro contactos C4/124 a 384/768 kHz con 38232 estados acústicos
y memoria viva entre golpes. Pasan cinco controles: energía, refinamientos
frío/reiterado, secuencia persistente y prefijo del observador.

Velocidad RMS del puente al refinar: 0,015692% / 0,037502%; máximos de
energía/trabajo: 0,076833% / 0,005345%, dentro del 0,2%. Balance global máximo
5,81e−13; presupuesto fino de la secuencia 5,18e−13. El prefijo vivo de 64
pasos conserva estados/trabajos idénticos, con radiación relativa 3,55e−15.
Las integraciones suman 18,1 minutos, además de inicialización/prefijo.

Sigue medir los contactos equivalentes de 90 y observar ambas bases en SI
con grilla común. Todavía no hay fuente sostenida ni audio de esta nueva
física: el plugin instalado mantiene el sonido anterior. La primera escucha
útil debe ser una nota sostenida controlada antes de MIDI/integración.
Informes únicos en el repo; cuatro NPZ únicos de unos 3,28 MB en
`C:\SHARED\Fisica-pendiente-2026-10-05\P6-contacto-1200-refinado`, sin copia
del factor ni WAV de ventanas cortadas.

### 2026-10-05 — P6: primera escucha física de una cuerda

El usuario pidió avanzar hacia una primera nota audible. Se adelanta una
escucha diagnóstica de C4/124 a la comparación estructural de 90, que sigue
pendiente. [Resultados y reproducción](RESULTADOS-P6-primera-nota.md).
Se continúa el contacto frío validado hasta 250 ms a 768 kHz, sin cambiar
potencial no lineal, modos, carga ni memoria. El acelerador de las mismas
ecuaciones pasa referencia viva y ataque completo; pasan siete tests nuevos.
La continuación tarda 578,187 s incluyendo guardar el NPZ. No hay nuevos
contactos ni retornos al tangente completo durante el sostenido.

Micrófonos sobre la misma superficie 192×144, dos cápsulas a altura 0,66 m,
retardos completos. Pasan retardo fraccionario, cuadratura, grilla y captura.
La diferencia de presión al reducir captura 128→64 kHz es 0,00002359%
en 60–6000 Hz. Primera escucha estéreo 48 kHz, ganancia común 0,25/Pa,
fade final de edición 8 ms; no simula soltar tecla ni tres cuerdas al unísono.
Audio único en `C:\SHARED\Fisica-pendiente-2026-10-05\P6-primera-nota\C4-124-Piloto-fisico-250ms.wav`.
Trayectoria y kernel únicos en esa carpeta; informes sólo en el repo.

No se selecciona un nuevo programa ni se cambia el paquete. Siguen abiertos
refinamiento temporal del sostenido, 59→90, integral angular independiente
del sostenido, identificación, unísono, apagador/acción/pedal y coste runtime.
La radiación inferida del almacenamiento en esta continuación no reemplaza
un balance cerrado por una segunda integral independiente. Ya hay una nota
para escuchar; la fidelidad y la mejora perceptiva todavía deben evaluarse.

### 2026-10-05 — P6: matemática del sostenido y coste medido

El usuario cuestionó extrapolar unos 40 minutos para un segundo de nota.
[Método, resultados y reproducción](RESULTADOS-P6-coast-exponencial.md).
Se implementa AAVF con fases cuadráticas exactas, integral no lineal
polinómica y composición simétrica con el puerto pasivo de aire. Se conservan
todos los modos, parámetros y 38232 estados acústicos; sólo aplica al
martillo separado. Pasan seis pruebas independientes y su repetición con
la integral polinómica más un control de equivalencia, 13 ejecuciones.

Desde el mismo ataque de 4 ms, se repiten los 246 ms restantes a
64/128/256 kHz. Presión 64 vs 256: 0,053813%; B/V: 0,017496% / 0,039735%.
128 vs 256: presión 0,013612%. Pasan refinamiento y pérdida inferida positiva.
64 kHz es candidato para esta nota/intervalo; dinámica medida 54,724 s.
Contra el midpoint anterior a 768 kHz la presión cambia 0,378119%.
Los 578,187 s anteriores incluían controles/guardado; los 54,724 s nuevos
excluyen esos costes y no se comparan como un speedup exacto.

Estimación revisada: 4–6 minutos para un segundo reutilizando el ataque.
Ese segundo sigue pendiente: medir 64/128 durante todo el sostenido antes
de elegir otra escucha. El paquete y el WAV anterior siguen iguales.
Informes únicos en el repo; trayectorias únicas en las dos carpetas
`P6-coast-exponencial-prefix` / `P6-coast-exponencial-250ms` de SHARED.
Siguen abiertos 59→90, identificación, unísono, acción/pedal y coste runtime.

### 2026-10-05 — P6: primera nota sostenida de un segundo

El usuario pidió renderizar la nota más larga antes de introducir más
física. [Resultados y reproducción](RESULTADOS-P6-nota-1s.md). Se continúan
las trayectorias de 250 ms a 64/128 kHz hasta un segundo, conservando todos
sus estados. Sólo se guardan los segmentos nuevos; no se duplica el factor
ni los prefijos. Las uniones de posición/velocidad son exactamente iguales.
Pasan tres pruebas de continuidad/transformada sin impulso de empalme.

Pasan los cuatro controles completos: presión 64 vs 128, 0,133475%; tabla
B/V, 0,043449% / 0,095684%; captura, 0,00002939%; pérdida inferida positiva.
No hay nuevos contactos ni tangentes completos. Dinámica adicional:
164,203 / 296,297 s para 750 ms; ambos ensayos y exportación tardan
555,516 s nuevos, incluyendo controles, sin recalcular ataque/prefijos.

WAV único: `C:\SHARED\Fisica-pendiente-2026-10-05\P6-nota-1s\C4-124-Piloto-fisico-1s.wav`.
Usa la trayectoria de 128 kHz, mismo kernel y ganancia 0,25/Pa; estéreo
48 kHz, pico 0,505248. Un segundo de fuente, 1,403479 s de archivo por
silencios/propagación, fade de edición 8 ms. Sigue siendo una cuerda del
piloto SI. Paquete sin cambios. Informes únicos en el repo, dos extensiones
únicas y audio en SHARED. Siguiente: escucha, comparación 59→90 y unísono;
siguen pendientes identificación, radiación angular larga, acción/pedal y runtime.

### 2026-10-05 — P6: primer contexto musical del piloto

El usuario pidió una melodía para poder juzgar el modelo.
[Banco, controles y alcance](RESULTADOS-P6-frase.md). Se simulan E4/G4/C5
con sus propios parámetros de cuerda/contacto y proyección de puente. C4
reutiliza su WAV de un segundo. El exportador ignorado se amplía para notas
21–108; C4 conserva exactamente sus nueve parámetros primitivos y pasan
las dos huellas de sonido de fábrica/pedal. No se modifica el motor de fábrica.

Las tres notas pasan contacto 384/768 kHz, energía independiente (máximo
5,973e−12), sostenido 64/128 kHz, captura y pérdida inferida positiva.
Presión al refinar: E4 0,122554%, G4 0,223585%, C5 0,150925%. Se usa 128 kHz
para la salida con la misma ganancia 0,25/Pa, sin ajustar niveles por nota.
Los tres procesos independientes completan el banco en unos 17,4 minutos,
incluyendo dos contactos y dos sostenidos por altura nueva.

Primera frase: C4–E4–G4–C5–G4–E4–C4, notas separadas 1,2 s, fuente de 1 s
por nota, archivo 8,603479 s. WAV único en
`C:\SHARED\Fisica-pendiente-2026-10-05\P6-frase\Arpegio-C4-E4-G4-C5-Piloto-fisico.wav`.
Cada altura es calculada físicamente. El ensamblado usa notas frías
separadas, una cuerda por altura y el fade de edición existente; no acredita
tabla compartida, acción/pedal ni reiteraciones vivas. Informes únicos en
el repo, datos únicos en P6-frase y sin copia del C4, factor o kernel.
Siguiente: escucha, 59→90 y unísono antes de ampliar a piezas polifónicas.

### 2026-10-05 — P6: banda alta y simetría del piloto

Por pedido del usuario se eliminan los audios recientes y paquetes obsoletos;
los datos nuevos se guardan en `local/`, con informes únicos en el repo.
[Resultados, controles y reproducción](RESULTADOS-P6-banda-alta.md).
La base completa 76×56 falla presión alta (4,7128% al refinar) y ningún
corte hasta 18 kHz conserva la admitancia al 1%. Distribuir la misma masa/EI
de vigas sobre su ancho tampoco resuelve la convergencia; hipótesis no seleccionada.

Se implementa la simetría exacta respecto del centro de esta geometría para
fuerzas verticales y estado antisimétrico cero. El bloque 76×56 reproduce el
sistema completo con máximo error integrado 2,6521e−10. Permite refinar a
132×112 con 7392 coordenadas activas conservando todas sus frecuencias.
Último refinamiento: presión alta 0,43455%, admitancia alta 0,09488%; celdas
192×144 → 256×192: presión alta 0,46502%. Pasan los tres controles espaciales
y ocho pruebas nuevas. No se modifica el paquete ni se produce otro WAV.

Siguiente: proyección pasiva que conserve flexibilidad modal alta y nueva
carga acústica recíproca de la misma superficie; después contacto, sostenido,
liberación y escucha. La simetría no acredita fuerzas laterales ni una tabla
real asimétrica. Siguen pendientes identificación, unísono, P4, ambas caras,
acción/pedal y coste runtime. La medición seca no establece mejora de timbre.

### 2026-10-05 — P6: proyección pasiva y controles espaciales reservados

El avance anterior queda en **c6f3b1a8** por pedido del usuario; pasan 35
pruebas dirigidas. [Método, resultados y reproducción](RESULTADOS-P6-proyeccion-pasiva.md).
Se conservan 296 modos hasta 6 kHz y se enriquece la cola mediante estados
forzados reales/imaginarios, incluyendo flexibilidad estática. Se proyectan
masa, rigidez, pérdidas completas, puertos y radiación con el mismo mapa.

Cuatro/18 puntos de construcción fallan en notas reservadas: 13,717%/2,649%.
35 posiciones virtuales del puente, distintas de los siete controles MIDI,
dan 606 coordenadas. Pasan transferencias en dos grillas y controles físicos:
máximo por nota/banda 0,138867%, masa 1,50e−15, flexibilidad estática 5,33e−16.
Velocidad modal completa ponderada por masa: error 0,195859%, pasa el 1%.
Norma elástica: 1,364417%, diagnóstico separado; no acredita balance temporal.
Pasan cuatro pruebas nuevas de congruencia, interpolación, potencia y rechazo.

Siguiente: carga recíproca de estas formas, controles del sistema cargado y
extensión del integrador para pérdidas de cola no diagonales. Datos nuevos
únicos en `local/`; sin WAV nuevo ni actualización del paquete. Sigue pendiente
la escucha, y la precisión seca no certifica fidelidad musical ni las 88 teclas.

### 2026-10-05 — P6: carga directa del aire y control espacial pendiente

La proyección pasiva queda en **379cc51**, por pedido del usuario.
[Método y mediciones](RESULTADOS-P6-carga-directa.md). Se implementa la
reacción Rayleigh continua sin matrices densas de carga ni factores FIR
grandes; solver en bloque con residual original. Pasan cinco pruebas nuevas
contra solve denso, solver independiente, mapas transpuestos, potencia y presión.

En 83 controles entre 60 y 6000 Hz, proyección frente al padre cargado:
máximo por puerto de presión 0,618522%, admitancia 0,255239%. Pasa balance
armónico (1,615e−10), reciprocidad, puertos disipativos y cuadraturas. La
integral angular independiente coincide. **No se aprueba la corrida 192×144**:
refinar a 256×192 cambia presión a 6 kHz un 1,018371%, por encima del 1%.
Se conserva el control fallido; siguiente: base 256×192 y contraste 320×240.
Sin nuevo WAV ni paquete, datos nuevos pequeños sólo en `local/`.

### 2026-10-05 — P6: refinamiento cargado aprobado y aritmética equivalente

El control original queda reproducible en **0a99543**. La
[repetición refinada](RESULTADOS-P6-carga-directa.md#carga-con-superficie-refinada)
usa superficie 256×192 y referencia 320×240. Pasan los nueve controles con
los mismos límites: presión proyectada máxima 0,622059%, admitancia 0,255148%,
cambio espacial máximo 0,479522%. Balance de potencia 1,902e−10; no es aún
un balance de estados acústicos en el tiempo. Los 83 puntos son discretos.

Se evita la copia compleja de la matriz Ritz mediante productos separados
real/imaginario. Dos pruebas adicionales y un preflight idéntico al anterior
dan diferencia máxima 2,615e−14. Ese preflight baja de 33,375 a 17,297 s;
la corrida refinada completa tarda 542,828 s con pico de 2,10 GiB. No es una
medición de audio en tiempo real. Siguiente: realización causal pasiva y
contacto/coast/diagnósticos con la matriz completa de pérdidas proyectadas.
El paquete instalado sigue en 0.172.0; no se dejan nuevos archivos en SHARED.

### 2026-10-05 — P6: soporte temporal con pérdidas proyectadas completas

La carga refinada queda en **40e3547**, por pedido del usuario.
[Método y mediciones](RESULTADOS-P6-perdidas-proyectadas.md). El adaptador
usa masa/rigidez reales y conserva la matriz completa de pérdidas tras el
cambio modal. Fuerza y calor de contacto, ambos controles de aire y componentes
usan esa matriz; coast y apagador mantienen sus términos mutuos.

Pasan cinco pruebas nuevas contra coordenadas físicas/ODE y balances vivos,
y tres regresiones de soporte/contacto anteriores. Con la tabla real de 606,
en 83 puntos y once puertos, el cambio de coordenadas da error máximo de
estado 1,068e−11 y de puertos 2,251e−12. Contacto inicial completo de 0,125 ms:
balance de energía 5,843e−14, residual de fuerza 9,020e−15 N. No es una nota
completa ni valida el timbre. La carga causal nueva sigue pendiente; deberá
usar el mismo cambio de base para fuerzas y micrófonos. No se duplican datos
ni se genera otro WAV/paquete.

### 2026-10-05 — P6: onda causal por bloques y rechazo temprano de masa

La integración de pérdidas queda en **7257bc4**, por pedido del usuario.
[Método y mediciones](RESULTADOS-P6-aire-bloques.md). Se construye el factor
causal por bloques sin descartar direcciones/taps/modos; las correlaciones
quedan en memoria y el certificado de energía/radiación regenera esos bloques.
Tres pruebas coinciden con el factor completo y sus estados vivos. Otras tres
verifican la identidad de masa por factor acumulado, evitando reservar Gram
y correlaciones para ensayar un completado imposible.

Los preflights de las 606 coordenadas con cuadraturas 16×32 y 32×64 conservan
pasividad, pero **se rechazan**: el residual de masa es indefinido y la presión
de la onda incompleta no cumple precisión. Refinar reduce el fallo; no se
recortan autovalores ni se ajusta ganancia. Pico observado ~6,1 GiB sin factor
guardado en disco. El control acumulado explora el siguiente refinamiento
antes de construir otra correlación completa. No hay WAV ni paquete nuevo.

El control acumulado reproduce el residual de 32×64 a 1,938e−14 y alcanza
64×128 con pico de 0,857 GiB, evitando ~5,72 GB de arreglos. Ese residual
también es negativo (mínimo −0,0016222293): completado rechazado antes de
construir correlaciones. Siguiente: 96×256 y contraste de tasa, separando
cuadratura angular de aproximación temporal antes de volver al contacto.

### 2026-10-06 — P6: masa residual positiva tras refinamiento angular

La construcción por bloques queda en **bee4f9c**, por pedido del usuario.
[Método y mediciones](RESULTADOS-P6-integracion-angular.md). La simetría en y
permite agrupar contribuciones angulares equivalentes con peso doble. Dos
pruebas nuevas conservan correlaciones/fuerza/energía; ocho pruebas pasan.
El control real 32×64 se reproduce a 1,983e−14 y tarda 34,860 s frente a
68,516 s en el cálculo acumulado anterior.

Con la misma superficie 256×192, la cuadratura 96×256 da residual de masa
**positivo**: mínimo 1,283e−5 a 128 kHz y 2,793e−5 a 64 kHz. El rechazo
anterior dependía decisivamente de la integración angular; la tasa temporal
modifica la masa restante. Pico de la corrida: 0,858 GiB, sin Gram ni
correlaciones guardados. La condición de masa no valida aún la carga completa.
Siguiente: Gram espectral real de las parejas, completado y controles de
impedancia/transitorios con 96×256/128 kHz. Sin WAV ni paquete nuevo.

### 2026-10-06 — P6: Gram real y carga completa rechazada en alta frecuencia

El refinamiento angular queda en **d2ab0c6**, por pedido del usuario.
[Método y mediciones](RESULTADOS-P6-Gram-real.md). El Gram de cada pareja
invertida es real y simétrico; se calcula un triángulo con dos productos
reales. Dos pruebas nuevas conservan correlaciones/masa/impedancia y fuerza,
energía y pérdida de estados activos; pasan diez pruebas dirigidas en total.

Con las 606 coordenadas y cuadratura 96×256/128 kHz, el completado finito y
la pasividad pasan. Los controles de 60 y 1881,960712 Hz pasan precisión;
el de 6 kHz se rechaza con los mismos límites: Z 10,012998%, R 8,618405%,
X 12,349082%, presión C4 1,058836%. Admitancia máxima 0,231594% sí pasa.
Tiempo 666,281 s; pico 4,294 GiB. No se guarda otro factor ni se genera WAV
o paquete. Se separan ahora integral angular y efectos temporales con un
sondeo de resistencia que no sustituye las puertas del candidato completo.

El sondeo de 30 direcciones contiene las once respuestas continuas a 6 kHz
(error de representación 1,614e−15) y ocho direcciones aleatorias. Su integral
angular exacta coincide con Rayleigh a 5,758e−15. El error temporal domina:
onda vs Rayleigh 9,753% a 128 kHz y 2,609% a 256 kHz; sólo el warp da 8,184%
y 2,160%. Dos pruebas independientes adicionales pasan, doce en total.
Esto localiza el defecto en el banco, sin aprobar toda la carga. Siguiente:
retardo de mayor orden/refinamiento temporal, certificado vivo y condiciones
de límites antes de repetir la carga completa. Sin WAV ni cambios de timbre.

### 2026-10-06 — P6: Padé de segundo orden y derivada cuarta

El Gram real y el diagnóstico temporal quedan en **0c48a73**, por pedido del
usuario. [Método, certificado y mediciones](RESULTADOS-P6-retardo-orden2.md).
Cinco pruebas nuevas cubren transferencia, estabilidad, energía de estados
arbitrarios, ODE independiente, Schur y tabla acoplada. Se mantiene un Gram
real/taps iguales más dos, con dos coordenadas de estado por sección.

En el sondeo congelado de 30, la resistencia de onda a 6 kHz pasa de 9,753%
a **0,773%** con las dos correcciones; cada corrección aislada aún falla.
El completado del sondeo tiene masa/límite realizados al nivel de redondeo;
Z/R/X máximos 0,706%/0,650%/0,926%, y Laplace máximo 0,603%. Sus cuatro gates
pasan, sin cambio de tasa/celdas/cuadratura/geometría ni ajuste de muestras.

La puerta global reproduce el legado a 1,983e−14, pero **rechaza** la masa de
la derivada nueva con 96×256/128 kHz: mínimo −2,75934e−6, máximo 0,0007889262.
No se reparan extremos propios ni se acopla ese completado al instrumento.
509,906 s con control y pico 0,862 GiB. Siguiente: refinamiento angular
separado, empezando por 128×384. Luego transferencia completa y adaptación
de FastBoundedContact/ExponentialCoast a dos estados. Informes sólo en el
repo; sin WAV ni paquete nuevo.

### 2026-10-06 — P6: condición global e integración de dos estados

La etapa de retardo y el rechazo a 96×256 quedan en **e11fe6d**.
[Condición y pruebas nuevas](RESULTADOS-P6-carga-orden2-completa.md).
Refinando sólo la cuadratura a 128×384, la masa pasa: mínimo +9,976987e−6,
máximo 0,0006321346. El control anterior se reproduce a 1,983e−14. Corrida
1006,234 s, pico 0,864 GiB; sin recorte ni ajuste de ganancia.

Se adapta la recurrencia rápida de contacto/coast a las dos coordenadas
persistentes. Tres pruebas nuevas contrastan una resolución matricial
independiente, contacto con estado vivo/relanzamiento y sostenido contra
DOP853; pasan también las nueve regresiones anteriores de Cayley. Son
bancos pequeños, no aprobación temporal de las 606 coordenadas ni timbre.
El auditor completo exige masa positiva antes de construir correlaciones
y mide tres puntos antes de recorrer la grilla congelada de 83 frecuencias.

Pasan los nueve gates de las 83 frecuencias y tres puntos Laplace. Z/R/X
máximos 0,667445%/0,494951%/0,921198% a 6 kHz; movilidad 0,020731% y presión
0,123714%. Masa por correlaciones reproduce el control acumulado a 7,615e−14;
límites realizados al nivel de redondeo. Tiempo 1654,188 s y pico 4,338 GiB.
La puerta general sigue falsa: faltan contacto/repetición y release completos,
coste/observer temporal e identificación antes de escuchar o integrar al plugin.
Sin WAV ni paquete nuevo; informes únicos en el repo.

### 2026-10-06 — P6: prefijos vivos de 606 y captura compacta

La carga completa queda en **6253412**. Fuentes del primer auditor en
**c47e746**; corrección de escritura, controles de fuerza y primera recurrencia
de reconstrucción compacta en **d850ef7**. [Método y medidas](RESULTADOS-P6-contacto-orden2-prefijos.md).
Se completan seis prefijos C4 supuestos, frío/continuado, a tres tasas.
125+125 microsegundos; no se relanza el martillo ni se resetea memoria.

La primera ejecución pierde las medidas finales por un booleano NumPy
no serializable. Se conserva su construcción/contactos incompletos y se
repite reutilizando una única R autenticada de 1,91 GB. Pasan balance
independiente (máximo total 3,251251e−13), reacción y refinamiento de
trayectoria/tabla/impulso y términos energéticos (< 0,014268% en el par fino).
El máximo AVF frío falla 0,635430% porque promedia intervalos distintos;
la fuerza constitutiva en tiempos comunes da 0,000242279%, con razón de
refinamiento ~0,2. Se conserva el fallo original y pasan doce controles
enlazados del prefijo. La radiación de onda es casi nula a esta duración;
falta precisión de la cola no nula. Repetición 1844,820 s, pico 6,589 GiB.

Cinco pruebas de certificado/caché/reconstrucción pasan. La captura real
desde puertos reconstruye exactamente los seis extremos acústicos: 3,26 MB
de historias frente a 2,12 GB, sin descartar modos. Pico propio 2,699 GiB;
146,390 s junto al otro auditor. No es un benchmark aislado ni validación
del certificado compacto completo. Siguiente: certificado y coste compactos
con golpe completo/radiación no nula, repetición, release y presión temporal.
La puerta global sigue falsa; no hay nuevo audio ni paquete en SHARED.

### 2026-10-06 — P6: observación del campo con residuo acotado

[Método, cotas y medidas](RESULTADOS-P6-observador-acotado.md). Se acelera
la observación independiente del campo mediante QR/SVD de memorias y
puertos; las fuerzas y trayectorias conservan los 606 modos. Rango elegido:
102. Las normas de los residuos completos se convierten en intervalos de
almacenamiento/flujo, sin deducir radiación del balance. Tres pruebas pasan
contra factores completos, incluida memoria viva y flujo no nulo.

En la repetición de los seis prefijos, los 66 diagnósticos mecánicos
comparados no cambian y las 18 energías anteriores quedan dentro de las
cotas. Pasan 12 controles; máxima cota relativa con piso 1e−14 J:
2,923294e−7. Balance total/aire con extremos adversos:
3,258294e−13/5,490971e−16. Las salvaguardas de redondeo no son una prueba
formal de aritmética de intervalos. Tiempo 1516,607 s, certificado 1390,795 s,
pico 3,700088 GiB. La comparación de coste anterior no fue aislada;
no valida tiempo real. Una sola R, informes únicos en el repo, sin WAV/paquete.

Siguiente: duración suficiente para separación y flujo apreciable, con
refinamiento/cotas medidos allí. Repetición natural, release, presión temporal
y timbre siguen pendientes; la puerta global permanece falsa.

### 2026-10-06 — P6: separación y flujo no nulo, con refinamiento frío dirigido

[Método, ejecución interrumpida, medidas y siguientes pasos](RESULTADOS-P6-contacto-milisegundos.md).
Se amplía C4 supuesto a 1,5+1,5 ms con los 606 modos y aire vivo. La
separación aparece en las tres continuaciones: intervalo más fino
1,634115–1,635417 ms. Se preserva una primera ejecución incompleta por coste
de repetir 4032 updates en 384 bloques. **ea06e59** reutiliza una vez los
inputs de observación y aplica el refinamiento a los extremos adversos
de sus intervalos. Seis pruebas dirigidas pasan; física sin cambios.

El ensayo completo de esta duración pasa 11 controles: máximo balance
total/aire con cotas 2,414908e−11/1,864494e−11. Flujo con límite inferior
1,318446e−12 J frío y 3,533789e−8 J continuado; el observador elige rango 241,
sin reducir fuerzas/dinámica. Tiempo 2428,924 s, pico 8,678902 GiB. El flujo
frío falla el refinamiento 0,264677% frente a 0,2%; continuación pasa 0,034568%.

**09f151d** dirige otro banco sólo al frío, a 384/768/1536 kHz: pasan 11
controles y sus siete términos energéticos. Cambio acotado del flujo:
0,067444%. Los contactos compartidos reproducen exactamente sus diagnósticos;
las energías observadas con rango 241/189 se contienen mutuamente en sus
intervalos. Tiempo 2187,679 s, pico 7,400826 GiB. El fallo original permanece.
No se mezcla un nuevo extremo frío con la continuación anterior para afirmar
un recorrido uniforme a 1536 kHz. Una R, informes únicos, sin WAV/paquete.

Siguiente: coste de propagación después de separar el martillo y presión
temporal, después repetición natural/release y banco amplio. Las cotas de
proyección tienen salvaguardas numéricas; no son una prueba formal de
redondeo por intervalos. La puerta general y la aprobación del timbre siguen falsas.

### 2026-10-06 — P6: contracción completa más rápida y coast separado

[Método y mediciones](RESULTADOS-P6-propagacion-coste.md). **f8c614a** conserva
la contracción anterior como control y usa productos por sección sobre la
misma R de sólo lectura; no agrega otra matriz de 1,9 GB. Diez pruebas pasan.
El contacto continuo de 3 ms a 768 kHz reproduce el extremo congelado;
dinámica 288,554873→243,634189 s, reducción 15,5675%. Ambos kernels separan
el martillo en el mismo intervalo y difieren < 1,057e−14 en estados vivos.

Otros 0,5 ms de coast desde el mismo extremo, a 64/128/256 kHz, pasan nueve
controles del preflight. Diferencias físicas de tabla frente a 256 kHz:
0,00209233%/0,000672160%; razón 0,3212495. Coste de dinámica baja 15–17%.
Tiempo total 622,373 s, pico 2,571152 GiB. No se selecciona aún una tasa para
una nota: falta balance angular independiente y presión de la trayectoria.

**27d0705** conserva todos los pesos de retardo con signo en COO/CSR;
cinco pruebas de micrófonos pasan. En la tabla de 49.152 celdas, preparación
3,916595→0,597817 s (6,55 veces), diferencia de kernel 8,613e−16 y error
máximo contra Green continuo 0,0114133% en DC–6 kHz, bajo el límite 0,02%.
Se conserva una medición inicial con presupuesto de scratch mal contado;
**f9e7817** corrige el aplanado de las celdas y repite el control completo.
Pasan cuatro controles del operador. No se genera audio, paquete ni copias
en SHARED; la puerta global y la aprobación del timbre siguen pendientes.

### 2026-10-07 — P6: balance angular independiente y presión de la trayectoria

[Método, resultados y fallos conservados](RESULTADOS-P6-coast-campo.md).
**e6c3a93** captura los tres coast desde el mismo extremo de 3 ms, conserva
606 modos y memoria viva, y observa el factor angular completo con residuo
acotado. Trece pruebas pasan. Base de observación 351, replay/norma 0;
inputs 408.844.800 bytes en RAM. Máximos de balance total/aire con extremos
adversos: 4,583e−11/4,556e−11. Flujo fino 9,623702e−8 J, límite inferior
positivo; no se infiere del trabajo. Refinamiento fino de seis términos:
máximo 0,0496604%, bajo 0,2%. Presión espectral/temporal 0,00121814%/
0,00121680%, captura fina 0,00000111379%; pasan el 0,1% fijado. Kernel en
101 sondas DC–6 kHz cambia < 0,000716863% contra Green continuo.

El ensayo original pasa 12/13 controles: razón de tabla interpolada
0,371841 falla < 0,35. **8558e69/d9cc07c** mide separadamente los tiempos
físicos nativos comunes. Se conserva una tentativa incompleta por guardado
de booleanos NumPy. La repetición reproduce exactamente dinámica, presión
y el observable que falló; razón nativa 0,321249494, mismos límites.
Pasan cuatro controles de alineación y 16 vinculados; el fallo spline queda
en el padre, sin afirmar orden uniforme de la extensión interpolada.

Campo/presión: 1419,592 s, certificado 1107,991 s, pico 4,308224 GiB.
Alineación completa: 285,662 s, pico 2,847469 GiB. Faltan acelerar el
certificado, ampliar cuerda (3 transversales/dirección y 6 axiales aquí),
duración y repetición/release antes de una escucha. No se cambia fábrica,
paquete ni se dejan audios o informes en SHARED. Puerta general falsa.

### 2026-10-07 — P6: alternativa lineal de proyección del campo

[Operación, controles y coste](RESULTADOS-P6-proyeccion-celdas.md).
**bc6a362** proyecta las formas de celda antes de sus histogramas mediante
`L(S) Q = L(S Q)`. Conserva el factor completo para comprobar R[0] y mide
también la norma proyectada; geometría, ángulos, derivada y taps sin cambios.
Ocho pruebas del observador pasan, incluidas memoria viva, modos omitidos,
variantes de derivada/reflexión y detección de un factor proyectado alterado.
El ensayo completo pasa 17 controles. Dinámica/presión se reproducen con
diferencia 0; máxima diferencia de campo 2,750478e−16, mismas cotas y
refinamiento. Sin embargo, el certificado tarda 1488,233 s frente a
1107,991 s históricos: 34,3182% más. En las tres sondas del mismo proceso,
reconstruir el factor espacial proyectado cuesta 2,80–4,88 veces el producto
denso que reemplaza. Se conserva la ruta anterior por defecto; la nueva
queda como control opcional. Tiempo total 1798,766 s, pico 4,655491 GiB.
Una R, informes sólo en repo, sin WAV/paquete ni cambios de timbre.
Siguiente: ampliar parciales de cuerda con convergencia espectral antes de
extender una escucha; no adoptar la alternativa más lenta para el banco actual.

### 2026-10-07 — P6: resolución transversal/axial de cuerda con tabla completa

[Método, umbrales y mediciones](RESULTADOS-P6-resolucion-cuerda.md).
**ce94f3f** prepara la comparación de 3/6/12/24/48 modos por dirección,
con 2N axiales y el mismo fieltro, puente, tabla y R. Compara presión temprana
por bandas y temporal en 0–3,5 ms; usa coordenadas físicas comunes de tabla.
Separa controles de paso de contacto, cierre axial 2N/4N y propagación desde
el mismo extremo vivo. Cinco pruebas pasan contra cuadratura independiente,
incluido el puente móvil, y trabajo mecánico/captura del integrador de referencia.
La observación no deduce radiación del balance mecánico. Ventana corta,
una cuerda supuesta; sin aprobación del timbre, WAV, fábrica ni paquetes.

Ensayo terminado: **11/13 controles**, 2426,356 s, pico 3,153 GiB. El banco
de tres modos sólo llega a 785,53 Hz de frecuencia transversal libre; al
ampliar a 48 la fuerza máxima baja 25,3525→16,1850 N con el mismo fieltro.
La colisión estaba limitada por la base, no sólo por la observación de tabla.
Fallan presión compleja en 1980–6000 Hz para 24→48 (1,506794%, límite 1%)
y contacto 768→1536 kHz (0,101373%, límite 0,1%). Se conservan ambos fallos.
Reproducción anterior exacta; pasan balances, impulso, captura, control axial
y coast. Siguiente: matriz dirigida 48/96 modos y 1536/3072 kHz; comprobar
banda de fuente/carga y presupuesto del certificado angular antes de ampliar
ventana. No se adopta todavía 48 modos ni se cambia el plugin instalado.

### 2026-10-07 — P6: matriz dirigida y observador finito con extremos vivos

[Diseño y controles](RESULTADOS-P6-resolucion-dirigida.md). **27848bc**
separa resolución transversal (48/96) y temporal de contacto (1536/3072 kHz)
con idéntico coast a 256 kHz y todos los parámetros y límites anteriores.
Siete pruebas pasan, incluida cuadratura independiente hasta 96/384 y
detección de una banda fallida en cualquiera de las cuatro parejas.
Reconstruye el banco anterior antes de comparar. Ensayo completo: **14/14**,
3082,568 s (51,38 min), pico 3,287 GiB. Reproducción exacta del banco padre;
máximo espacial de presión alta 0,038670% (<1%), temporal 0,025344% (<0,1%).
Balance mecánico 3,19994e−12; impulso 5,16455e−15; refinamiento de trabajos
máximo 0,00123338% (<0,2%). Fallos anteriores conservados. Referencia
96/192 a 3072 kHz sólo para esta ventana temprana, sin aprobación global.
Siguiente: banda relevante, campo y coast del banco refinado, luego duración.

[Identidad finita y límites del prototipo](NOTA-P6-observador-ventana-finita.md).
**7baa4d1** calcula entradas centradas de cada sección Padé mediante DFT
de ventana finita, conservando memoria inicial y final. Tres pruebas pasan:
replay escalar y Parseval, padding/paridad/bloques, 650 taps con memoria fría
y viva. No modifica dinámica ni se adopta como certificado; todavía falta
replay enlazado, almacenamiento independiente y coste con 606 modos/campo
completo. Puede evitar historia de 27 GiB, pero no se promete aceleración.
Se registra además la posible eliminación causal del subsistema lineal de
tabla/aire para contactos repetidos del mismo operador. Debe conservar la
inercia compartida y reacción completa; todavía no está implementada ni medida.

### 2026-10-07 — P6: certificado angular desde la cuerda refinada

[Resultados, fallo y reproducción](RESULTADOS-P6-coast-refinado.md).
**0a6c1f7** añade el auditor de coast 96/192 y sondas espectrales sobre
memorias vivas. Once pruebas pasan, tres nuevas y ocho controles existentes.
El ensayo termina **15/16** en 2231,768 s (37,20 min), pico 4,853 GiB,
reproduciendo exactamente contacto, coast y presión del padre.
El certificado angular completo pasa replay, almacenamiento, flujo positivo,
pérdidas y balance acotado máximo 4,32661e−11. Tabla nativa: razón 0,234935
frente a 0,35; presión fina máxima hasta 6 kHz: 0,00311481% frente a 0,1%.

Falla únicamente el refinamiento de radiación: 128→256 cambia 0,274024%
con cotas frente a 0,2%; no se cambia el límite ni se aprueba el coast.
Las 12 sondas en dos bloques pasan campo complejo/flujo a 2,25109e−13 /
1,47126e−13. El kernel espectral cuesta 10,26–13,80 veces el producto
temporal con inputs guardados; preparación/replay se mide aparte y se puede
compartir. No se adopta como observador por defecto ni se promete ahorro
global. Una R existente, historias en RAM, informe sólo en repo.
Siguiente: 512 kHz desde el mismo seed antes de duración; después banda
superior de fuente/carga y balance angular del contacto completo. No hay
WAV ni paquete nuevo; aprobación global falsa, faltan release/unísono/escucha.

### 2026-10-07 — P6: refinamiento a 512 y estado reutilizable

[Ensayo, incidencia y resultados finales](RESULTADOS-P6-coast-512.md).
**90cfd00** agrega 128/256/512 kHz con todos los límites anteriores y
una cápsula única del contacto completo. Cinco pruebas pasan. Al detectar
un booleano NumPy no serializable, se interrumpe el primer certificado a
336/384 bloques; sus resultados angulares no se dan por medidos. Su
checkpoint físico/presión queda en repo como procedencia del estado.
**9e80a33** corrige el escritor y agrega autenticación del productor
histórico desde Git para continuar desde la misma cápsula; tres regresiones
pasan, incluida serialización de intervalos reales del observador.

El nuevo certificado completo termina **15/15** en **1251,862 s (20,86 min)**,
1152,779 s de certificado, pico 4,914 GiB. Reproducción de estado,
continuaciones y presión del padre: error 0; seis intervalos angulares del
padre solapan, replay 0, balance adverso acotado máximo 4,32294e−11.
Flujo 256→512: **0,0690548%** con cotas (<0,2%); presión alta fina
**0,000761170%** (<0,1%). 128→512 conserva fallo de radiación 0,342584%.
256 pasa sólo para este coast de 0,5 ms, sin aprobación global ni de timbre.

Una cápsula completa de **6,31 MB** evita **16,99 min** de contacto;
no guarda R ni historias, no acelera el campo angular y no se duplica en
SHARED. Fuente/carga/estado originales autenticados y verificados contra
el operador actual. Sigue banda superior de fuente/carga, balance del
contacto completo y luego duración/release. Sin audio ni paquete nuevo.

### 2026-10-08 — P6: banda superior de fuente y carga, con controles de ventana

[Diseño, fallos y resultados](RESULTADOS-P6-banda-superior.md). **61931ea**
agrega observación rectangular/Hann de V/A sin alterar dinámica ni derivar
una posición enventanada; cuatro pruebas pasan. El primer diagnóstico
256/512 termina **6/9** en 130,549 s: fallan fuente alta, integración en
frecuencia y captura de presión. **44d6c1b** conserva esos fallos y agrega
512/1024/2048 y grillas 125/62,5/31,25 Hz con los mismos límites.

Refinamiento completo **9/9** en **407,429 s**, pico **3,323 GiB**;
reproducción física/espectral/presión exacta, fuente fina máxima 0,0215987%,
presión 0,0455909%, integración 0,0776924% (límites 0,1%). Global falso.
Entre 6–24 kHz, fracción de norma de A rectangular/Hann **2,91969 /
0,0360188%**: fuerte sensibilidad a extremos, sin afirmar ausencia de modos
altos. Sondas de carga con fuente fina: error de acción contra Rayleigh
en la malla vigente **2,06477% a 10 kHz**, **21,2811% a 16 kHz**,
**69,4099% a 24 kHz**. Contra la malla fina, 6 kHz alcanza 2,02431%.
Pasar controles de diagnóstico no aprueba esas sondas ni amplía el operador
certificado de 60–6000 Hz. No está medido el efecto audible ni su causa interna.

Siguiente: fuente/presión del contacto completo y presupuesto del ledger
angular antes de elegir banda o duración. Misma R/cápsula; historias en RAM,
informes sólo en repo, sin WAV, SHARED o modificación de fábrica/paquete.

### 2026-10-08 — P6: fuente/presión del ataque completo y cuadratura analítica

[Diseño y resultados](RESULTADOS-P6-contacto-banda-superior.md). **b59d947**
agrega momentos uniformes por bloques y reconstrucción 0–3 ms de C4,
96/192 modos, 1536/3072 kHz; cinco pruebas pasan. Ensayo completo **9/13**,
**1711,709 s (28,53 min)**, pico **4,895 GiB**. Reproducción física exacta
de ambas tasas y de los cuatro arrays de la cápsula al terminar a 3072.
El espectro de V/A, 772 frecuencias/dos ventanas, cuesta **2,280/4,288 s**;
la dinámica sigue costando **521,827/1001,169 s**. Sin nuevas historias o R.

Fallan oracle de V Hann a 24 kHz (máximo 4,96601e−10, límite 1e−10),
convergencia temporal de colas Hann (A alta 2,23017%, límite 0,1%),
integración por trapecios (0,484016%, límite 0,1%) y celdas de carga
(9,50396%, límite 2%). **a0e5335** agrega identidad Simpson sobre grillas
altas uniformes, tres pruebas y derivación del puerto con inercia completa.
Control posterior autenticado: integración **0,0184258%**, mismos límites,
sin contacto nuevo; quedan **10/13**, conservando todos los demás fallos.

Presión global 1536→3072 cambia **0,00245269%** en Hann; banda 6–24 aporta
**0,103420%** rectangular o **0,000365104%** Hann a la norma total. No prueba
irrelevancia audible, ausencia de modos altos o causa de falta de cuerpo.
Acción de carga contra malla fina: **4,43480% a 6 kHz**, **14,3412% a
16 kHz**, **72,4721% a 24 kHz**; distinta dirección de fuente que el coast.
No ampliar aprobación de carga, retirar modos o retocar fieltro por estos datos.

Siguiente: Hann estable, contribución absoluta por banda y malla relevante;
guardar sólo vectores espectrales pequeños de sondas para reutilizarlas.
Después ledger angular/duración/release, con la eliminación lineal todavía
pendiente de implementación y validación. Global falso, sin audio/paquete.

### 2026-10-08 — P6: Hann local y malla de sondas reutilizables

[Diseño, resultados e incidencia de cierre](RESULTADOS-P6-Hann-local.md).
**beeb90a** agrega pesos Hann locales antes de la contracción espectral,
espaciados reales, momentos acotados y suma compensada; seis pruebas pasan
(Hann y contribución absoluta). Una reconstrucción fina 3072 kHz reproduce
trece medidas físicas con error 0 y los cuatro arrays finales exactamente.
Medición **1523,050 s (25,38 min)**, contacto **1023,455 s (17,06 min)**,
pico **5,256 GiB**; misma R/cápsula y ganancias.

V Hann a 24 kHz baja de **4,37686e−10** global a **2,60361e−11** local;
V/A locales pasan 1e−10 en nueve sondas, A máxima **9,60383e−11** pasa
con poco margen. Orden local 8/12: 0; bloques: máximo 9,74113e−12.
La tasa gruesa no se vuelve a medir; sus gates permanecen sin reemplazar.

Rayleigh 512×384→1024×768 pasa 2% en seis de siete sondas; **16 kHz
falla con 2,55585%**. Cuadratura 12/16 máxima 2,48363e−15. Z vigente
frente a 1024 difiere 5,36621% a 6 kHz, 15,9305% a 16 kHz y 72,4386%
a 24 kHz; no aprobar el operador ni cambiar R a partir de estas acciones.
Guardar siete vectores complejos por método permite próximas mallas sin
reconstruir el contacto; sólo datos pequeños en informe, sin historias/WAV.

Al resumir, el helper antiguo rechaza listas después de guardar las siete
sondas. Productor/checkpoint congelados; cierre independiente autentica
datos/archivos y termina **8/9 controles locales**, conservando el fallo.
Dos regresiones del cierre pasan, sin repetir dinámica ni duplicar vectores.
Contribución absoluta del error Hann 12–24 kHz a presión total: **0,000271615%**;
su error relativo de banda **2,03722%** sigue fallando. No es un umbral audible.

Sigue tasa gruesa con Hann local y sonda de 16 kHz a 2048; después resolver
paso/carga, ledger angular y duración/release con presupuesto. Puerto discreto
todavía sin implementar; global falso, plugin instalado intacto.

### 2026-10-08 — P6: cierre de Hann de V y sonda de 16 kHz

[Diseño, fallo de A y resultados](RESULTADOS-P6-Hann-gruesa-malla16.md).
**9d2e588** agrega contacto grueso a 1536 kHz y reutilización autenticada
del vector fino de 16 kHz; tres pruebas pasan. Ensayo **816,736 s
(13,61 min)**, contacto **516,391 s (8,61 min)**, pico **4,309 GiB**.
Trece medidas físicas reproducidas con error 0, misma R/cápsula; no volver
a integrar contacto fino ni guardar otras historias/vectores.

**11/12 controles locales**: V local pasa 1e−10 con **9,00934e−11**;
A local falla por **1,06024e−10**. Se conserva el fallo. A analítica
anterior pasa **4,72080e−12** en gruesa y **6,14836e−12** en fina.
**d9fd3bd** implementa y mide por separado V local/A anterior de la misma
trayectoria/ventana; tres pruebas pasan y perfil de ambas tasas aprobado,
sin cambiar el límite ni sobrescribir productores/gates anteriores.

Sonda 16 kHz: cambio 512→1024 conserva fallo **2,55585%**; 1024→2048
da **0,651384%**, cuadratura 12/16 **1,15314e−15**. Reproducción del
padre exacta. Z vigente frente a 2048 sigue difiriendo **16,3568%**.
Las siete sondas pasan su refinamiento direccional con referencias mixtas
(seis hasta 1024, una hasta 2048); no es convergencia de operador uniforme.
La dirección de fuente temporal sigue cambiando hasta **2,28763%**.

Se cierran Hann de V y refinamiento de esta sonda. Mantener pendientes
temporal alto y carga frente a malla fina. Sigue presupuesto/método del
ledger angular del contacto y derivación/validación del puerto discreto
para reutilizar tabla/aire antes de más contactos largos. Después duración,
release, unísono e identificación/escucha. Global falso, sin audio/paquete.

### 2026-10-08 — P6: prioridad de escucha del prototipo actual

[Diseño y límites de la escucha](RESULTADOS-P6-escucha-actual.md).
Se mantienen las 606 coordenadas mecánicas y la cuerda no lineal 96/192;
se reduce sólo la propagación acústica mediante una congruencia pasiva y
dos ramas positivas que conservan masa estática y límite resistivo.
La relajación original se conserva al 99,9 %. Tres pruebas pasan.

La reducción de 192 coordenadas acústicas **falla** el umbral del 5 % de
presión en nueve controles independientes: máximo **9,2021 %**. Se registra
como escucha diagnóstica explícita, sin aprobar el operador completo ni
reutilizar su cápsula. Primera comparación corta a 16/32/64/128 kHz contra
128: presión 60–6000 Hz cambia **1,94714 / 0,917188 / 0,0232943 / 0 %**.
Se elige 16 kHz sólo para este prototipo, con ataque a 768 kHz hasta la
separación, observación a 128 kHz y WAV a 48 kHz. No es convergencia de
duración completa o de todo el teclado.

Render de C4, F4, G4 y C5 a intensidad 84, tecla sostenida 350 ms y apagador
distribuido con 650 ms de cola, sin fade ni EQ. Audio sólo en
`C:/SHARED/Piano-actual-2026-10-08`; informes en el repo, ninguna R/cápsula
completa adicional. Se preservan los pendientes anteriores y falta unísono,
identificación y validación de duración. Global falso; plugin instalado intacto.
