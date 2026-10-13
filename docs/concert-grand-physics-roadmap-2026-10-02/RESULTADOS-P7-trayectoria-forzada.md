# P7 — Acoplamiento con trayectoria axial forzada

2026-10-13. Continuación de las [fuerzas con fases modales](RESULTADOS-P7-fases-cuadraticas.md).
Se implementa un solver experimental de cuerdas a puente fijo. **No se
selecciona para ejecución: falla CPU ampliamente.** Permite comprobar el
acoplamiento que la integral sobre portadores libres no incluía.

[Medición, fuentes y controles autenticados](medidas-P7-cuerdas-nativas-2026-10-13/phase-trajectory.json).
C4, tres cuerdas 96/192, 1.152 coordenadas de cuerda más la cabeza inmóvil,
estado mixto seed 83, ambos ejes, sostenido de **6 ms sin lanzamientos**.
La referencia comprueba contacto máximo y calor cero. No hay cuerpo,
presión ni pedales; el solver nuevo no simula la cabeza. No se genera audio.

## Referencia y control del motor anterior

Se usa el ejecutable monolítico con posición/velocidad coherentes y trabajo
local comprobado, autenticado contra los ocho fuentes congelados en Git
**6ffae8d9**. El mismo estado físico se integra a **8,192 y 16,384 MHz**,
variando sólo la tasa: el estado final cambia **0,1184666 %**. Ledgers
**1,046e−13 / 8,973e−13**; ambos pasan 1e−9. Esta referencia se valida
solamente para este estado, geometría y ventana, sin aprobación global.

La métrica es la norma relativa de `(omega q_cuerdas, v_todos)` en el estado
final. La posición fija de la cabeza no entra como desplazamiento de cuerda.
No es presión, audibilidad ni convergencia uniforme de toda la trayectoria.

Para reutilizar los kernels se exponen constructor, tamaño y gradiente
puntual de `Wire`. El acceso a frecuencias y gradiente cúbico se compila sólo
con `--cfg concert_phase_library`; allí se usa un único contador de
asignaciones. El solver nativo normal conserva sus ecuaciones y algoritmo.
Dos replays vivos, monolítico y ramp N16, reproducen **exactamente** q/v,
energía, calor, lanzamientos, ledger, fuerzas y gradiente de extremo del
ejecutable anterior. No se modifica la fábrica ni el paquete instalado.

## Primer candidato: energía cerrada, trayectoria rechazada

Se plantea un centro implícito en coordenadas transportadas por los
portadores lineales. La fuerza cuadrática se integra analíticamente y
la cúbica con Gauss temporal y el FFT espacial original. Una restricción
numérica de energía se acopla a la iteración, con dirección fija desde el
estado inicial transportado, sin acumulación dependiente del camino.

El control `--no-axial-waveform`, a 128 kHz/N16, cierra ledger
**9,226e−14**, pero el estado difiere **98,4255 %** de la referencia fina.
**Rechazado**. La representación libre no incluye el desplazamiento axial
forzado dentro del bloque. No aceptar un método por energía cerrada.

## Trayectoria axial dentro del bloque

Se conserva el potencial SI original:

`V_NL = EA integral [u_x eta + eta²/2] dx`, `eta = (v_x²+w_x²)/2`.

El gradiente axial sólo depende de las coordenadas transversales. Para un
portador transversal dado, su fuerza tiene frecuencias firmadas
`nu = omega_k ± omega_l`. Se calcula la respuesta axial con condiciones
iniciales cero `P_U(t)` para **todos los modos**, y se incorpora dentro
de la evaluación transversal, en lugar de usar sólo su valor final.

Para `omega_U > 4 max(omega_T)`, la parte particular de cada armónico es
`-g_nu(t)/(omega_U²-nu²)`. Sus términos homogéneos se incorporan al kernel
analítico, restando posición y velocidad particulares iniciales. Como
`|nu| <= 2 max(omega_T)`, el denominador supera `3 omega_U²/4`.
Los modos restantes, incluidas resonancias cercanas, usan la convolución
con `sinc` sin dividir por una diferencia de frecuencias.

**Sólo se elige una fórmula de evaluación**, sin eliminar modos ni suponer
equilibrio axial estático. En C4 son 134 modos axiales por cuerda en la rama
particular/homogénea y 58 en la rama de convolución directa. q/v siguen vivos
en las 192 coordenadas axiales. Cinco puntos por bloque reconstruyen la
respuesta forzada contra integrales Duhamel independientes, a 4 y 96 modos.

Con `R(t)` como transportador libre, la trayectoria interna propuesta es:

- Transversal: `x_T(t) = R_T(t) c_T`.
- Axial: `x_U(t) = R_U(t) c_U + P_U(t; c_T)`.
- Extremo: `z1 = R(h) z0 + K(c) - lambda d`.
- Centro transversal: `c_T = [zT0 + R_T(-h) zT1]/2`.
- Centro axial: `c_U = [zU0 + R_U(-h)(zU1-P_U(h))]/2`.

`K` integra el gradiente completo sobre esa trayectoria interna. El kernel
analítico trata los portadores rápidos; Gauss integra la parte forzada
restante y la fuerza cúbica con los kernels espaciales originales.
Las condiciones de centro y la restricción de energía convergen juntas.
**La respuesta transversal forzada dentro del intervalo todavía no está
representada**: el modelo interno transversal sigue siendo un portador libre.

La dirección numérica es `d_q = q_free + g_NL(q_free)/omega²`,
`d_v = v_free`, fija por paso. Se resuelve `E(z1)=E(z0)` con tolerancia local
1e−13 y residual de centro 1e−11; máximo 32 iteraciones. La corrección
no es pérdida física ni calor. Se guarda el defecto antes de corregir y
`|lambda|`. Ningún q/v físico se publica si falla la convergencia.

## Precisión: mejora, pero no alcanza a 128 kHz

| Tasa / bloque | Orden Gauss | Error final frente a 16,384 MHz | Ledger máximo | Resultado estado/trabajo |
|---|---:|---:|---:|---|
| 128 kHz / N16 | 96 | 6,46053 % | 6,704e−14 | Falla estado |
| 128 kHz / N8 | 64 | 5,82791 % | 1,045e−14 | Falla estado |
| 128 kHz / N4 | 40 | 5,03756 % | 1,127e−13 | Falla estado |
| 128 kHz / N1 | 24 | 3,08537 % | 6,479e−13 | Falla estado |
| 256 kHz / N1 | 16 | 2,66823 % | 2,132e−12 | Falla estado |
| 512 kHz / N1 | 12 | 2,59903 % | 9,707e−12 | Falla estado |
| 1,024 MHz / N1 | 8 | 1,26911 % | 1,106e−13 | Falla estado |
| 2,048 MHz / N1 | 8 | **0,638734 %** | **9,283e−11** | Pasa sólo este ensayo |

A 128 kHz/N16, incluir la respuesta axial forzada baja el error
**98,4255 → 6,46053 %**, conservando el rechazo temporal. Sin restricción
numérica, el estado cambia poco (**6,46044 %**), pero el ledger falla
**9,761e−5**. La corrección no arregla el fallo de trayectoria.
El defecto local previo a corregir baja de **8,236e−4** en el control libre
a **5,156e−5** con la respuesta axial forzada; `|lambda|` máximo baja
**4,127e−4 → 2,583e−5**. A 2,048 MHz queda **5,787e−7**.

El único caso que pasa estado/trabajo requiere 16 veces el reloj de 128 kHz,
con cuatro iteraciones máximas. No aprobarlo a un reloj inferior.
Gauss 8/16 a 2,048 MHz, en una sonda de 125 microsegundos del mismo estado,
cambia **2,008e−15**. Esta sonda comprueba la cuadratura; no amplía la
validación temporal de 6 ms ni demuestra exactitud continua del solver.

## CPU: rechazado como ruta de ejecución

La [auditoría independiente por cuerda](medidas-P7-cuerdas-nativas-2026-10-13/phase-trajectory-wire-audit.json)
detecta además una limitación de la restricción compartida. En este ensayo
sin contacto ni cuerpo, cada cuerda debe conservar su propia energía.
A 2,048 MHz, los errores finales por cuerda son **4,916e−7 / 4,500e−7 /
8,333e−8**: fallan 1e−9 aunque el ledger total pase. La referencia fina
pasa por cuerda, máximo **1,961e−12**. Se conserva el resultado nominal de
estado/ledger total del cuadro anterior, pero **no se aprueba conservación
local**. La corrección compartida permite transferencias numéricas entre
cuerdas desacopladas; debe reemplazarse por restricciones independientes.

Ocho buffers de **256 microframes a 2,048 MHz**, duración física total **1 ms**,
desde el mismo estado inicial mixto; se avanza dinámica y se comprueba ledger.
Presupuesto por buffer **0,125 ms**. Media **1.010,912 ms**, P99/máximo
**1.033,978 ms**, cero asignaciones. Falla ampliamente CPU, incluso sin
contacto/cuerpo/presión. No convertir esta medida en una estimación de
render de La Campanella ni de un instrumento integrado.

Tablas contabilizadas: **16,312 MB** a 2,048 MHz/N1/orden8, o **95,791 MB**
a 128 kHz/N16/orden96. No es memoria total del proceso: quedan fuera
estructuras, buffers de estado y parte de los kernels nativos.
La tabla temporal de respuestas por pareja es una herramienta de control;
su contracción actual no sirve para ejecución. No sustituye el núcleo rápido
anterior ni sus gates, y no se integra en el plugin.

## Validación y continuación

### Restricciones independientes por cuerda

El candidato compartido y su rechazo local quedan congelados en **164f3481**.
La [medición de las restricciones independientes](medidas-P7-cuerdas-nativas-2026-10-13/phase-trajectory-wire-roots.json)
autentica ese snapshot y reutiliza sus referencias finas, sin volver a
integrarlas ni copiar sus datos. `--shared-energy-root` conserva el control
anterior y reproduce exactamente sus arrays, ledger y lambda.

El solver experimental usa ahora tres raíces `lambda_i`: cada dirección
está restringida a su propia cuerda y cada raíz impone `Ei(z1)=Ei(z0)`.
Las raíces se acoplan a las condiciones de centro, con la misma tolerancia
local y publicación atómica de q/v. No se agrega intercambio entre cuerdas.
**Esta condición corresponde al puente fijo y ausencia de contacto**; un
futuro martillo/cuerpo acoplado debe incorporar su trabajo, pérdidas y
transferencias físicas, no imponer energía individual constante.

A 2,048 MHz/N1/orden8, en los mismos 6 ms:

| Control | Error final de estado | Error final de energía por cuerda | Resultado local |
|---|---:|---:|---|
| Raíz compartida | 0,638734 % | 4,916e−7 / 4,500e−7 / 8,333e−8 | Falla 1e−9 |
| Raíces independientes | **0,638741 %** | **1,851e−10 / 4,126e−11 / 1,228e−10** | Pasa 1e−9 |

La evaluación energética es independiente, mediante matrices espaciales
densas, el mismo potencial SI y la energía cinética/modal original. El caso
independiente pasa estado, ledger total y energía final por cuerda sólo en
esta ventana. No amplía aprobación temporal, de contacto o del instrumento.

CPU vuelve a fallar: ocho buffers de 0,125 ms, media **1.039,585 ms**,
P99/máximo **1.076,166 ms**, cero asignaciones. El benchmark avanza 1 ms
físico y también pasa energía por cuerda, máximo **3,206e−11**. La corrección
local arregla un invariante; **no es una optimización de tiempo del motor**.
Tablas contabilizadas siguen en 16,312 MB y cuatro iteraciones máximas.

Pasan las **siete pruebas actuales** del prototipo, con dos regresiones
nuevas: el cierre individual que la raíz compartida viola y una cuerda
vacía que permanece vacía mientras las otras vibran. Las 32 pruebas
anteriores pasaron en esta etapa; la suite acumulada contiene **39**.

**37 pruebas pasan**: cinco nuevas de gradiente cúbico independiente,
reconstrucción axial Duhamel, portadores libres sin corrección, estado no
publicado al fallar y refinamiento contra ODE continuo de cuatro modos;
las 32 anteriores también pasan. Fuentes, binarios, entradas y resultados
se autentican en el informe. Los archivos grandes permanecen en `local/`.

Sigue representar también la respuesta transversal forzada y factorizar
las contracciones temporales/espaciales. Hay que comprobar si eso permite
volver a un reloj menor y reducir el costo antes de martillo, inercia
compartida con el cuerpo, presión, pedales y polifonía. Conservación de
energía, convergencia temporal y CPU mantienen gates separados. Global falso.

### Contracción agrupada de la respuesta axial

La [medición de esta optimización](medidas-P7-cuerdas-nativas-2026-10-13/phase-axial-waveform.json)
autentica fuentes, ejecutable, entrada y resultado del control **63be531a**.
Reutiliza el par de referencias finas anterior. Se conserva el mismo ensayo:
C4 mixto, tres cuerdas 96/192, puente fijo, sin martillo, 2,048 MHz/N1/orden8,
6 ms de evolución completa y tres restricciones independientes de energía.

La hipótesis es reducir las operaciones repetidas en `AxialWaveform`. Las
amplitudes `v/omega` se calculan una vez por modo transversal y se reutilizan
en todas las tríadas. La inicialización particular y cada muestra temporal
acumulan sus cuatro productos en registros AVX2; sólo se suman los canales
al completar un modo axial. El guard de CPU preparado selecciona esa ruta.
Todos los índices conservan validación de límites; las cargas admiten la
alineación original. No se altera la cuadratura, las frecuencias, la precisión
f64, los modos, las raíces energéticas ni el solver de centro.

`--scalar-axial-waveform` conserva la contracción escalar y sólo comparte el
cache de amplitudes; reproduce **exactamente** q/v, ledger total y lambda del
control congelado en los 6 ms. `--portable` fuerza también la forma de onda
escalar además del kernel cuadrático de fases. La trayectoria agrupada
difiere del control en **3,778e−13** en norma de estado ponderada por omega.
Frente a la referencia de 16,384 MHz mantiene **0,638741 %**; la auditoría
espacial independiente pasa energía por cuerda, máximo **1,858e−10** frente
al gate 1e−9. El ensayo pasa estado y energía dentro de esta ventana.

Los benchmarks avanzan el mismo estado durante ocho buffers de 256 microframes
(1 ms físico total), secuencialmente y sin pruebas simultáneas:

| Ruta | Media por buffer | P99/máximo | Asignaciones |
|---|---:|---:|---:|
| Ejecutable congelado 63be531a | 1.049,362 ms | 1.117,820 ms | 0 |
| Amplitudes cacheadas, contracción escalar | 1.026,789 ms | 1.113,070 ms | 0 |
| Contracción axial agrupada | **618,379 ms** | **642,725 ms** | **0** |
| Repetición agrupada | **617,901 ms** | **649,783 ms** | **0** |

La media mejora **1,697× / 41,07 %** frente al control medido en esta sesión;
el cache aislado aporta ~2,15 %. Cada benchmark también pasa energía por
cuerda y ledger total. Las tablas y buffers contabilizados aumentan de
16.311.816 a **16.321.032 bytes** por el cache de amplitudes preasignado.
No hay asignaciones al avanzar ni reducción del máximo de cuatro iteraciones.
El plazo sigue siendo **0,125 ms**: los dos máximos agrupados lo superan
más de 5.100 veces. **CPU continúa rechazado**, sin instrumento integrado.

Pasan las **14 pruebas actuales** de fases y trayectoria. La nueva regresión
compara SIMD, contracción axial escalar y kernel de fases escalar en cuatro
y 96 modos, a ambos relojes, y comprueba la reconstrucción en cinco tiempos,
incluido cero. Las pruebas independientes de integral Duhamel, energía por
cuerda, publicación atómica y convergencia contra ODE siguen pasando.
Las 26 pruebas del núcleo nativo/presión pasaron en la etapa anterior;
esos algoritmos no cambian aquí. La suite acumulada contiene 40 pruebas.

Esta aceleración confirma el costo de las contracciones repetidas. Sigue
factorizar la dependencia temporal y representar la respuesta transversal
interna para reducir también el reloj necesario, con estado y energía como
controles. No extiende la validación de duración, contacto, cuerpo, presión,
pedales o polifonía. No se modifica la fábrica ni se instala el prototipo.
