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
