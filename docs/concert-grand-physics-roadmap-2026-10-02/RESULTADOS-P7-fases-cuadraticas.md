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

Sigue reducir tablas y contracción con una factorización temporal controlada,
sin retirar coordenadas físicas, y luego evaluar
una representación temporal acoplada que incluya fuerza cúbica y trabajo.
El límite CPU de esta etapa no acredita el presupuesto completo: faltan
iteraciones, contacto, cuerpo/presión, pedales y polifonía. Global falso.
