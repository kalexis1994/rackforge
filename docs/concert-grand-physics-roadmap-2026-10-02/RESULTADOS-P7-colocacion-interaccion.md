# P7 — Colocación temporal en el marco de interacción

Fecha: 2026-10-13. Prototipo independiente de fábrica, de cuerdas a puente
fijo, sin martillo/cuerpo/presión/pedales. La hipótesis es representar la
respuesta transversal dentro de cada bloque para reducir el reloj necesario.
El [informe reproducible](medidas-P7-cuerdas-nativas-2026-10-13/phase-collocation.json)
autentica fuentes, ejecutables, entradas y resultados. Control congelado:
**aff59a7e**; referencias anteriores de 8,192/16,384 MHz reutilizadas.

## Método y alcance

`--collocation` conserva los 1152 modos del coro 96/192, sus frecuencias,
inercia y potencial SI. Propaga exactamente el operador lineal `R(t)` y
resuelve el gradiente no lineal **completo** en los nodos temporales Gauss.
El gradiente usa la cuadratura espacial nativa original. No limita amplitud,
recorta rigidez, elimina modos ni introduce pérdidas físicas.

En el marco de interacción, las integrales `Aij` de los polinomios cardinales
se preparan mediante ortogonalidad de Legendre. La ecuación de posición es:

`q_i = q_libre(t_i) − Σ_j Aij sin(omega*(t_i−t_j))/omega * g(q_j)`.

El extremo se reconstruye con pesos Gauss `b_j` y los mismos gradientes:

`q1 = q_libre(h) − Σ_j b_j sin(omega*(h−t_j))/omega * g(q_j)`;

`v1 = v_libre(h) − Σ_j b_j cos(omega*(h−t_j)) * g(q_j)`.

Se preasignan tablas y buffers. La iteración acoplada de posiciones converge
con residual ponderado por omega ≤1e−11 y límite de 32 iteraciones. La fuerza
depende de posiciones; no se requiere una incógnita de velocidad en cada
nodo para este problema sin pérdidas ni contacto. Una vez convergidos los
nodos, las raíces independientes existentes cierran la energía de cada
cuerda. **La corrección energética es posterior a la colocación**, a
diferencia de la restricción dentro del centro del método anterior.
q/v se publican por paso sólo después de resolver nodos y raíces; un fallo
deja el estado físico de ese paso intacto. `--unconstrained` permite medir
la trayectoria sin esa corrección.

Esta es una cuadratura temporal finita: los portadores lineales son exactos,
pero hay que resolver también las oscilaciones del gradiente en el marco de
interacción. Se preserva un caso de orden insuficiente para comprobarlo.
El método previo sigue disponible y reproduce exactamente su q/v, ledger
y lambda congelados en el mismo ensayo de 6 ms.

## Estado, energía y refinamiento

Todos los casos usan C4 mixto, semilla 83, tres cuerdas 96/192 y **6 ms**,
con las mismas condiciones iniciales y parámetros mecánicos. El maestro
8,192→16,384 MHz cambia **0,118467 %**; esa incertidumbre de referencia se
conserva y no permite atribuir precisión absoluta al último decimal.
El gate de estado sigue siendo 1 %, y los de ledger total y energía final
por cuerda 1e−9. La auditoría energética usa matrices espaciales densas
independientes del solver y de su FFT.

| Método / reloj / orden | Error de estado frente a 16,384 MHz | Máximo error energético por cuerda | Resultado local |
|---|---:|---:|---|
| Centro anterior, 2,048 MHz / 8 | 0,638741 % | 1,858e−10 | Pasa los gates de 6 ms |
| Colocación, 128 kHz / 16 | **23,274959 %** | 3,369e−13 | **Rechazado por estado** |
| Colocación, 128 kHz / 24 | 0,053051 % | 1,800e−13 | Pasa |
| Colocación, 128 kHz / 32 | 0,040553 % | 8,665e−13 | Pasa |
| Colocación, 256 kHz / 16 | 0,040580 % | 8,462e−14 | Pasa |
| Colocación, 256 kHz / 24 | 0,040552 % | 3,910e−14 | Pasa |
| Colocación, 512 kHz / 8 | 0,044801 % | 7,755e−14 | Pasa |
| Colocación, 512 kHz / 16 | 0,040552 % | 8,270e−14 | Pasa |

Todos convergen y conservan energía, incluido el rechazado. Cerrar energía
no valida la discretización temporal. En 128 kHz/16, lambda llega a
1,588e−4; el error de estado sigue siendo 23,3 %.

Refinar orden cambia estado en **0,024748 %** a 128 kHz/24→32,
**0,0000908 %** a 256 kHz/16→24 y **0,009415 %** a 512 kHz/8→16.
Con órdenes altos, cambiar reloj 128 kHz/32→256 kHz/24 cambia **1,192e−8**
en norma relativa, y 256 kHz/24→512 kHz/16 cambia **7,008e−13**.
Así se separa el error de cuadratura de la diferencia con el maestro fino.

A 256 kHz/24, el caso **sin restricción energética** produce exactamente
el mismo q/v final que el restringido: lambda es cero, ledger máximo
**2,114e−14** y energía por cuerda **3,910e−14**. Este resultado y la
convergencia contra ODE independiente sin proyección comprueban que la
mejora temporal no depende de ajustar el estado con la raíz energética.

## CPU con igual tiempo físico

`--buffer-microframes` permite comparar relojes con el mismo presupuesto.
Cada ejecución avanza **1 ms físico**, en ocho buffers de **0,125 ms**:
256 microframes a 2,048 MHz, 16 a 128 kHz, 32 a 256 kHz y 64 a 512 kHz.
Los benchmarks son secuenciales, sin pruebas simultáneas, e incluyen la
evaluación energética de cada paso. Ninguno asigna memoria al avanzar.

| Ruta | Media por buffer | P99/máximo | Tablas/buffers contabilizados |
|---|---:|---:|---:|
| Control aff59a7e, 2,048 MHz | 636,447 ms | 687,519 ms | 16.321.032 bytes |
| Colocación 128 kHz / 24 | 105,508 ms | 149,578 ms | 7.077.888 bytes |
| Colocación 256 kHz / 16 | 108,011 ms | 116,977 ms | 3.538.944 bytes |
| Colocación 512 kHz / 8 | **92,823 ms** | **95,330 ms** | **1.179.648 bytes** |
| Repetición 512 kHz / 8 | **94,812 ms** | **109,137 ms** | **1.179.648 bytes** |

La media a 512 kHz/8 mejora **6,86×**, repetición **6,71×**, frente al
control medido durante esta etapa y con igual duración física. El reloj es
cuatro veces menor; 128 kHz/24 también pasa estado y energía con un reloj
16 veces menor. La memoria indicada corresponde a tablas y buffers
contabilizados, no al consumo total del proceso.

**CPU sigue rechazado**: los máximos de 512 kHz superan el plazo de 0,125 ms
unas **763/873 veces**. Los benchmarks también pasan ledger y energía por
cuerda, pero no amplían el gate de estado más allá del ensayo de 6 ms.
No se usa esta mejora como estimación del render de una pieza completa.

## Validación y continuación

Pasan **20 pruebas actuales** de fases/trayectoria. Cuatro regresiones nuevas
verifican momentos polinómicos de la matriz de integración en órdenes 8/16/32,
refinamiento contra ODE continuo sin proyección, fallo sin publicación y
cuerda vacía, y duración física del benchmark. La prueba de portadores axiales
libres también ejercita el método nuevo. Se mantienen los controles anteriores
de gradiente espacial, energía por cuerda y trayectoria. Las 26 pruebas del
núcleo nativo/presión pasaron en la etapa anterior; sus algoritmos no cambian.
La suite acumulada contiene **46 pruebas**.

El nuevo control evita las contracciones de tríadas del centro anterior,
pero exige varias evaluaciones del gradiente y una matriz temporal densa.
Sigue reducir ese costo, mantener el refinamiento de orden/reloj y extender
la validación de duración antes de martillo, cuerpo, presión, pedales y
polifonía. El flag experimental no selecciona, instala ni modifica fábrica;
todavía no hay instrumento tocable integrado.

## Reutilización de las fuerzas al converger

La [medición de reutilización](medidas-P7-cuerdas-nativas-2026-10-13/collocation-force-reuse.json)
autentica el control **98487265** y reutiliza sus referencias finas. La ruta
anterior calculaba `F(q)`, verificaba `|F(q)−q|` y, al pasar el gate, actualizaba
otra vez los nodos y recalculaba todas sus fuerzas. La ruta nueva usa las
fuerzas de los nodos `q` que ya cumplen ese residual. No cambia la ecuación,
el límite de iteraciones ni la tolerancia **1e−11**; elimina una pasada de
fuerzas por paso. La reconstrucción del extremo y las raíces energéticas
siguen usando el mismo potencial SI. El estado físico se publica después
de completar todas las comprobaciones.

`--refresh-converged-forces` conserva la actualización y evaluación finales
anteriores. Reproduce **exactamente** q/v, ledger y lambda de 98487265 en los
tres controles de 6 ms. La reutilización cambia el estado respecto al control
en **2,559e−13 / 3,572e−12 / 1,450e−13** a 128/256/512 kHz; pasa un gate
de comparación 1e−9 además del gate de 1 % frente al maestro independiente.

| Reloj / orden | Error frente a 16,384 MHz | Máximo energético por cuerda | Ledger total máximo |
|---|---:|---:|---:|
| 128 kHz / 24 | 0,053051 % | 2,048e−13 | 1,239e−13 |
| 256 kHz / 16 | 0,040580 % | 4,974e−14 | 5,010e−14 |
| 512 kHz / 8 | 0,044801 % | 8,302e−14 | 2,883e−14 |

CPU vuelve a medirse secuencialmente con ocho buffers de **0,125 ms** y
**1 ms físico total**, incluyendo comprobaciones de energía por paso:

| Ruta | Media por buffer | P99/máximo | Asignaciones |
|---|---:|---:|---:|
| Ejecutable congelado 98487265, 512 kHz/8 | 91,442 ms | 96,460 ms | 0 |
| Ejecutable actual, refresco final | 98,953 ms | 107,234 ms | 0 |
| Reutilización 512 kHz/8 | **81,739 ms** | **83,877 ms** | **0** |
| Repetición de reutilización | **85,854 ms** | **93,305 ms** | **0** |
| Reutilización 256 kHz/16 | 94,355 ms | 100,778 ms | 0 |

Frente al ejecutable congelado, la media baja **10,61 %**, repetición
**6,11 %**. Los dos máximos también bajan en estos ensayos. Las tablas/buffers
contabilizados no crecen: **1.179.648 bytes** a 512 kHz/8; máximos de
iteraciones siguen en 8/7/6 para 128/256/512 kHz. Todos los benchmarks pasan
ledger y energía por cuerda. **CPU sigue rechazado**: 83,877/93,305 ms frente
a 0,125 ms, unas 671/746 veces el plazo. Las cifras no amplían la aprobación
de trayectoria fuera de los 6 ms.

Pasan **21 pruebas actuales** de fase/trayectoria. La nueva regresión compara
reutilización y refresco en cuatro/96 modos y tres relojes, verifica estado,
energía individual, residual y ausencia de asignaciones. Se mantiene el
refinamiento contra ODE continuo sin proyección. La suite acumulada contiene
47 pruebas; el núcleo nativo y presión no cambian en esta etapa.

Antes se probó acelerar la iteración con dos direcciones secantes Anderson
por cuerda. Se conserva el
[ensayo rechazado](medidas-P7-cuerdas-nativas-2026-10-13/collocation-anderson-rejected.json)
y su [patch reproducible](medidas-P7-cuerdas-nativas-2026-10-13/collocation-anderson-rejected.patch)
sobre 98487265: en los mismos 6 ms a 512 kHz/8, máximo **7 iteraciones** frente
a **6** y tiempo de evolución **4,990 s** frente a **4,250 s**. Es tiempo total
del sostenido, no el benchmark por callback. Se descarta por mayor trabajo;
su código no queda en la ruta actual.

Sigue reducir las evaluaciones y contracciones de la colocación, y extender
duración/contacto antes de integración con cuerpo, presión, pedales y polifonía.

## Transformadas agrupadas de cuatro nodos — 2026-10-14

La [medición agrupada](medidas-P7-cuerdas-nativas-2026-10-14/collocation-batch.json)
autentica **ca7e5f87**, conserva los tres controles de 6 ms y reutiliza las
referencias finas anteriores. La hipótesis es aprovechar que las fuerzas
de distintos nodos temporales usan los mismos coeficientes de transformación.

El nuevo `concert-grand-batch-cosine.rs` procesa cuatro nodos independientes
en paralelo. Cada componente real/imaginaria contiene cuatro valores f64,
alineados a 32 bytes; las operaciones ordinarias permiten autovectorización
del compilador. El ensamblador local con `target-cpu=native` contiene operaciones
de cuatro doubles en registros YMM en la síntesis agrupada. No se agregan
intrinsics ni accesos sin comprobación de límites. Se reutilizan las raíces,
fases y permutaciones preparadas del plan de cosenos existente.

Cada canal sintetiza los tres campos y analiza los tres componentes del
gradiente con la misma cuadratura y normalización. Los canales no se mezclan;
no se promedian fuerzas, recortan modos ni limitan amplitudes. El cálculo
agrupado devuelve sólo el gradiente consumido por la iteración; las evaluaciones
de energía y las raíces siguen usando la ruta completa individual. Las
asignaciones ocurren en la preparación. Para órdenes que no son múltiplos
de cuatro, los nodos restantes usan la evaluación individual.

La ruta nueva se limita a la biblioteca experimental de fases. El núcleo
normal de contacto conserva sus algoritmos. `--scalar-collocation-forces`
recupera la evaluación individual y libera el workspace agrupado antes del
procesamiento. La opción de refresco final anterior también se conserva.

Los seis replays de **6 ms** —agrupado e individual a 128 kHz/24, 256 kHz/16
y 512 kHz/8— reproducen **exactamente q/v, ledger total y lambda de ca7e5f87**.
El máximo de iteraciones sigue en 8/7/6. Los errores frente al maestro permanecen
en **0,053051 / 0,040580 / 0,044801 %** y los errores finales por cuerda en
**2,048e−13 / 4,974e−14 / 8,302e−14**. Todos pasan sus gates de estado y energía.

Los benchmarks son secuenciales, sin pruebas simultáneas, y avanzan el mismo
**1 ms físico** en ocho buffers de **0,125 ms**. Cada paso incluye sus
comprobaciones de energía:

| Ruta | Media por buffer | P99/máximo | Asignaciones |
|---|---:|---:|---:|
| Ejecutable congelado ca7e5f87, 512 kHz/8 | 77,829 ms | 78,667 ms | 0 |
| Ejecutable actual, nodos individuales | 82,146 ms | 84,012 ms | 0 |
| Cuatro nodos agrupados, 512 kHz/8 | **47,915 ms** | **48,733 ms** | **0** |
| Repetición agrupada | **48,068 ms** | **50,052 ms** | **0** |
| Repetición del ejecutable congelado | 78,155 ms | 79,330 ms | 0 |
| Cuatro nodos agrupados, 256 kHz/16 | 54,237 ms | 56,335 ms | 0 |

La media mejora **1,624× / 38,44 %** frente al primer control; la repetición
mejora **1,626× / 38,50 %** frente al control repetido. Los máximos también
bajan en estas mediciones. Todos los benchmarks pasan energía por cuerda y
ledger. Las tablas y buffers contabilizados suman **1.351.680 bytes** a
512 kHz/8, incremento de **172.032 bytes** por los workspaces de las tres
cuerdas. No es memoria total del proceso. **CPU sigue rechazado**: máximos
48,733/50,052 ms frente a 0,125 ms, unas **390/400 veces** el presupuesto.

Pasan las **49 pruebas** actuales: 17 de trayectoria, seis de fases, 22 del
núcleo de cuerdas y cuatro de presión. Las dos regresiones nuevas comparan
gradientes agrupados e individuales y una integración espacial Gauss
independiente con 4/96/256 modos transversales y hasta 512 axiales. Incluyen
un canal exactamente vacío y otro con amplitud 1e−12 del control, que permanece
activo. La prueba de trayectoria compara estado, ledger y lambda exactos a
tres relojes y prueba un orden 10 con dos nodos restantes individuales. No
cambia el gate de residual 1e−11 ni el de conservación energética 1e−9.

La mejora mantiene el alcance del sostenido mixto de 6 ms sin contacto, cuerpo,
presión, pedales o polifonía. Sigue reducir el costo y extender la validación
temporal antes de seleccionar una ruta para el instrumento tocable.

## Recorrido del residual y alternancia de buffers — 2026-10-14

La [medición del recorrido nodal](medidas-P7-cuerdas-nativas-2026-10-14/collocation-node-updates.json)
autentica el control **983f634a**, sus fuentes, ejecutable y resultados.
Se mantienen las referencias finas y los tres ensayos de 6 ms.

El residual se calcula nodo por nodo, recorriendo directamente el vector
de frecuencias. Desaparece `k % size` por coordenada y se conserva el orden
original de las sumas y multiplicaciones. Tras una iteración no convergida,
se intercambian los dos buffers preasignados: el siguiente cálculo sobrescribe
completamente el buffer de destino desde el portador libre. No se cambia
ningún nodo, fuerza, peso de cuadratura, tolerancia ni ecuación de evolución.
El refresco final opcional conserva su semántica anterior.

`--legacy-node-updates` recupera el recorrido lineal con módulo y la copia de
buffers. Tanto la ruta nueva como ese control reproducen **exactamente q/v,
ledger, lambda, residual máximo e iteraciones** del snapshot en los tres
relojes. Los errores frente al maestro siguen en **0,053051 / 0,040580 /
0,044801 %** a 128/256/512 kHz, pasando también energía por cuerda y ledger.

Benchmarks secuenciales, sin pruebas simultáneas, con ocho buffers de
**0,125 ms** y **1 ms físico total**. Se incluyen las comprobaciones de energía:

| Ruta | Media por buffer | P99/máximo | Asignaciones |
|---|---:|---:|---:|
| Control congelado 983f634a, 512 kHz/8 | 47,954 ms | 48,711 ms | 0 |
| Ejecutable actual, recorrido anterior | 48,052 ms | 49,290 ms | 0 |
| Recorrido por bloques, 512 kHz/8 | **45,271 ms** | **46,276 ms** | **0** |
| Repetición por bloques | **46,488 ms** | **50,486 ms** | **0** |
| Repetición del control congelado | 50,432 ms | 57,303 ms | 0 |
| Recorrido por bloques, 256 kHz/16 | 54,304 ms | 57,104 ms | 0 |

La media baja **5,60 %** en la primera comparación y **7,82 %** frente al
control repetido. Hay variación visible entre ejecuciones: el máximo de la
repetición nueva supera el máximo del primer control. El ahorro medio es
pequeño y no establece una mejora estable de la cola. No se agrega memoria:
tablas/buffers contabilizados permanecen en **1.351.680 bytes** a 512 kHz/8.
Todos los benchmarks pasan ledger y energía individual.

Pasan **24 pruebas actuales** de fases y trayectoria. La regresión nueva
compara ambas rutas a tres relojes, con cuatro/96 modos y un orden 10 que usa
la cola escalar. También fuerza un fallo tras dos iteraciones, verificando
que el intercambio interno de buffers no publica estado físico parcial.
Las 26 pruebas del núcleo nativo/presión pasaron en la etapa anterior;
esos algoritmos no cambian aquí. La suite acumulada contiene **50 pruebas**.

**CPU sigue rechazado**: máximos 46,276/50,486 ms frente a 0,125 ms, unas
370/404 veces el plazo. El alcance de estado sigue limitado al sostenido
mixto de 6 ms, sin contacto, cuerpo, presión, pedales o polifonía. Sigue
reducir el costo de la colocación y extender la validación temporal.

## Mapas preparados del empaquetado FFT — 2026-10-14

La [medición del empaquetado](medidas-P7-cuerdas-nativas-2026-10-14/collocation-prepared-packing.json)
autentica el control **e90ec2c5** y reutiliza sus tres ensayos de 6 ms y el
par de referencias finas. Se mantiene el núcleo FFT, la cuadratura espacial,
todos los modos y la propagación temporal anteriores.

Cada celda compleja del FFT real empaquetado depende de hasta cuatro
coordenadas modales. Sus índices, permutación y coeficientes se preparan
una sola vez aplicando la fórmula de empaquetado a esas bases. Los índices
duplicados en las celdas especiales se cuentan una sola vez. Cada salida
del análisis se prepara como combinación de las cuatro componentes reales
de dos celdas espectrales. Las tablas transversales se comparten entre los
ejes vertical y horizontal de cada cuerda.

En la evaluación se toman coordenadas vivas y se aplican esas combinaciones
f64 a los cuatro nodos independientes. Desaparecen del callback las fórmulas
de coeficientes del empaquetado y los índices espectrales del análisis. Las fuerzas
siguen siendo no lineales y se recalculan en cada iteración: se prepara sólo
la transformación lineal. No hay congelación de fuerzas, truncamiento modal,
cambio de rigidez, mezcla de nodos ni corte de amplitud.

La evaluación reordena sumas y productos de coma flotante. Las pruebas
exigen error de gradiente **<1e−13 relativo** frente al cálculo anterior,
además de la cuadratura Gauss espacial independiente con hasta 256/512 modos.
Los canales cero permanecen cero; el canal de amplitud 1e−12 sigue activo.
`--legacy-batch-packing` libera las tablas y recupera las fórmulas anteriores.
Reproduce exactamente q/v, ledger, lambda, residual máximo e iteraciones
de e90ec2c5 en los tres controles de 6 ms.

| Reloj / orden | Cambio de estado frente al control | Error frente al maestro | Máximo energético por cuerda |
|---|---:|---:|---:|
| 128 kHz / 24 | 1,071e−14 | 0,053051 % | 1,808e−13 |
| 256 kHz / 16 | 3,090e−14 | 0,040580 % | 5,344e−14 |
| 512 kHz / 8 | 3,607e−14 | 0,044801 % | 9,541e−14 |

Los tres candidatos pasan el gate adicional de comparación 1e−9 y mantienen
los gates originales de estado/energía. Ledger total máximo:
**1,082e−13 / 5,142e−14 / 5,718e−14**. Máximos de iteración siguen en 8/7/6;
el residual continúa limitado por 1e−11.

CPU se mide secuencialmente, sin pruebas simultáneas, en ocho buffers de
**0,125 ms** y **1 ms físico total**, con comprobación energética por paso:

| Ruta | Media por buffer | P99/máximo | Asignaciones |
|---|---:|---:|---:|
| Ejecutable congelado e90ec2c5, 512 kHz/8 | 44,531 ms | 44,967 ms | 0 |
| Ejecutable actual, empaquetado anterior | 45,073 ms | 46,043 ms | 0 |
| Mapas preparados, 512 kHz/8 | **33,592 ms** | **33,985 ms** | **0** |
| Repetición preparada | **33,093 ms** | **33,717 ms** | **0** |
| Repetición del control congelado | 45,094 ms | 46,192 ms | 0 |
| Mapas preparados, 256 kHz/16 | 36,748 ms | 37,676 ms | 0 |

La media mejora **1,326× / 24,56 %** en la primera comparación y
**1,363× / 26,61 %** frente al control repetido. Todos los benchmarks pasan
ledger y energía por cuerda. Las tablas/buffers contabilizados crecen
**127.488 bytes** para el coro, hasta **1.479.168 bytes** a 512 kHz/8;
no se mide aquí memoria total del proceso. La preparación y liberación de
esas tablas ocurren antes del contador de asignaciones del procesamiento.

Pasan **25 pruebas actuales** de fases/trayectoria. La regresión nueva compara
la trayectoria preparada y la anterior a tres relojes; los controles de
gradiente y de trayectoria con cola escalar ahora comprueban el pequeño
cambio de redondeo mediante normas relativas, y conservan la auditoría
energética independiente. Las 26 pruebas del núcleo normal/presión pasaron
en 983f634a, sin cambios a esos algoritmos aquí. La suite acumulada contiene
**51 pruebas**.

**CPU sigue rechazado**: máximos 33,985/33,717 ms frente a 0,125 ms, unas
272/270 veces el plazo. El alcance de precisión continúa en el sostenido
mixto de 6 ms, sin contacto, cuerpo, presión, pedales o polifonía. Sigue
reducir el costo y extender la validación antes de integrar el instrumento.

## Tres experimentos de FFT y acceso modal descartados — 2026-10-14

Se mantiene **52e96a0b** como implementación vigente. Tres candidatos
conservan exactamente q/v, ledger, lambda, residual e iteraciones de sus
tres controles de 6 ms a 128/256/512 kHz, pero no demuestran una mejora de
CPU suficiente y estable frente al ejecutable congelado. Sus cambios de
código se retiran y se conservan como parches independientes sobre ese commit.

1. **Fusionar dos etapas radix-2 consecutivas** reduce pasadas por memoria,
   manteniendo el orden de operaciones de cada mariposa. La media a
   512 kHz/8 empeora **33,562 → 36,390 ms**; repetición
   **33,543 → 34,370 ms**. Se descarta. Ver
   [medición y autenticación del parche](medidas-P7-cuerdas-nativas-2026-10-14/collocation-fused-stages-rejected.json)
   y [parche](medidas-P7-cuerdas-nativas-2026-10-14/collocation-fused-stages-rejected.patch).
2. **Especializar los tamaños de etapa al compilar** conserva las mariposas
   anteriores y expande los diez tamaños complejos admitidos. La media
   empeora **37,189 → 40,156 ms**; repetición **35,183 → 40,759 ms**.
   El control dinámico dentro del nuevo ejecutable da 33,944 ms. Se descarta.
   Ver [medición y autenticación](medidas-P7-cuerdas-nativas-2026-10-14/collocation-fixed-stages-rejected.json)
   y [parche](medidas-P7-cuerdas-nativas-2026-10-14/collocation-fixed-stages-rejected.patch).
3. **Reunir una sola vez las coordenadas de cuatro nodos por campo** evita
   reunirlas de nuevo en las celdas de síntesis que las comparten. El buffer
   se rellena con posiciones vivas en cada evaluación, sin cambiar operaciones
   aritméticas. Agrega 18.432 bytes al coro: 1.497.600 bytes contabilizados.
   En los ensayos cortos la media cambia **33,486 → 33,746 ms** y
   **37,004 → 32,841 ms**: el resultado depende de la comparación.
   Ver [medición](medidas-P7-cuerdas-nativas-2026-10-14/collocation-gathered-modes.json),
   [decisión y hashes](medidas-P7-cuerdas-nativas-2026-10-14/collocation-gathered-modes-rejected.json)
   y [parche](medidas-P7-cuerdas-nativas-2026-10-14/collocation-gathered-modes-rejected.patch).

Para resolver la incertidumbre del tercer caso se amplía sólo el ensayo
de CPU a **64 buffers de 0,125 ms: 8 ms físicos por ejecución**, a 512 kHz/8.
Se ejecuta secuencialmente control congelado, control directo dentro del
candidato, candidato, candidato repetido, directo repetido y congelado
repetido. El [productor](../../tools/measure-concert-grand-gather-benchmark.py)
autentica fuentes, ejecutables y entrada del ensayo corto, y guarda comandos,
hashes y resultados en el [informe largo](medidas-P7-cuerdas-nativas-2026-10-14/collocation-gathered-long-bench.json).

| Ruta, en orden de ejecución | Media por buffer | P99/máximo |
|---|---:|---:|
| Congelado 52e96a0b | 33,074 ms | 35,776 ms |
| Acceso directo, ejecutable candidato | 33,706 ms | 36,100 ms |
| Coordenadas reunidas | 32,976 ms | 38,654 ms |
| Coordenadas reunidas, repetición | 33,214 ms | 36,595 ms |
| Acceso directo, repetición | 34,669 ms | 45,004 ms |
| Congelado, repetición | 33,739 ms | 39,225 ms |

La mejora media frente al congelado es **0,30 % / 1,56 %**, con máximos que
no mejoran consistentemente. No se establece un ahorro estable de CPU y
se descarta también el buffer adicional. El control directo dentro del
candidato resulta más lento que el ejecutable anterior: compararlo sólo
contra ese control sobreestimaría el beneficio. No se adopta aquí un nuevo
umbral porcentual de aceptación.

Los seis ensayos largos terminan con cero asignaciones y con q/v, ledger,
lambda, residual, iteraciones y energías individuales exactamente iguales
al control. Pasan el cierre energético nativo por cuerda y el ledger total.
La auditoría energética espacial independiente y el contraste con los
maestros siguen limitados a **6 ms**; los 8 ms del benchmark no amplían la
validación de fidelidad temporal.

Cada candidato pasa **26 pruebas** de fases/trayectoria, incluyendo comparación
exacta de gradientes con accesos anteriores, cuadratura espacial independiente,
canales cero/pequeños y trayectoria con cola escalar. Los parches reconstruyen
exactamente todos los SHA-256 de fuentes medidos sobre 52e96a0b; se verificó
su aplicación por separado con `git -c core.autocrlf=false apply`.
Los archivos de medición corta/larga del tercer caso se mantienen sin
modificar y la decisión se guarda en un manifiesto separado para preservar
sus hashes encadenados.

Restaurada la implementación vigente, pasan sus **25 pruebas actuales**;
suite acumulada 51. Los algoritmos normales de cuerdas/presión no cambian.
CPU continúa rechazado frente a **0,125 ms** por buffer. Sigue medir el
costo separado de fuerzas, contracción temporal y raíces energéticas antes
de otra modificación del núcleo FFT; también sigue pendiente extender la
duración y añadir contacto/cuerpo/presión/pedales/polifonía.

## Perfil separado de colocación — 2026-10-14

El [productor de perfil](../../tools/measure-concert-grand-collocation-profile.py)
compila dos ejecutables de las mismas fuentes. `--cfg concert_phase_profile`
activa diez relojes de diagnóstico; el ejecutable normal los elimina al
compilar. No hay relojes adicionales ni buffers de perfil en el procesamiento
normal. El perfil usa 160 bytes de contadores y acumula intervalos disjuntos;
su serialización y asignaciones ocurren después del contador de procesamiento.

La [medición autenticada](medidas-P7-cuerdas-nativas-2026-10-14/collocation-profile.json)
reutiliza entradas y controles de fb4c030b, sin reintegrar maestros. Los seis
ensayos de 6 ms —normal/perfil a tres relojes— reproducen exactamente q/v,
ledger, lambda, residual, iteraciones y energías individuales congelados.
Los benchmarks usan 64 buffers de 0,125 ms, 8 ms físicos por ejecución.

| Sección / porcentaje del tiempo instrumentado | 512 kHz / 8 | 256 kHz / 16 | 128 kHz / 24 |
|---|---:|---:|---:|
| Fuerzas no lineales nodales | 52,44 % | 52,43 % | 48,14 % |
| Contracción temporal | 15,96 % | 28,68 % | 38,55 % |
| Residual | 6,65 % | 6,64 % | 6,07 % |
| Energías iniciales, transporte y dirección de corrección | 9,45 % | 4,22 % | 2,19 % |
| Proyección energética | 8,36 % | 3,80 % | 1,97 % |

Los porcentajes restantes incluyen inicialización nodal, extremo, energía
sin corregir, publicación, monitor energético y tiempo sin atribuir. La
contracción gana peso con el orden temporal; conviene medir una disposición
de pesos por pequeños bloques de modos antes de otra modificación de FFT.
La energía incluye comprobaciones de diagnóstico que permanecen contadas.

A 512 kHz/8, las medias normal/perfil son **35,450/35,640 ms**, frente a
**35,384 ms** del congelado. Repeticiones: **36,132/35,858 ms**, frente a
**36,715 ms**. No se interpreta esta variación como aceleración ni como una
estimación precisa de la sobrecarga de relojes. Los cuatro replays largos
normal/perfil y sus controles congelados son exactamente iguales y no
asignan memoria durante el procesamiento. Los tiempos instrumentados sirven
para distribuir el costo; el presupuesto se decide con el ejecutable normal.

Pasan **26 pruebas actuales** de fases/trayectoria; la nueva comprueba
transparencia del perfil, llamadas por etapa, suma de intervalos y cero
asignaciones. Suite acumulada 52. El alcance de fidelidad continúa en 6 ms,
con CPU rechazado frente a 0,125 ms y sin integración de contacto/cuerpo.
