# P6 — Carga racional con certificado de energía

2026-10-03–04. La etapa anterior quedó en **77f00db**, por pedido del usuario.
**Ninguna variante se selecciona para fábrica.** Hay una representación
compacta que pasa frecuencia y elimina la recurrencia tardía anterior, pero
todavía falla la fuerza transitoria. Paquete instalado: 0.172.0.

## Construcción y límites físicos

Se conserva la superficie P2 de 35 modos, normalizada por masa física,
con una cara en bafle. Cada celda rectangular aporta su distribución exacta
de retardos proyectados en una dirección angular; se integra en intervalos
temporales. Una diferencia temporal genera el factor de radiación H.
Las direcciones opuestas se construyen por inversión temporal exacta:
sus autocorrelaciones sumadas son recíprocas. El binning, la derivada discreta,
la integración angular y el cambio de frecuencia bilineal introducen error.

Con R_k = Σ H_lᵀ H_{l+k}, se construye
Z_d(z) = R_0/2 + Σ R_k z⁻ᵏ. Su parte hermítica sobre el círculo es
H(z)ᴴH(z)/2 ≥ 0. H(1)=0 y la reciprocidad dan Z_d(1)=0.
La transformación z⁻¹=(a−s)/(a+s), a=2·factor_rate, produce una impedancia
racional continua pasiva, con polos repetidos estables en −a.
No es Vector Fitting ni una implementación del código de SINTEF.
La conservación de la propiedad positiva real por la transformación bilineal
está descrita en [Smith, Physical Audio Signal Processing](https://www.dsprelated.com/freebooks/pasp/Digitizing_Bridge_Reflectance.html).

Las variables continuas son r'_j=a(u_{j−1}−r_j), u_j=2r_j−u_{j−1}.
No son posiciones/velocidades de osciladores del baño anterior. Para un estado
r, los futuros frentes libres son t_j=Σ_{d≥1} H_{j+d} r_d. Su almacenamiento
E = Σ ||t_j||²/(2·factor_rate) es no negativo. Se verifica la identidad
de trabajo/almacenamiento/radiación con midpoint a distintos pasos, también
con estados vivos arbitrarios. La transformación de Cayley transporta este
certificado al sistema continuo. La prueba no consiste sólo en inspeccionar
autovalores en unas frecuencias; [Mehrmann y Van Dooren](https://arxiv.org/pdf/1904.13326)
describen el vínculo entre almacenamiento y realizaciones racionales pasivas.

La masa DC es −Σ k R_k/factor_rate. Se probaron dos completados:

- **Mass:** congruencia global que iguala la masa sin ajuste de frecuencias.
  Conserva pasividad pero altera la resistencia. Falla los controles; se
  conserva como rechazo.
- **Tail:** añade exclusivamente el residuo de masa estática cuando ya es
  PSD. No cambia H ni la resistencia. Un residuo materialmente indefinido se
  rechaza; no se recortan autovalores para aceptarlo. La energía inercial
  requiere la velocidad del puerto y su fuerza requiere aceleración.

Tail es sólo una aproximación inercial de lo que falta en frecuencia alta.
Su buen límite DC **no** demuestra que represente correctamente transitorios.
Tampoco identifica la pérdida material real ni la geometría de un Steinway.

## Pruebas independientes

Seis pruebas nuevas comprueban: identidad del factor y parte real positiva;
balance a pasos arbitrarios; convergencia contra DOP853; radiación angular
de una superficie uniforme; masa/energía de cola; y respuesta de segmentos
lineales con parada exacta contra una ODE independiente. El último control
incluye cálculo Decimal de 80 dígitos: evita borrar una cola por underflow
de la exponencial antes de la amplificación polinómica.

En los factores físicos de 35 modos, tres pasos (48/192/768 kHz) y estados
arbitrarios cierran balances relativos por debajo de 7,22e−16. Se conserva
el certificado completo en los datos. No es todavía la prueba de un contacto
no lineal reducido, de una nota larga, de pedales o de costo en tiempo real.

## Frecuencia y estados

Criterios heredados: máximos Z/R/X/Laplace <2%, velocidad de tabla cargada
<5%, masa estática <1e−10 y estados <10% de los 352026 de referencia.
Los 187 puntos de 60–800 Hz y tres puntos de Laplace son verificación;
no se usan para ajustar coeficientes. La cola usa únicamente la masa congelada.

| Variante Tail, celdas 128×96 | Estados | Z máx. | R máx. | X máx. | Velocidad cargada máx. |
| --- | ---: | ---: | ---: | ---: | ---: |
| 32 kHz, angular 24×48 | 5740 | 0,86017% | 0,92228% | 0,78599% | 0,30044% |
| 64 kHz, angular 24×48 | 11340 | 0,21754% | 0,23165% | 0,20083% | 0,07605% |
| 32 kHz, angular 48×96 | 5740 | 0,85976% | 0,92229% | 0,78505% | 0,30036% |

Estas tres variantes pasan los criterios de frecuencia, Laplace, masa y
estado. La de 5740 estados reduce 61,33× el almacenamiento dinámico del aire.
Esto **no** mide ahorro de CPU ni de memoria total: conservar H para evaluar
el certificado ocupa bastante más que los estados. Los residuos operativos
son compactos; el evaluador de energía/radiación aún es un banco offline.
Las grillas iniciales 64×48, angular 12×24 y las congruencias rechazadas
también se conservan en `resumen.json` y sus NPZ.

## Memoria: sin eco tardío, pero aún rechazada

Se prescribe exactamente la velocidad congelada de tabla C4 durante 4 ms
y después un puerto estacionario hasta 64 ms. Es un diagnóstico acústico;
no es la evolución libre acoplada de una nota de piano. Se compara la fuerza
de la referencia no inercial, descontando su cola en los segmentos lineales.
Las impulsiones delta de ambas colas al parar se excluyen de las curvas;
la fuerza de onda en el instante de parada usa su límite izquierdo.

Además del control trapezoidal refinado, se calcula Z_wave(s)/s mediante
polinomios de Laguerre escalados. Integrar este kernel continuo sobre cada
segmento y añadir −K(t−4 ms)·v_final representa el salto exacto de velocidad.
Así la parada no se diluye en un intervalo de integración. Gauss 8→16 cambia
el resultado relativo 8,81e−15 en la variante fina de 32 kHz y 9,23e−9 en
la de 64 kHz. El fallo restante no se resuelve refinando ese integrador.

| Variante | Fuerza RMS, kernel continuo | Error pico después de 10 ms / pico inicial | Estado |
| --- | ---: | ---: | --- |
| 32 kHz, angular 24×48 | 6,60586% | 0,18720% | Rechazada por RMS |
| 64 kHz, angular 24×48 | 8,76807% | 0,18720% | Rechazada por RMS |
| 32 kHz, angular 48×96 | 5,29358% | 0,18720% | Rechazada por RMS |

Límites sin cambios: RMS <2%, error de pico tardío <1%. El eco recurrente
de 29 ms de la etapa anterior desaparece en estas variantes. La fuerza
racional después de 10 ms es despreciable; el 0,18720% corresponde aquí a
la pequeña cola de la referencia finita. No se fuerza ni se recorta la salida
a cero. En la variante mejor, los aportes al error global son 3,88203% durante
la excitación, 3,57599% entre parada y 10 ms, y 0,40521% después de 10 ms;
sus cuadrados suman el RMS global. El máximo está a 4,03125 ms.

## Qué revela el fallo

El banco mecánico tiene modos hasta 800 Hz, pero una excitación y una parada
de duración finita no están limitadas a esa banda. Un diagnóstico independiente
del Rayleigh geométrico muestra, para 32 kHz/angular 48×96, error complejo
2,20618% a 2 kHz, 5,30792% a 3 kHz y 20,63944% a 6 kHz. Refinar sólo
factor_rate a 64 kHz/angular 24×48 empeora la memoria y llega a 94,47483%
de error complejo a 6 kHz. Una curva excelente bajo 800 Hz sigue siendo
insuficiente para validar la carga durante el ataque y la parada.

Hay también un límite físico adicional: para la superficie local en bafle,
Z(∞)=ρc∫φφᵀdS, resistencia finita. La cola inercial s·M_tail crece sin
límite. En la variante de 32 kHz/angular 48×96, la diferencia entre esa
resistencia física y el límite de su parte de onda tiene autovalor mínimo
−63,13422 s⁻¹. No puede completarse agregando sólo una resistencia PSD.
En la de 64 kHz/angular 24×48 esa diferencia sí es PSD; ello por sí solo
tampoco garantiza la precisión transitoria. No se hizo una corrección ad hoc.

Los resultados localizan limitaciones de este completado racional y de la
banda de verificación. No atribuyen todavía cada diferencia a un único
mecanismo ni demuestran que el baño finito de 6 kHz sea exacto ante una parada
instantánea. Antes de promocionar, hay que contrastar **ambas** realizaciones
con el kernel causal geométrico, y completar la frecuencia alta respetando
masa, resistencia límite, pasividad y transitorios. Luego retomar tabla,
contactos reiterados, bandas/registros y escucha con ganancia común.

## Datos y reproducción

Datos, coeficientes, CSV, fuerzas, balances, pruebas y fuentes:
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-carga-racional`.
`resumen.json` incluye parámetros, criterios, hashes de entradas, versiones
y certificados. `source-rational-air.zip` conserva las fuentes con sus hashes.
Los WAV y el paquete del usuario se conservan; esta etapa no genera nueva voz.

Con NumPy/SciPy disponibles, desde el repo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_rational_air.py -v
python tools/audit-concert-grand-rational-air.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --reference C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos --output C:/SHARED/Fisica-pendiente-2026-10-03/P6-carga-racional --rates 32000 --cells 128 96 --angular 48 96
python tools/audit-concert-grand-rational-air.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --reference C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos --output C:/SHARED/Fisica-pendiente-2026-10-03/P6-carga-racional --profile Wave32000-Cells128x96-Angle48x96-Tail --contact-reference C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire/T72-768000-Air.npz --memory-reference C:/SHARED/Fisica-pendiente-2026-10-03/P6-reduccion-aire/Grid2-Rank0.01-High40-Gamma2-Parts-memory.npz --exact-only --exact-quadrature 8 16 --wide-band
```

La generación del factor angular 48×96 tarda unos tres minutos en esta máquina;
el ensayo con kernel continuo tarda segundos. `--reuse-factors` permite
volver a medir factores Raw congelados con igual entrada y grilla. No equivale
a un benchmark de tiempo real ni a duración estimada de la investigación.
