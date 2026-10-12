# P7 — Destilación del modelo hacia tiempo real

Fecha: 2026-10-12. Motivo MIDI anterior guardado en **f0190e32**.

## Qué se implementó

Un primer componente nativo de **tabla estructural compartida**, derivado de
las mismas 606 coordenadas, geometría de puertos y matriz completa de pérdidas
materiales del modelo de referencia. Todavía es la tabla **sin carga acústica**:
no es un piano completo ni produce presión de micrófono por sí solo.

La referencia lenta queda como maestro medible; el estudiante recibe fuerzas
y devuelve velocidades por los mismos puertos. No se entrenó una red ni se
mezclaron notas muestreadas. Preparación, reducción, exponenciales y lectura de
coeficientes ocurren fuera del procesamiento de audio. El núcleo Rust mantiene
estado entre buffers y no realiza asignaciones, factorizaciones, E/S o bloqueos
durante el callback medido. El contador de asignaciones registra **0**.

Se estudió reducción que conserva estructura física, como la descrita por
[Gugercin et al., interpolación de sistemas port-Hamiltonianos](https://arxiv.org/abs/1101.3485).
La aplicación aquí es un componente mecánico lineal y recíproco; ese artículo
no certifica nuestro piano no lineal o su implementación completa.

## Construcción y balance de energía

En coordenadas modales de masa unitaria, con `Omega=diag(2*pi*f)`, se usa
`x=(Omega*q,v)`, `E=x^T*x/2` y

```
xdot = A*x + B*F
A = [0, Omega; -Omega, -D]
B = [0; Phi]
y = B^T*x
Edot = F^T*y - v^T*D*v
```

`D` conserva los términos cruzados. Una proyección ortonormal de energía
conserva pasividad, pero una POD que mezcla arbitrariamente posición y
velocidad no garantiza reciprocidad. Las primeras reducciones usan una base
espacial `U` y `V=diag(orth(Omega*U),U)`, conservando rigidez positiva,
pérdidas proyectadas y fuerza/velocidad colocalizadas.

La implementación final diagnóstica conserva **todos los 606 modos** y
comprime sólo la geometría de los 88 puertos mediante una matriz `P`:
`F_lat=P^T*F`, `y=P*y_lat`. Así, `F^T*y=F_lat^T*y_lat`; la misma proyección
debe aparecer en entrada y salida. Hay una sola tabla compartida.

Las ecuaciones lineales se realizan en polos amortiguados, sin convertir
`D` en una diagonal ni ajustar decaimientos independientes. La normalización
de residuos recíprocos permite compartir la fila espacial de entrada/salida
de cada polo. Se comprueba su equivalencia compleja antes de exportar.

El avance es exacto para fuerza constante por muestra. La salida de puerto
es la **velocidad media del intervalo**, no su valor final: el trabajo de
entrada es `dt*F^T*y_media`. Los polos reales sobreamortiguados conservan sus
signos y no se duplican como pares conjugados. No se recortan polos para
ocultar inestabilidad. La energía se evalúa reconstruyendo las coordenadas
mecánicas; la norma de los estados complejos no es por sí sola energía.

## Validación y candidato diagnóstico

Construcción: 14 alturas y frecuencias de entrenamiento declaradas en los
informes. Validación: otros 11 puertos de fuerza y **111 frecuencias entre
23 y 5970 Hz**. Se observan los 88 puntos durante la construcción de la base:
las alturas de entrada y frecuencias de validación no fueron excitadas en
entrenamiento. Esta validación se usa para seleccionar tamaño; no constituye
una prueba auditiva final independiente ni una garantía entre todas las sondas.

El error usa la **matriz compleja de movilidad**, incluyendo ganancia y fase.
Límites prefijados: RMS ponderado por frecuencia **5 %**, peor sonda **10 %**.
Los valores comparan transferencias continuas. La equivalencia del runtime
se comprueba por separado para fuerza retenida a 44,1/48 kHz: no se presenta
ese score como un error de WAV/presión frente al maestro a 128 kHz.

Se conserva un candidato de **20 canales espaciales** y todos los modos:

| Medida | Resultado |
| --- | ---: |
| Error RMS de movilidad compleja | **1,091826 %** |
| Peor frecuencia de validación | **8,526689 %** |
| Puertos externos / canales internos | 88 / 20 |
| Coordenadas estructurales / estados de energía | 606 / 1212 |
| Coeficientes por tasa | 237576 bytes |
| Mapa de puertos | 14096 bytes |
| Modos mecánicos eliminados | 0 |

Medición nativa, un hilo, AMD Ryzen 5 5600X, build `target-cpu=native`:

| Tasa, buffer 256 | Plazo | Media | P99 | Máximo observado |
| --- | ---: | ---: | ---: | ---: |
| 44,1 kHz | 5,804989 ms | 2,293294 ms | **2,5914 ms** | 3,3438 ms |
| 48 kHz | 5,333333 ms | 2,263592 ms | **2,5686 ms** | 3,3289 ms |

Cada medición usa 2048 buffers, tras 32 de calentamiento, con todos los puntos
de fuerza y salida incluidos. El maestro dry-body condensado entra **solo**
en esos plazos observados. Sin embargo, falla la reserva elegida de **20 %**
del plazo para la tabla: P99 consume **44,64/48,16 %**. Se conserva como
diagnóstico para integrar y estudiar; **no se cambió el gate para aprobarlo**.
No es una medición del host ASIO ni garantía de plazo bajo otras cargas.

Ocho pruebas unitarias pasan: integración/puerto frente a una ODE independiente,
pasividad con entrada y en decaimiento libre, reciprocidad, pérdidas cruzadas,
normalización de residuos y polos reales. Hay **72 controles nativos/Python**
de las variantes. Se cotejan estado final, checksum de salida y trabajo; la
energía paso a paso se comprueba en la trayectoria Python reconstruida. No se
afirma una reproducción independiente de cada estado nativo o del piano entero.
El cierre independiente autentica fuentes/tabla/informes/coeficientes y
recalcula los gates; conserva explícitamente el fallo de reserva de CPU.

## Experimentos descartados, sin ocultar sus fallos

Todos los informes quedan en `medidas-P7-cuerpo-realtime-2026-10-12`.

| Informe | Conclusión |
| --- | --- |
| `destilacion.json` | Proyección de estados 16–128: matrices densas. 16 entra en reserva, pero error RMS 39,19 %; 128 mejora a 14,52 % y excede el plazo. |
| `modal.json` | Realización modal equivalente acelera la ejecución; reducir estados sigue perdiendo respuesta. Sin reducción, error numérico 1,08e−13, costo superior al plazo. |
| `espacial.json` | Conservar modos y comprimir puertos: 24 canales da 0,331535 % RMS y 2,302956 % máximo; aún cuesta demasiado para la reserva. |
| `estatico.json` | Modos dominantes más 24 coordenadas que preservan flexibilidad estática. La identidad estática pasa; la respuesta dinámica no cumple fidelidad y reserva simultáneamente. |
| `reciproco.json` | Fila compartida de residuos y FMA. 20 canales cumple fidelidad y plazo aislado; falla reserva 20 %. Es el candidato conservado. |
| `vector.json` | Reorganizar por modos no mejoró costo en esta CPU; mantiene la equivalencia. |
| `avx2.json` | Fase AVX2 explícita produce mejora limitada; reserva todavía falla. |
| `ilp.json` | Varias cadenas de suma independientes tampoco mejoraron el resultado; no repetir esta hipótesis sin otro perfil medido. |

Se descartaron los coeficientes de pruebas; sólo se conserva el candidato
diagnóstico reproducido **bit a bit** respecto al informe `reciproco.json`.
`candidato.json` registra sus rutas/hashes y el gate fallido. No hay nuevas R,
historia modal en disco, WAVs ni informes duplicados en SHARED. No se modificó
la calibración o el plugin instalado. Los resultados anteriores de acústica
(9,2021 %), mallas, Hann/tiempo y la malla espectral siguen pendientes.

## Qué falta para el piano realtime

1. **Tabla con aire causal y presión.** La siguiente reducción debe representar
   conjuntamente tabla/carga, medir movilidad recíproca y presión de ambos
   micrófonos, con ganancia fija y sondas reservadas. Un buen score de tabla
   seca no permite reemplazar el cuerpo cargado del piano. Resolver también
   propagación, formato digital y antialiasing: la tabla contiene 93 modos
   sobre 22050 Hz y llega a 486332 Hz; conservar polos no valida su audibilidad
   ni la presión muestreada. Reducir contra la respuesta cargada pertinente
   antes de más optimizaciones pequeñas del mismo núcleo seco.
2. **Cuerdas y puente.** Estados vivos de los tres unísonos y dos polarizaciones,
   dispersión, pérdidas y componente longitudinal. Convertir el operador de
   referencia a propagación/filtros o resonadores económicos, con mediciones
   reservadas; conservar realimentación recíproca de la tabla.
3. **Contacto no lineal.** Mantener la ley del fieltro y un contacto de tamaño
   pequeño con energía/pérdidas controladas; comparar ataques suaves/fuertes,
   retorno y reiteración. Preparar coeficientes para los subpasos necesarios
   fuera del callback. No resolver la matriz mecánica global durante el ataque.
4. **Pedales/apagadores.** CC64/67 continuo, estados persistentes y trabajo de
   cambios de parámetros; note-off y reiteración no reinician cuerdas o caja.
   La acción completa de martillo y registro sin apagadores siguen pendientes.
5. **Integración y escucha.** Cierre de puertos instantáneos, eventos dentro del
   buffer y salida física de micrófonos. Contraste musical contra el maestro
   congelado, misma ganancia, sin normalizar notas ni sustituirlas por samples.
6. **Presupuesto total.** Medir 44,1 kHz/256, polifonía/pedal/reiteración, cambios
   de tasa, bloque pequeño y callback del host; fallback portable y WASM.
   Después de esas pruebas y la escucha, integrar/promover en RF-Concert Grand.

El próximo ensayo debe atacar **tabla + aire + observación**, con presupuesto
de cuerpo explícito. Esta etapa ya establece una interfaz nativa conservativa
y medidas de costo/fidelidad; todavía no entrega un instrumento completo.
