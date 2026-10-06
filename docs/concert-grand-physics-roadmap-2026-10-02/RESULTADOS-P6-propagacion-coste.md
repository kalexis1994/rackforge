# P6 — Coste de la carga completa y propagación después del ataque

Fecha: 2026-10-06. Se continúa el
[contacto de tres milisegundos](RESULTADOS-P6-contacto-milisegundos.md).
Implementación de la contracción por lotes y auditor: **f8c614a**;
autenticación de fases de informes anteriores: **b163e90**.

## Hipótesis y alcance

Cada paso aplica los 649 residuos matriciales de la carga acústica a las
velocidades internas de las secciones. Son 606×606 coeficientes por sección;
R ocupa 1.909.627.328 bytes. La lectura repetida de esa matriz es una parte
importante del coste de la referencia completa. No se intenta mejorar el
timbre cambiando fieltro, pérdidas, afinación o modos físicos.

La versión anterior acumulaba la contracción con `einsum`. La nueva evalúa
el producto matriz/vector de cada sección con `matmul` y suma sus salidas.
Ambas implementan exactamente la misma expresión matemática:

```text
f_m = sum_k sum_n R[k,m,n] offsets[k,n]
```

Sólo cambia el orden de acumulación en coma flotante. Se conserva la
implementación anterior como control explícito `residue_kernel='reference'`.
La opción por defecto es `batched`. No se guarda una matriz transpuesta de
1,9 GB: el temporal de salidas mide 649×606×8 = **3.146.352 bytes**.
La R autenticada sigue siendo una única copia local de sólo lectura.

Diez pruebas dirigidas pasan en **3,018 s**: referencia midpoint matricial
independiente, identidad de trabajo con radiación directa, dos kernels con
memoria viva y tres tasas, contacto/relaunch, coast frente a DOP853,
observación con residuo acotado y refinamiento con intervalos. La primera
ejecución de la prueba nueva detectó que su fixture aleatorio no anulaba DC;
se corrigió la construcción del fixture antes de medir la referencia grande.
No se modificó el criterio ni la física para hacer pasar esa prueba.

## Ensayo completo

[Medición única](medidas-P6-propagacion-coste/pares-completos.json).
El auditor comparte la preparación de superficie/tabla y los controles de
carga con el auditor anterior. Se extraen funciones de ese mismo código;
la geometría y los coeficientes físicos congelados conservan sus hashes.
Se comprueba de nuevo la firma del proveedor, SHA256 de R, masa residual y
las tres sondas armónicas de construcción.

1. Reconstruir el recorrido frío de C4/5 m/s durante 3 ms a 768 kHz, con
   ambos kernels, desde idénticas condiciones iniciales. La comparación
   observa cada extremo completo y cada input interno de onda.
2. Comparar el extremo del control con el informe congelado: energía
   mecánica y de relajación, norma de memoria, compresión y trabajo/calor.
3. Desde el **mismo extremo del control**, medir otros 0,5 ms de propagación
   con martillo separado a 64/128/256 kHz. Mantener todos los modos y las
   memorias vivas; no relanzar el martillo.
4. Medir diferencia entre kernels y refinamiento de posición/velocidad
   físicas de la tabla en tiempos comunes de 64 kHz.

Cada par alterna qué kernel se ejecuta primero. Los planes y factores se
preparan antes de medir dinámica; inicialización y evaluación final no se
atribuyen al tiempo por paso. BLAS usa un hilo. Los tramos usan iguales
ecuaciones y tasa dentro de cada par; el paso exponencial del coast es otro
método numérico cuya precisión necesita su propio refinamiento.

Límites declarados: 1e−10 para diferencia entre kernels y extremo congelado;
1% para el par fino de tabla; razón de refinamiento < 0,35. Estos controles
**no sustituyen** el balance angular independiente de un nuevo coast ni la
presión temporal de los micrófonos. La puerta global sigue falsa.

No se guardan trayectorias, historias de onda ni otra R en disco. No se
generan audios ni paquetes y el plugin instalado no cambia.

## Resultado

Pasan los **nueve controles del preflight**. El extremo del kernel anterior
reproduce exactamente las cuatro medidas congeladas: energía mecánica,
relajación, norma de onda y compresión. El calor/trabajo acumulado cambia
como máximo 2,130458e−16 de la energía inicial al juntar los dos tramos.
Ambos kernels separan el martillo en [1,634115; 1,635417] ms.

| Dinámica medida | Referencia, s | Por lotes, s | Reducción |
| --- | ---: | ---: | ---: |
| Contacto continuo de 3 ms, 768 kHz | 288,554873 | 243,634189 | 15,5675% |
| Coast de 0,5 ms, 64 kHz | 4,086777 | 3,399781 | 16,8102% |
| Coast de 0,5 ms, 128 kHz | 8,040166 | 6,829927 | 15,0524% |
| Coast de 0,5 ms, 256 kHz | 15,512990 | 12,998721 | 16,2075% |

Durante el contacto, la máxima diferencia relativa de estados vivos es
**1,056664e−14**; inputs internos, **6,752609e−15**. En los coast, el máximo
de estado es 1,237015e−15. La diferencia de trabajo integrada más grande es
8,470329e−22 J. Energía mecánica y calor del fieltro del contacto coinciden
exactamente entre kernels. No se necesitó tangente de respaldo.

El refinamiento propio del coast compara posición/velocidad físicas de
tabla, sin recorte modal: 64 frente a 256 kHz cambia **0,00209233%**;
128 frente a 256, **0,000672160%**. Razón 0,3212495, dentro del gate < 0,35.
Se observa esa razón; no se afirma una reducción exacta por cuatro ni se
escoge todavía una tasa para audio. Los residuos de interfaz finales son
< 1,94e−14 N y < 4,17e−17 W; el fieltro está inactivo durante todo el coast.

Tiempo total **622,373 s** (10,373 min), pico propio **2,571152 GiB**.
La tabla mide sólo dinámica. Durante la ejecución se corrieron comprobaciones
breves de código/pruebas de micrófono, sin otro auditor pesado simultáneo;
no se presenta una medición de aislamiento estricto del hardware.
La etapa cubre hasta **3,5 ms** en su recorrido fino, no una nota audible.
El coste por paso todavía impide tiempo real y no se extrapola linealmente
la precisión ni los tiempos a un sostenido largo.

## Preparación de los micrófonos

**27d0705** sustituye el histograma denso de retardos por COO/CSR. Cada
punto de cuadratura conserva su celda, bin y peso con signo; se suman los
bins repetidos. Después se proyectan las mismas 606 formas físicas.
Permanece la opción `aggregation='reference'` para repetir el control.
Cinco pruebas pasan en 0,014 s, incluida equivalencia de ambas agregaciones
con órdenes de cuadratura 2/4/6 y el control del Green continuo.

[Ensayo de la tabla completa](medidas-P6-propagacion-coste/microfonos-completos.json):
49.152 celdas, orden 4, 786.432 puntos por micrófono; posiciones supuestas
(0,55; 0,5; 0,66) m y (0,85; 0,5; 0,66) m, iguales a las sondas del banco
armónico. Kernel de 445 taps a 128 kHz, **4.314.720 bytes**.

- Histograma denso: **174.981.120 bytes** (166,875 MiB).
- Scratch COO/CSR estimado conservador: **28.313.336 bytes** (27,002 MiB).
- Preparación densa **3,916595 s**; dispersa **0,597817 s**, unas **6,55 veces**.
- Diferencia relativa de kernel **8,613428e−16**; máximo absoluto 3,252607e−18.
- Error máximo contra el Green continuo en siete sondas entre DC y 6 kHz:
  **0,0114133%**, dentro del 0,02% original. El primer arribo es causal.

Pasan los cuatro controles del kernel. Tiempo total **7,276818 s**;
el pico del proceso incluye ambas implementaciones, no mide por separado
el pico de CSR. La menor reserva del scratch es un cálculo de arrays,
no un factor medido de reducción de todo el working set.

La primera ejecución conservada en
[metadatos previos](medidas-P6-propagacion-coste/microfonos-metadatos-previos.json)
ya usaba todas las celdas y pasó las comparaciones físicas, pero su cálculo
de scratch confundía la primera dimensión de shapes (256 filas) con las
49.152 celdas aplanadas. No se utiliza ese presupuesto. **f9e7817** corrige
el conteo y repite el control completo; no se altera el informe anterior ni
se cambia el kernel para aprobarlo. Fuentes y padres quedan autenticados.

Esto valida el **operador** de micrófono sobre la tabla supuesta; todavía
no mide presión de la trayectoria viva de 606, refinamiento de captura ni
un sostenido largo. Ambos informes generales mantienen `all_gates_passed=false`.

## Siguiente etapa

Cerrar almacenamiento y flujo angular independientes del coast completo;
comprobar presión temporal de las dos cápsulas y su refinamiento de captura.
Después extender el sostenido, la segunda pulsación y el apagador/release.
La reducción de coste de una contracción no demuestra tiempo real ni una
mejora del timbre. La cobertura sigue limitada a una cuerda y geometría
supuestas, sin identificación de un Steinway real.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1; sin bytecode, BLAS de un hilo.
Usar un destino nuevo para preservar el informe existente:

```powershell
python tools/audit-concert-grand-full-propagation-cost.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --contact-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-milisegundos/contacto-1p5ms-inputs.json --coast-duration .0005 --coast-rates 64000 128000 256000 --max-array-gib 8 --report NUEVO_COSTE.json
python tools/audit-concert-grand-full-microphone-kernel.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --max-array-gib 8 --report NUEVOS_MICROFONOS.json
```

Pruebas: `test_concert_pade_midpoint`, `test_concert_pade_contact`,
`test_concert_bounded_observer`, `test_concert_work_intervals` y
`test_concert_cell_audio` en `tools/tests`.
