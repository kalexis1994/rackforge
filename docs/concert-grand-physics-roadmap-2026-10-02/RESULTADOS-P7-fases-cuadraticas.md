# P7 — Convolución cuadrática con las fases modales

2026-10-13. Continuación de la [referencia temporal refinada](RESULTADOS-P7-referencia-tiempo.md).
El motor y la referencia anteriores permanecen sin cambios. Se implementa un
componente experimental para sustituir la interpolación de una parte de la
fuerza por su convolución analítica. No constituye un paso no lineal aceptado.

## Operador y alcance

Se conserva el gradiente de la parte cúbica del potencial SI:

`V3 = EA integral u_x (v_x² + w_x²)/2 dx`.

Sus fuerzas son cuadráticas: `gu = EA Du' eta`, `gv = EA Dv'(u_x v_x)`
y `gw = EA Dw'(u_x w_x)`, con `eta = (v_x² + w_x²)/2`. Queda fuera la
fuerza cúbica que procede de `EA integral eta²/2 dx`, así como contacto,
cuerpo, presión y pedales. No se cambia el potencial del motor vigente.

Cada coordenada sigue el portador libre exacto
`q_i(t) = q_i(0) cos(omega_i t) + v_i(0)/omega_i sin(omega_i t)`.
La integral espacial de tres cosenos vale `L/4` sólo para
`j = k+l` o `j = |k-l| > 0`. Se enumeran esos productos, incluidos los
diagonales con su multiplicidad correcta, y ambos ejes transversales.
Los productos temporales tienen frecuencias firmadas `omega_i ± omega_j`.
Se preparan sus respuestas contra `sin(omega_o(h-t))/omega_o` y
`cos(omega_o(h-t))`, conservando resonancias mediante `sinc`, sin divisiones
por una diferencia de frecuencias ni descarte de modos.

El resultado son **incrementos forzados** `delta q`, `delta v`; no los estados
finales de un solver. La trayectoria libre es una hipótesis de evaluación,
no una solución del sistema no lineal. Hace falta resolver la trayectoria
acoplada y comprobar trabajo antes de incorporar este componente al motor.
Cerrar exactamente esta integral no prueba conservación ni convergencia
temporal de una nota completa.

## Control independiente

[Productor y datos congelados](medidas-P7-cuerdas-nativas-2026-10-13/phase-forces.json).
Tres geometrías C3/C4/C5, tres cuerdas 96/192, ambos ejes y estado mixto
seed 83. Bloque de 16/128000 s = 125 microsegundos, sin lanzamientos.
La referencia usa matrices espaciales densas y Gauss temporal de orden
1024/2048; la prueba unitaria también usa Gauss espacial por celdas, sin
enumerar triadas. La métrica es la norma de `(omega delta q, delta v)`.

| Geometría | Ciclos del portador axial máximo por bloque | Cambio Gauss 1024/2048 | Error analítico frente a 2048 | Error de fuerza afín entre extremos |
|---|---:|---:|---:|---:|
| C3 | 48,847 | 1,748e−11 | 1,881e−11 | 108,082 % |
| C4 | 97,694 | 2,643e−11 | 2,802e−11 | 100,489 % |
| C5 | 195,389 | 3,478e−11 | 3,836e−11 | 96,240 % |

El error de la última columna se refiere a **esta respuesta cuadrática sobre
portadores libres**, no al estado de la nota, la presión ni un criterio audible.
La integral muestra por qué los dos extremos de fuerza no bastan para estos
portadores. No demuestra por sí sola la causa exclusiva del fallo temporal
mixto del motor. Los seis controles native/portable pasan 1e−9; ambas
compilaciones producen los mismos incrementos en estos fixtures.

## Costo inicial: rechazado para tiempo real

82.656 términos preparados para el coro, **7.953.408 bytes** de tablas y
scratch, sin contar las estructuras propietarias y buffers del llamador.
Preparación fuera del procesamiento: alrededor de 22–24 ms en este ensayo.
Se repite el mismo estado 16 veces por buffer de 256 microframes, 512 buffers,
sin avanzar dinámica ni resolver ninguna iteración. Cero asignaciones medidas.

| Geometría | Native media / P99 / máximo, ms | Portable media / P99 / máximo, ms |
|---|---:|---:|
| C3 | 5,019 / 8,601 / 9,367 | 5,011 / 5,988 / 9,585 |
| C4 | 4,891 / 7,070 / 8,505 | 5,038 / 6,835 / 9,137 |
| C5 | 4,913 / 6,998 / 8,707 | 5,120 / 8,712 / 9,282 |

**Falla el presupuesto de 2 ms por sí solo**. No sustituye la ruta actual
ni modifica sus gates. La tabla exacta sirve como control de nuevas
factorizaciones; no se selecciona como solver rápido ni se instala.

## Contracción SIMD y reutilización entre ejes

La versión inicial queda en **60a6b662**, con fuentes, medidas y rechazo CPU.
La [medición posterior](medidas-P7-cuerdas-nativas-2026-10-13/phase-forces-simd.json)
autentica ese control mediante los bytes de fuentes en Git y SHA256 de sus
ejecutables, fixtures y resultados. Los archivos anteriores se conservan.

La ruta nueva prepara el despacho AVX2 una vez, con fallback portable.
Contrae los cuatro productos de fase en paralelo. En cada término transversal
se calcula una sola vez la contracción con el portador axial izquierdo y
se reutiliza para los ejes vertical y horizontal. Sólo cambia el orden de
operaciones; términos, tablas, estados físicos y respuesta analítica se
conservan. Las cargas SIMD son de cuatro doubles dentro de cada fila;
no requieren alineación ni leen fuera de ella.

El control portable conserva el orden expandido original y reproduce
**exactamente** los arrays `delta q/delta v` de 60a6b662 en las tres geometrías.
AVX2 difiere **1,13–1,71e−16** de ese control en la métrica de respuesta;
ambas rutas siguen pasando la cuadratura independiente. Cero asignaciones.

| Geometría | AVX2 media / P99 / máximo, ms | Portable contemporáneo media / P99 / máximo, ms | Aceleración de media |
|---|---:|---:|---:|
| C3 | 3,723 / 5,916 / 9,342 | 4,784 / 5,226 / 6,368 | 1,285× |
| C4 | 3,788 / 6,013 / 9,908 | 4,781 / 5,217 / 6,624 | 1,262× |
| C5 | 3,657 / 4,292 / 5,742 | 4,783 / 5,138 / 5,819 | 1,308× |

512 buffers por ruta, mismo estado mixto y bloque de 125 microsegundos.
La media baja **20,8–23,5 %** frente al control portable contemporáneo;
el máximo no mejora uniformemente y C4 empeora frente al ensayo inicial.
**Ambas rutas siguen fallando 2 ms**, incluso sin iteraciones/contacto.
No atribuir estas medidas al tiempo del motor vigente ni sustituir sus gates.

## Agrupación por salida y tablas compactas

Continuación sobre **b1f4ce75**, con el motor y la referencia temporal intactos.
[Medición de 512 buffers](medidas-P7-cuerdas-nativas-2026-10-13/phase-forces-grouped.json)
y [repetición independiente](medidas-P7-cuerdas-nativas-2026-10-13/phase-forces-grouped-repeat.json).
Ambas autentican fuentes de control desde Git, ejecutables, mismos fixtures
y arrays anteriores. Se conserva también el pico alto de la primera corrida.

Los términos se agrupan de manera estable por modo de salida. Cada salida
acumula los cuatro canales de fase en registros SIMD y se reduce al terminar
su grupo, en lugar de reducir/escribir cada término. Para el coro se pasa de
137.664 reducciones de pares `delta q/delta v` a 1.152 por evaluación; esto
no implica esa misma aceleración de CPU. La ruta portable mantiene la suma
escalar original dentro de cada salida y reproduce sus arrays exactamente.

Se separan las filas de respuesta, los índices y los coeficientes usados por
el diagnóstico puntual. Las filas de 64 bytes tienen alineación de 32 bytes;
ambas cargas AVX2 de cuatro doubles quedan dentro de la fila. Los índices
se almacenan en 16 bits: el constructor limita 256/512 modos y como máximo
1.024 coordenadas por cuerda. **Los estados, frecuencias y coeficientes siguen
en f64**; no se retira ninguna de las 82.656 interacciones preparadas.

El procesamiento comprueba las longitudes de los estados al entrar. Los
índices de lectura de la ruta SIMD se mantienen privados y proceden de la
enumeración del constructor: axial en `[nl,nl+nt)`, transversal izquierdo en `[0,nl)` y derecho
en `[nl,nl+nt)`. Sumar `nt` selecciona el otro eje dentro del mismo estado.
Esto permite evitar comprobaciones repetidas de esos límites en cada producto.
Se prueba además el último índice físico con una integral temporal continua
independiente, usando **256/512 modos** y ambos ejes, sin referencia densa grande.

Memoria preparada: **7.953.408 → 6.307.248 bytes**, baja **20,7 %**.
Las tablas consultadas por el callback ocupan **5.627.568 bytes**; el resto
incluye los coeficientes de diagnóstico y vectores de frecuencias/amplitudes.
No se cuentan estructuras propietarias o buffers del llamador, como en la
medida inicial. Preparación fuera del procesamiento: **30,5–32,3 ms**.
Cero asignaciones durante los buffers medidos.

| Geometría | AVX2 media / P99 / máximo, ms | Repetición AVX2 media / P99 / máximo, ms | Media AVX2 en b1f4ce75, ms |
|---|---:|---:|---:|
| C3 | 1,747 / 2,527 / 3,022 | 1,708 / 2,499 / 2,615 | 3,723 |
| C4 | 1,704 / 2,409 / 2,691 | 1,841 / 2,871 / 6,793 | 3,788 |
| C5 | 1,717 / 2,429 / 2,586 | 1,679 / 2,334 / 2,467 | 3,657 |

C4 mejora **2,06–2,22× en media** frente al control congelado, sin cambios
del operador. La diferencia de respuesta AVX2 queda **2,47–2,70e−16**;
error frente a Gauss refinado **1,88–3,84e−11**, mismos gates sin relajar.
Las dos ejecuciones producen los mismos incrementos por backend.

Portable contemporáneo, media/P99/máximo en ms: C3 **4,852/5,226/5,970**,
C4 **4,864/5,331/5,747**, C5 **5,327/14,378/28,677**. La repetición conserva
C3 **4,857/5,247/7,459**, C4 **4,884/5,769/6,699**, C5 **4,875/5,313/6,271**.
El pico de C5 no se reproduce, pero se conserva; no se atribuye una causa
sin evidencia. No se selecciona la ruta portable como mejora de CPU.

**Sigue fallando el plazo de 2 ms en todas las geometrías y ejecuciones**:
P99 y máximo AVX2 quedan por encima, y la repetición C4 alcanza 6,793 ms.
Que la media pase no aprueba tiempo real. Es una evaluación cuadrática sobre
un estado fijo repetido: aún faltan trayectoria no lineal acoplada, fuerza
cúbica, contacto, comprobación de trabajo y resto del instrumento.

## Validación y siguiente paso

30 pruebas pasan: cuatro nuevas de convolución, resonancia/signo/cero,
homogeneidad cuadrática, intercambio de ejes y estructura con axial cero;
las 26 existentes siguen pasando. Los fuentes del motor anterior y sus
medidas permanecen autenticados. Los datos grandes siguen en `local/`;
no se genera audio ni otra copia de fuentes o calibración.

Después de SIMD pasan las cinco pruebas del componente, incluida la paridad
SIMD/portable a 4 y 96 modos. Las 26 pruebas del motor/cuerpo pasaron en la
corrida previa; sus fuentes no cambian durante esta continuación. La suite
acumulada contiene 31 pruebas. No se vuelve a integrar la referencia larga.

Después de agrupar pasan las seis pruebas actuales del componente, incluida
la nueva integral de modos máximos. Los 26 controles anteriores mantienen
sus fuentes sin cambios; la suite acumulada contiene ahora 32 pruebas.
No se vuelven a ejecutar los contactos largos para esta modificación aislada.

Sigue reducir tablas y contracción con una factorización temporal controlada,
sin retirar coordenadas físicas, y luego evaluar
una representación temporal acoplada que incluya fuerza cúbica y trabajo.
El límite CPU de esta etapa no acredita el presupuesto completo: faltan
iteraciones, contacto, cuerpo/presión, pedales y polifonía. Global falso.
