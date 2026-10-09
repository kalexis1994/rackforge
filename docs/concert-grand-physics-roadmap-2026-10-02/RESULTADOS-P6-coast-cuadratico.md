# P6 — Aislar el error dinámico del sostenido

Fecha: 2026-10-09. Continúa la
[atribución de bandas nativas](RESULTADOS-P6-bandas-nativas.md).
La etapa anterior identifica diferencias de dinámica y reconstrucción:
coast 16 kHz falla; 128 kHz pasa sólo un tramo de 8 ms y cuesta 6,42×.
No se modifica el fieltro, las pérdidas o la afinación para compensar
esas diferencias numéricas. El diagnóstico previo quedó en `e15f0f6`.

## Hipótesis y controles

Se reutiliza el único estado completo C4/84 al terminar 2 ms de contacto
midpoint 3072 kHz. Se verifica su SHA256, fuentes y evidencia de origen;
no se vuelve a calcular el ataque ni se reinician cuerdas o aire. Banco
de C4/G4/C5, nueve cuerdas, 606 coordenadas de tabla, aire de rango 192,
todos los apagadores abiertos. Las trayectorias duran 8 ms adicionales.

- **Modelo completo a 16 kHz:** debe reproducir la presión previamente
  medida, con error relativo menor que 1e-10, como control de continuidad.
- **Control cuadrático:** desactiva sólo el residuo geométrico no lineal
  de las cuerdas. Conserva matrices de masa/rigidez, puertos, fieltro,
  coordenadas, cabezas y estado inicial; su energía al cambiar de potencial
  se registra como cambio intencional de la ablación, no como calor.
  Tasas 16/32/64/128/512 kHz. No es un candidato de piano.
- **Control cuadrático con aire subdividido:** macro 16 kHz con ocho
  subpasos de aire, y macro 32 kHz con cuatro; el aire evoluciona a 128 kHz
  mientras las dos mitades mecánicas mantienen el paso macro. Conserva las
  ecuaciones de esa ablación. Permite medir el error interno del subflujo
  del aire frente al error de composición mecánica–aire; no elimina
  automáticamente el segundo.
- **Mecánica cuadrática sola:** conserva todas las portadoras exactas,
  omite el subflujo completo de aire/pérdidas de tabla y congela los estados
  acústicos. Contraste 16/512 kHz. Sólo diagnóstico del flujo mecánico y
  de su fuente reconstruida; no es un piano ni una escucha sin resonancia.

Se compara presión nativa y también la misma captura gruesa de la referencia
fina. La segunda comparación separa diferencias dinámicas de la imagen
creada por Hermite. Ventanas rectangular/Hann de aceleración física y el
mismo micrófono congelado a 128 kHz, sin corregir ganancia o fase.

Grilla inicial 6,25 Hz contra 12,5 Hz, con refinamiento adicional en RAM si
no pasan los controles ya fijados. Se mantienen los límites: bandas 1 %,
norma entre grillas 0,1 %, variación del error 0,0001 absoluto, oráculo
Hermite 1e-10. El balance mecánico cuadrático mide
`H_final-H_inicial+calor_material+trabajo_del_puerto`, límite 1e-9 relativo
a H inicial. No incluye un certificado de energía/radiación total del aire.

## Pruebas y evidencia

Dos pruebas nuevas pasan: copia de control sin alterar el modelo original
y evolución cuadrática exacta sin aire; equivalencia de la subdivisión
contra la composición explícita, conservación del estado de entrada y
balance mecánico de trabajo/calor en pasos sucesivos. Los paquetes de
entrada al aire se conservan por subpaso; nunca se suman como si fueran
una única muestra de radiación.

Fuentes: `tools/concert_grand_coast_controls.py` y
`tools/measure-concert-grand-coast-controls.py`. Evidencia única en el repo:
`medidas-P6-coast-cuadratico-2026-10-09/controles.json` y `presion.npz`.
No se crean WAVs ni otra R o copia del estado en disco. El plugin conserva
0.172.0; estos controles pertenecen al prototipo físico offline.

## Resultados y decisión

Ensayo completado y verificado en **298,609 s (4,98 min)**: diez recorridos
y catorce comparaciones, con grilla de 1913 puntos a 6,25 Hz. Pasan todos
los controles de medición: norma entre grillas máxima 0,058509 %, variación
del error máxima 0,000054951 absoluto y oráculo máximo 8,292e-12.
Los nueve balances cuadráticos pasan: error máximo 4,278e-13 relativo.
El replay completo de 16 kHz reproduce exactamente la presión anterior
en rectangular y Hann. El estado de entrada y los hashes quedan intactos.

La primera ejecución completó los diez recorridos y falló después por un
nombre inconsistente de la referencia cuadrática, antes de guardar los
espectros. Sus diez estadísticas quedan en `controles-fallido.json`, con
el cambio mínimo que reconstruye y autentica el productor anterior por
SHA256. Se corrigió el nombre y se repitió el ensayo desde el mismo estado;
no se alteraron las ecuaciones ni los límites. Ese informe parcial no
certifica presión ni sustituye la ejecución completada.

### Residuo no lineal y memoria acústica

Errores porcentuales respecto a la referencia cuadrática de 512 kHz,
desde la misma historia física completa. La ablación cambia el potencial
inicial en +2,249394e-7 J: se registra como intervención deliberada.

| Coast cuadrático | Banda 6–12 kHz Hann | Estado mecánico | Onda | Relajación |
| --- | ---: | ---: | ---: | ---: |
| 16 kHz | **63,667780** | 0,066477 | **7,056364** | **1,001340** |
| 32 kHz | **4,032541** | 0,016862 | **1,786355** | 0,243425 |
| 64 kHz | **1,674689** | 0,003870 | 0,442174 | 0,059091 |
| 128 kHz | 0,456273 | 0,000936 | 0,105282 | 0,014059 |

El control físico completo de 16 kHz, contra su propia referencia física
de 512 kHz, cambiaba 1,265874 % en mecánica y 7,054417 % en onda. Quitar
el residuo reduce mucho la diferencia mecánica, pero el error de onda
persiste prácticamente igual. El residuo importa para la precisión
mecánica; no explica por sí solo el error de memoria acústica/campo.
Los dos controles usan referencias distintas de acuerdo con su potencial.

### Refinar sólo el subflujo del aire no resuelve la composición

Errores porcentuales frente a la referencia cuadrática de 512 kHz.

| Mecánica / aire | 6–12 kHz Hann nativo | Campo con misma captura gruesa | Onda | Relajación |
| --- | ---: | ---: | ---: | ---: |
| 16 / 128 kHz (8 subpasos) | **64,424050** | **50,052445** | **10,303069** | **9,030088** |
| 32 / 128 kHz (4 subpasos) | **3,134620** | **3,043969** | **6,750717** | **4,116492** |

La segunda columna de error de campo usa como denominador la referencia
fina capturada a la tasa mecánica, y no su representación nativa. No
sumar esos porcentajes. El aire subdividido mejora algunas bandas de
presión, pero empeora onda/relajación respecto al coast cuadrático sin
subdividir: 7,056→10,303 % y 1,786→6,751 % en onda. La conservación de
trabajo no garantiza la trayectoria correcta de la memoria acústica.
Ambos controles se rechazan como solución. Sus costes de 20,636 / 21,258 s
por 8 ms tampoco permiten declarar un ahorro validado a igual precisión.

### Portadoras y reconstrucción de fuente

Sin el subflujo de aire/material, las portadoras cuadráticas 16/512 kHz
coinciden en el extremo: error mecánico 2,656e-12 relativo; estados de
aire idénticos. Aun así, el campo nativo cambia **18,154728 % en 6–12 kHz
Hann** por su reconstrucción Hermite. Al usar la misma captura gruesa de
la referencia fina, la diferencia de campo prácticamente desaparece.
La fuente reconstruida es un error separado; pasar extremos o energía
no certifica presión. No se adopta la ablación sin aire.

### Verificación y siguiente intervención

Verificados independientemente: hashes de fuentes, estado, datos y reporte
de origen; vectores finitos y normas de las catorce comparaciones;
controles de grilla, reparto de energía y gates conservados. Un archivo
de presión de 1.779.012 bytes guarda los espectros del ensayo. No queda
un trabajador activo ni se genera una nueva escucha.

**Decisión:** conservar los resultados de las ablaciones como diagnóstico,
mantener toda la no linealidad en el candidato físico y rechazar refinar
sólo el aire. La intervención siguiente prueba un puerto acoplado con
promedio de desplazamiento y portadoras exactas, para compartir el mismo
trabajo mecánico/acústico sin el splitting. Su ensayo y resultado se
documentan en [puerto acoplado](RESULTADOS-P6-puerto-acoplado.md).
La precisión acústica, convergencia de duración/release y selección global
siguen pendientes; ninguna ablación actualiza fábrica.
