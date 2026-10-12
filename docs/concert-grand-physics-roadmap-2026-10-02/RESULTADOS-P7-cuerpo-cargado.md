# P7 — Destilación del cuerpo cargado y de la presión

Fecha: 2026-10-12. Prototipos de identificación y ejecución fuera del plugin.
Esta etapa compara contra el **maestro físico asumido de P6**, con sus límites
de geometría, malla y tiempo todavía abiertos. Sus porcentajes no certifican
fidelidad al Steinway ni al instrumento polifónico completo.

## Maestro y condiciones conservadas

Se mide conjuntamente tabla, carga de aire causal y presión de los dos
micrófonos originales. Se mantienen los 606 modos estructurales y los 606
acústicos, el único `R.npy` de 1.909.627.328 bytes, Padé de orden 2 y el reloj
de 128 kHz del observador. No se aplica la reducción acústica de rango 192.
Los micrófonos permanecen en (0,55; 0,50; 0,66) y (0,85; 0,50; 0,66) m.

El cálculo por bloques reutiliza una vista de las correlaciones. Su contraste
contra el operador escalar original da error relativo máximo 4,85e−14 en
las tres sondas adicionales. No crea otra R ni historias modales en disco.
Los 1300 puntos tardan 240,41 s; el dataset compacto ocupa 13.892.359 bytes.
Informe: [maestro.json](medidas-P7-cuerpo-cargado-2026-10-12/maestro.json).

- Entrenamiento: 1189 frecuencias, 20–6000 Hz, más densas en las resonancias.
- Validación: 111 frecuencias distintas, 23–5970 Hz.
- Contraste externo: respuestas exactas a fuerzas en 11 puntos del teclado,
  frente a sus predicciones mediante el mapa espacial de 88→20 canales.
  Los canales de entrenamiento son combinaciones de fuerzas espaciales; no
  se afirma que las fuerzas puntuales sean estadísticamente independientes
  de todo su espacio. Las frecuencias sí están excluidas del entrenamiento.
- Presión en Pa y movilidad en m/s/N, con fase compleja y ganancia común.
  Sin normalizar cada nota ni introducir ganancias independientes de micrófono.
- Gate de cada operador: RMS ≤5 % y máximo relativo por frecuencia ≤10 %.
  El RMS integra las normas cuadráticas en frecuencia; no es la raíz del
  promedio de los porcentajes individuales.

La sola compresión geométrica, conservando el operador cargado completo, da
**1,2753 % RMS/8,5974 % máximo en movilidad**, y
**1,3644 % RMS/3,3047 % máximo en presión**. Es un control para distinguir la
pérdida geométrica de la pérdida al reducir la dinámica.

## Realizaciones mecánicas positivas

El primer banco suma residuos positivos de resonadores virtuales. Identifica
picos de la movilidad cargada y sus direcciones espaciales; los coeficientes
no representan cuerdas nuevas ni otro ajuste del fieltro. Una terminación
derivada de dos momentos conserva:

1. Flexibilidad estática: `Bᵀ K⁻¹ B`.
2. Inercia a alta frecuencia: `Bᵀ B`, con masas virtuales unitarias.

El banco independiente de 114 modos falla: **56,17 % RMS/264,50 % máximo** en
movilidad externa. La terminación añade modos dentro de la banda cuya pérdida
material sola no representa la carga radiativa. Suprimirlos o amortiguarlos
a ojo no valida la reducción. Se conserva el resultado rechazado.

Después se ajusta `C = diagonal positiva + L Lᵀ`, con ocho columnas en L.
La matriz sigue siendo simétrica y positiva. Con K, B y masas fijos conserva
ambos momentos; movilidad externa **7,7580 % RMS/22,4728 % máximo**.

La siguiente familia modifica K dentro del complemento del espacio de
flexibilidad estática. Para Ω diagonal, W ortonormal y `Wᵀ Ω⁻¹ B = 0`:

```
K = Ω [I + W (exp(-k) - I) Wᵀ] Ω
C = S C₀ S + L Lᵀ, S diagonal positiva
```

K y C permanecen positivos. El inverso de la matriz central de K cambia sólo
en W, por lo que `Bᵀ K⁻¹ B` se conserva algebraicamente. Los modos de la
terminación por encima de 30 kHz permanecen en el sistema y en cada solve;
su rigidez no se modifica. No se recortan polos para reparar estabilidad.

Con 114 modos: **5,8217 % RMS/19,2685 % máximo**. Falla el gate; además el
cambio modal global de una matriz con frecuencias de varios MHz introduce
un defecto numérico 2,32e−7, superior al límite local 1e−7 de equivalencia de
esa transformación. La flexibilidad final difiere 3,19e−8 y la inercia
1,14e−15. La próxima transformación debe diagonalizar por separado el bloque
de rigidez ajustado y las terminaciones rígidas que quedaron intactas.
El optimizador alcanzó 800 iteraciones; no se afirma convergencia.

## Observación causal de presión

Se ajustan coeficientes **reales** sobre las aceleraciones de los modos
virtuales, con cuatro grupos de retardos por micrófono. Los retardos están
dentro del soporte del kernel físico existente: 1,914–3,469 ms. Primero se
suman los modos de cada grupo y después se retarda la suma; el futuro
consumidor no necesita un buffer de audio por modo.

El ajuste usa las 1189 frecuencias de entrenamiento. Se comprueba la presión
compleja sobre las 111 reservadas y los 11 puntos de fuerza originales.
Regularización de Gram escalada 1e−6; los coeficientes conservan la escala de
Pa del maestro. La presión es un observador y no realimenta el puente.

Con pérdidas compartidas, el banco de 114 modos da **24,7422 % RMS/56,9907 %
máximo** en presión externa. Después de ajustar rigidez da **22,1920 %
RMS/48,1873 % máximo**. Ambos fallan. Una aproximación del puente mejor que
la presión no permite promover el modelo al plugin.

## Ampliación espacial y costo del ajuste

La sonda local en los 31 picos de entrenamiento muestra que dos direcciones
de la parte real de movilidad pueden dejar hasta 51,43 % de su norma y
87,34 % de la presión fuera de la proyección. Con cuatro direcciones esos
máximos bajan a 33,97 % y 62,32 %. Es un diagnóstico local; **no es una cota
inferior del error de un banco global con modos acoplados**.
Ver [direcciones.json](medidas-P7-cuerpo-cargado-2026-10-12/direcciones.json).

Se amplía el banco a cuatro direcciones por pico/ancla. El ajuste no negativo
retiene 141 modos más 20 de terminación: **161 modos**. Los momentos se
conservan; el banco independiente vuelve a fallar. Al ajustar pérdidas
compartidas pasa a **6,0535 % RMS/17,7893 % máximo en movilidad**, y
**14,8258 % RMS/30,9158 % máximo en presión**. La hipótesis espacial mejora
la presión, pero estos resultados siguen fuera del gate.

Duplicar los grupos de retardos a ocho sobre ese mismo modelo mejora sólo a
**13,7829 % RMS/29,4964 % máximo**. El control mantiene los estados y pérdidas;
no se atribuye todo el defecto de presión a la discretización de propagación.
Ver `presion-acoplado-4-retardos8.json`. La siguiente selección de estados debe
considerar también la respuesta a los micrófonos, no sólo picos de movilidad.

El ajuste de pérdidas usa ahora Woodbury para resolver una matriz del rango
de L. El gradiente se contrae directamente sobre los parámetros, sin formar
la matriz densa de gradientes de C. Se verifica contra solves/adjuntos densos:
defecto del objetivo 1,20e−16 y del gradiente 5,19e−16 en el caso de 161 modos,
348 frecuencias y 20 canales. Cinco mediciones alternadas, seis hilos BLAS:
mediana **0,7266 s → 0,1447 s**, mejora **5,023×**. Es costo de identificación
offline, no una aceleración del callback ni del render completo.
Ver [woodbury-coste.json](medidas-P7-cuerpo-cargado-2026-10-12/woodbury-coste.json).
Los adjuntos directos de rigidez/pérdidas también pasan contraste con los
adjuntos densos y sus familias positivas no cambian.

La batería de esta etapa y del núcleo seco reúne **21 pruebas**, todas pasan:
ODE independiente para paso/trabajo medio, decaimiento libre, reciprocidad,
polos reales sobreamortiguados, presión/propagación conocidas, momentos y
gradientes por diferencias finitas o contra el adjunto denso. No se hicieron
escuchas nuevas ni se confunde esa batería con validación del instrumento.

## Ejecución nativa del diagnóstico de 114 modos

Se reutiliza el núcleo Rust de residuos recíprocos, con paso exacto de fuerza
constante por frame y **velocidad media del intervalo** como puerto de trabajo.
La preparación, diagonalización, lectura y asignaciones suceden antes del
callback. El mismo mapa espacial se usa para fuerzas y velocidades.

| Tasa / frames | Plazo | Media | P99 | Máximo observado | Asignaciones |
| --- | ---: | ---: | ---: | ---: | ---: |
| 44,1 kHz / 256 | 5,805 ms | 0,6230 ms | 0,7736 ms | 0,9525 ms | 0 |
| 48 kHz / 256 | 5,333 ms | 0,6152 ms | 0,7648 ms | 0,9669 ms | 0 |

P99 consume 13,33 %/14,34 % del plazo: **pasa la reserva de 20 % del cuerpo**.
Se cotejan estados finales, checksum de salida y trabajo contra Python;
la energía paso a paso se calcula en su trayectoria reconstruida.
Informe: [runtime-114.json](medidas-P7-cuerpo-cargado-2026-10-12/runtime-114.json).
Este benchmark excluye observación de presión, cuerdas, contacto, pedales y
cierre de puertos. El modelo continúa siendo diagnóstico porque falla fidelidad.
Compilación específica de Ryzen 5 5600X; fallback portable/WASM sin medir.

## Estado y pendientes de esta etapa

El ajuste de rigidez/pérdidas del banco de 161 modos termina en 876,48 s,
800 iteraciones, sin afirmar convergencia del optimizador. Da **4,0738 % RMS/
13,0557 % máximo en movilidad**: pasa RMS y falla el máximo. La presión con
cuatro grupos da **13,7633 % RMS/28,6114 % máximo**, y también falla.
La equivalencia del cambio modal sí pasa en este caso (3,92e−8); los momentos
estático/de inercia difieren 3,51e−9/1,06e−15, con pérdida mínima positiva 6,192.
Informe: `almacenamiento-4-directo.json` y `presion-almacenamiento-4.json`.

La primera medición nativa de 161 modos coincidió parcialmente con el ajuste
offline de su observador de presión; se conserva como corrida con contención.
Se repitió el benchmark con **los mismos bytes** después de terminar los ajustes:

| Tasa / frames | P99 | Máximo observado | Fracción P99 del plazo | Asignaciones |
| --- | ---: | ---: | ---: | ---: |
| 44,1 kHz / 256 | 0,9581 ms | 1,2140 ms | 16,505 % | 0 |
| 48 kHz / 256 | 0,9554 ms | 1,2124 ms | 17,914 % | 0 |

Pasa la reserva de cuerpo del 20 % en ambas tasas. Se conservan cuatro controles
nativos/Python de los modelos de 114/161 modos. La repetición mide sólo costo,
con verificación de hashes; hereda los controles de estados/checksum/trabajo
de los mismos coeficientes. No incluye presión ni garantiza aislamiento del
CPU o del host. Ver `runtime-161.json` y `runtime-161-sin-ajuste.json`.

**Resultado: ya hay un cuerpo cargado reducido que entra en la reserva de CPU,
pero no un sustituto acústicamente validado del maestro.** El siguiente ensayo
debe seleccionar/ajustar estados considerando conjuntamente la movilidad y
la presión; la mejor movilidad sola dejó el micrófono todavía a 13,76 % RMS.
Después corresponde medir el costo de esa observación e integrar cuerdas.

Las fuentes medidas quedan congeladas y cada familia
conserva informes y hashes propios. `acoplado-8.json` registra un fallo de
orientación de puertos en el primer productor, antes de validar o guardar un
modelo; el productor v2 lo corrige. Ese intento no cuenta como resultado físico.
El cierre `verificacion.json` autentica fuentes, datos, la única R y los
informes; reproduce las seis movilidades y cinco observadores de presión
sobre las frecuencias reservadas y conserva los gates fallidos. No reproduce
otra vez el solver temporal de aire completo.

Para promover un cuerpo reducido faltan conjuntamente:

1. Gates de movilidad y presión, más equivalencia del cambio de coordenadas.
2. Validación temporal contra tabla/aire completos, fuerzas de ataque, cambio
   de fuerza, sostenido y release. El fit de armónicos no certifica transitorios.
3. Formato digital, retardos fraccionales y antialiasing a 44,1/48 kHz.
4. Costo de cuerpo **más observación**, seguido de cierre con cuerdas/contacto,
   pedales vivos, polifonía, buffers pequeños y WASM.
5. Escucha con eventos y ganancia iguales al maestro, antes de instalar.

No hay WAV nuevo, actualización de fábrica ni paquete nuevo. Se eliminaron
16 artefactos propios de compilación/coeficientes descartados de P7,
35.806.379 bytes; se conservan informes, fuentes, candidato seco, consumidor
necesario y dataset cargado. El manifiesto está sólo en el repo:
[limpieza.json](medidas-P7-cuerpo-realtime-2026-10-12/limpieza.json).
