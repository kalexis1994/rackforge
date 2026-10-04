# P6 — Reducción pasiva y prueba de ecos

2026-10-03. El contacto con aire anterior quedó en **6ebf47e**, por pedido
del usuario. **Sin selección ni promoción.** Las reducciones fuertes pasan
controles cortos pero fallan la memoria de 64 ms. Se conserva la referencia
completa para el siguiente método. El plugin instalado permanece en 0.172.0.

## Qué se prueba

Reducir los 176013 canales/352026 estados de la referencia preservando la
matriz modal de 35 modos de P2. Se agrupan canales de frecuencia y pérdida
idénticas, se construyen residuos positivos y se retienen sus términos entre
modos. Descartar direcciones espaciales débiles pasa su masa estática a la
cola: M_static = M_tail + Σ W/Ω² sigue exacta. No se reemplaza la matriz por
35 cargas diagonales ni se ajustan grabaciones o programas del piano.

Se ajustan pesos escalares no negativos de matrices espectrales PSD fijas.
La restricción Σ k A ≤ M_static se verifica en coordenadas blanqueadas;
una masa de cola materialmente negativa se rechaza. La ley conserva la
identidad de energía del baño y del contacto SI. Pasividad no implica
precisión de transferencia ni ausencia de ecos artificiales.

Las primeras grillas de 20/10/5 Hz fallan la resistencia, pese a ser pasivas.
Un primer ajuste fino alcanza su límite de iteraciones y se rechaza. Para
condicionar el ajuste se introduce ridge declarado 0,001 hacia la cuadratura
física. Ajustar sólo el error complejo favorecía la reactancia, mayor en
graves; se separa la normalización de R y X, coherente con los criterios.
La rutina final utiliza 998 puntos de entrenamiento y 187 puntos de
verificación heredados. Estos últimos **no** entran al ajuste. Las variantes
iniciales guardan sus pesos y estados, aunque la rutina final haya cambiado.

## Criterios previos

Errores máximos Z/R/X/Laplace <2%; velocidad de tabla cargada <5%; masa
estática <1e−10 relativo; al menos 10× menos canales. Para trayectorias:
tabla/puente RMS <1%; energía/trabajo de contacto <0,2%; duración <0,1%;
balances <1e−9. La memoria de 64 ms añade RMS de fuerza <2% y error de pico
tardío <1% del pico durante la excitación. Ningún límite se relajó.

## Búsqueda registrada

| Variante | Canales | Máx. error Z | Máx. error R | Reducción ≥10× |
| --- | ---: | ---: | ---: | --- |
| Grid20 | 2255 | 7.622% | 15.838% | Sí |
| Grid10 | 4127 | 3.971% | 9.819% | Sí |
| Grid5 | 7874 | 2.266% | 4.909% | Sí |
| Grid2-Rank0.0001-High20 | 24163 | 6.328% | 9.264% | No |
| Grid1-Rank0.0001-High20 | 39922 | 0.603% | 1.427% | No |
| Grid2-Rank0.01-High40-Gamma2-Raw | 17434 | 1.036% | 2.458% | Sí |
| Grid1-Rank0.01-High40-Gamma2-Raw | 30674 | 0.908% | 2.121% | No |
| Grid2-Rank0.01-High40-Gamma2-Fit | 17014 | 0.876% | 2.034% | Sí |
| Grid2-Rank0.005-High80-Gamma1.85-Fit | 15178 | 1.658% | 2.390% | Sí |
| Grid2-Rank0.01-High40-Gamma2-Parts | 17119 | 0.906% | 1.536% | Sí |
| Grid2-Rank0.01-High40-Gamma2-HighGamma32-Parts | 13654 | 1.293% | 1.554% | Sí |
| Grid0.25-Rank0.01-High10-Gamma0.5-Raw | 122696 | 0.578% | 1.004% | No |

## Variante fuerte: pasa corto, falla largo

`Grid2-Rank0.01-High40-Gamma2-Parts` usa 17119 canales, **10,28× menos**. Frente a la superficie
física, Z máxima cambia 0,90606%, R 1,53552% y X 0,81588%. La velocidad
de tabla cargada cambia hasta 0,67425%. Masa estática conservada a 7,44e−16
relativo; cola PSD certificada. La absorción Γ=2 Hz es propia de esta
representación, no una medición nueva de radiación física.

En los dos impulsos de tabla de 24 ms a 384 kHz, velocidad del puente
difiere **0,27347% RMS**, posición 0,06357%; balance total
2.565e-13. La distribución entre
energía del baño y absorción cambia; se compara trabajo de puerto y respuesta
mecánica, sin exigir equivalencia de estados internos de distinta dimensión.

También pasan las magnitudes comparadas de los dos contactos C4/124 de 4 ms
a 72/144 modos y 768 kHz. El segundo conserva cuerda, tabla y aire; se
contabiliza el trabajo externo del rearmado. Este éxito no salva el eco largo.
El refinamiento del segundo contacto sigue pendiente en la referencia.

## Diagnóstico de memoria: misma fuente, 64 ms

Se prescribe exactamente la misma velocidad de tabla de C4 congelada,
lineal entre sus muestras a 128 kHz durante 4 ms. Después el puerto permanece
en reposo. Una exponencial aumentada integra exactamente cada segmento,
incluidos canales sobreamortiguados. La fuerza libre se calcula agrupando
frecuencia/pérdida sin aproximación. El test independiente compara esa fuerza
contra DOP853 y exponencial matricial densa.

Es un control de memoria acústica: **no es una nota de piano acoplada durante
64 ms**, ni un pedal o apagador. El salto prescrito de velocidad a reposo
requiere un impulso de cola independiente; el control tardío compara la
fuerza oscilatoria, sin convertir ese impulso en un pico muestreado.

| Control | Error RMS de fuerza | Error de pico después de 10 ms / pico inicial | Resultado |
| --- | ---: | ---: | --- |
| Grilla gruesa Γ=2 Hz | 49.39123% | 55.30072% | Rechazado |
| Absorción alta Γ=32 Hz desde 1200 Hz | 16.28266% | 8.75142% | Rechazado |
| Sólo descarte espacial, grilla/Γ originales | 0.29763% | 0.06273% | Pasa memoria; falla objetivo de coste |

El primer pico de error grueso cae a **29,0625 ms**, compatible con la
recurrencia de los canales altos espaciados 40 Hz: periodo 25 ms, desplazado
por los 4 ms de excitación. El control de 24 ms terminaba antes de ese pico.
La fuerza tardía de la referencia llega sólo a 0,18720% del pico inicial.

Aumentar absorción de los canales altos mantiene los criterios de frecuencia
y baja el eco, pero quedan errores de memoria demasiado grandes, con máximo
a 10,09375 ms. No se presenta como una cura: altera la representación y no
identifica una pérdida física del piano.

El control espacial preserva frecuencias, Γ=0,5 Hz y masa estática. Usa
122696 canales: **30,29% menos**, insuficiente para el objetivo de 10×.
Pasa frecuencia y memoria. Esto apunta al adelgazamiento de la grilla y sus
modificaciones temporales como problema de las variantes fuertes, no a la
necesidad de cambiar el fieltro o el detune. El control espacial no tiene
todavía el banco completo de contactos y no se promueve al plugin.

## Pruebas y decisión

Cuatro pruebas pasan: agrupación exacta con frecuencias desordenadas/pérdidas
distintas; ajuste multipuerto conocido con verificación fuera del entrenamiento;
masa estática y energía tras descarte espacial; excitación lineal y fuerza
tardía exactas contra ODE independiente, con canal sobreamortiguado.

**Ninguna variante cumple todos los criterios.** Se conserva el banco completo
como referencia, los controles rechazados y el control espacial seguro en los
ensayos realizados. No se generan audios de fábrica ni paquetes nuevos a
partir de un éxito parcial. El rechazo evita trasladar ecos al sostenido.

## Siguiente método

Ensayar una aproximación racional matricial con polos compartidos que se
adapten al kernel, en lugar de adelgazar su grilla periódica. Verificar
estabilidad, realidad, reciprocidad y pasividad global de la realización;
después repetir estos mismos controles temporales antes de escuchar.
[SINTEF](https://www.sintef.no/en/software/vector-fitting/) describe el ajuste
racional multipuerto y la corrección de matrices de residuos;
su [documentación de pasividad](https://www.sintef.no/en/software/vector-fitting/passivity/)
separa polos estables de una matriz incapaz de generar energía. Una prueba
de banda o una corrección de autovalores muestreados sola no sustituirá la
certificación ni la prueba de memoria. Revisar la implementación/licencia
antes de adoptar código de terceros.

Otra opción es una reducción de estados con certificado energético y
absorción de frontera consistente. Mantener la grilla completa como control
hasta tener una alternativa que pase. Más allá de 64 ms, contrastar también
la propia referencia espectral finita con el kernel causal geométrico de
Rayleigh: no suponer que su grilla evita recurrencias indefinidamente.
Banda alta, unísono, P4/Timoshenko,
partición de pérdidas, acción y pedal continúan en el plan; esta etapa no
identifica el déficit audible del Steinway.

## Artefactos y reproducción

`C:\SHARED\Fisica-pendiente-2026-10-03\P6-reduccion-aire`: baños NPZ con
pesos/bases congelados, errores de frecuencia, trayectorias de tabla/contacto,
fuerzas de memoria, resumen con criterios y hashes, logs y fuentes finales
con dependencias. Los estados internos permanecen separados de ondas físicas
escapadas. Los tiempos registrados son diagnóstico, con un hilo BLAS por
proceso, y algunas pruebas se superponen; no son un benchmark de tiempo real.

SHA256 de `source-air-reduction.zip`:
`3482f2dc378825cf44d6afaacea653fa6f06680ba99821ca9ec361e00d20809e`.

Paquete 0.172.0 preservado:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

Con NumPy/SciPy:

```powershell
$reference = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos'
$contactReference = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire'
$asset = 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz'
$outputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-reduccion-aire'
python -m unittest discover -s tools/tests -p test_concert_bath_reduction.py -v
python tools/audit-concert-grand-air-reduction.py --reference $reference --asset $asset --output $outputDir --spacing 2 --rank-tolerance 0.01 --high-spacing 40 --gamma 2
$profile = 'Grid2-Rank0.01-High40-Gamma2-Parts'
python tools/audit-concert-grand-air-reduction.py --phase board --reference $reference --asset $asset --output $outputDir --profile $profile
python tools/audit-concert-grand-air-reduction.py --phase memory --reference $reference --contact-reference $contactReference --asset $asset --output $outputDir --profile $profile
$inputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input'
$bath = "$outputDir/$profile.npz"
python tools/audit-concert-grand-bath-contact.py --input $inputDir --asset $asset --bath $bath --output "$outputDir/contact" --transverse 72 --rates 768000 --relaunch
python tools/audit-concert-grand-air-reduction.py --reference $reference --asset $asset --output $outputDir --spacing 2 --rank-tolerance 0.01 --high-spacing 40 --gamma 2 --high-gamma 32
python tools/audit-concert-grand-air-reduction.py --reference $reference --asset $asset --output $outputDir --spacing 0.25 --rank-tolerance 0.01 --high-spacing 10 --gamma 0.5 --no-fit
```

Repetir la fase `memory` con el nombre del nuevo perfil conserva exactamente
la fuente. `contact.log` conserva las magnitudes y balances de sus dos
contactos. El renderer de fábrica no cambia.
