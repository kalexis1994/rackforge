# P6 — Carga pasiva con masa y resistencia límite

2026-10-04. La referencia geométrica y el comparador corregido quedaron en
**c353719**, por pedido del usuario. Esta etapa mejora la aproximación compacta
con controles físicos. **No modifica audio ni selecciona un modelo de fábrica.**

## Problema y construcción

La cola anterior s·M_tail conserva masa estática, pero crece indefinidamente
con la frecuencia. El Rayleigh geométrico tiene en cambio resistencia límite
finita R∞=ρc∫φφᵀdS. Además, la resistencia del factor anterior en el extremo
de Nyquist no deja siempre un residuo positivo para completar R∞.

Se promedian dos taps vecinos del factor FIR: H_new(z)=H(z)(1+z⁻¹)/2.
Se conserva el cero DC, la inversión de las direcciones opuestas y la
construcción positiva real. El cero adicional en Nyquist hace nula la
resistencia límite de la parte de ondas bajo la misma transformación de
Cayley. Es una aproximación controlada de radiación, no un nuevo mecanismo
identificado en el piano. Se verifica su transferencia fuera de los extremos.

Para completar los residuos **ya positivos** M_res y R_res, se construye:

M_res=B diag(τ) Bᵀ; R_res=B Bᵀ;
Z_res(s)=B diag(sτ/(1+sτ)) Bᵀ.

B y τ provienen de raíces/eigenvectores de las matrices, no de un ajuste de
muestras. Se rechazan residuos no positivos; no se recortan eigenvalores.
La masa DC y la resistencia infinita son condiciones de construcción,
no evidencia por sí solas de fidelidad en las frecuencias intermedias.

Los estados q satisfacen q'=(Bᵀv−q)/τ, con fuerza B(Bᵀv−q). Almacenamiento
E=Στq²/2 y pérdida ||Bᵀv−q||² cumplen trabajo=ΔE+pérdida. Su kernel de
paso es B diag(exp(−t/τ)) Bᵀ: **no tiene delta inercial**. La integral de
cada segmento lineal usa expm1; la parada sigue siendo exacta. No se aplica
un umbral para esconder colas ni un normalizador nuevo.

La interpretación como relajación es una representación pasiva de la carga
del aire. Sus tiempos no se atribuyen a fricción del fieltro, amortiguación
de cuerdas, sala o un parámetro aprendido del Steinway.

## Pruebas y límites

Cuatro tests nuevos cubren matrices no diagonales, masa/límite, pasividad
fuera del eje, rechazo de residuos, trabajo con estados arbitrarios y pasos
distintos, y fuerza/parada contra una ODE independiente. También prueban una
tabla de dos modos acoplada al aire: convergencia contra DOP853 y balances
global y del puerto. Pasan los **15 tests** de esta etapa, carga racional y
geometría. El límite DC se verifica por extrapolación de puntos de Laplace
finitos, evitando convertir cancelación numérica en un ensayo de física.

En los 35 modos P2 del candidato fino:

- 5810 estados frente a 352026: 60,59× menos. Es un recuento, no un benchmark
  de CPU de fábrica. Hay 35 estados de relajación y un tap adicional por modo.
- Tiempos de relajación: 19,2453–22,7259 µs.
- Masa y resistencia límite realizadas coinciden al nivel de roundoff. Son completados
  algebraicos de matrices, sin reparación.
- Bajo 800 Hz, máximos Z/R/X: 1,05394% / 0,84679% / 1,24298%.
  Laplace: 0,98234%; velocidad cargada: 0,46894%. Pasan los criterios previos.
- Balance de trabajo con estados aleatorios a 48/192/768 kHz: <2,93e−16
  relativo; pérdida radiada no negativa. Es adicional al certificado algebraico.

## Fuerza de memoria: margen recuperado

La entrada sigue siendo la velocidad C4 congelada de 4 ms y una parada
instantánea. **Todos** los errores usan el RMS y pico inicial del baño anterior.
El ensayo completo dura 64 ms; no es una nota de audio acoplada de 64 ms.

| Carga | RMS geométrico 128×96 | 192×144 | 256×192 |
| --- | ---: | ---: | ---: |
| Limitada 32 kHz, angular 24×48 | 2,89569% | 2,92436% | 2,93658% |
| Limitada 32 kHz, angular 48×96 | **0,88913%** | **0,97641%** | **1,01116%** |
| Limitada 64 kHz, angular 24×48 | 5,13265% | 5,13832% | 5,14096% |

Las variantes angulares gruesas siguen rechazadas. La fina pasa el límite
de 2% en las tres superficies. Su margen final de 0,98884 puntos porcentuales
supera el cambio espacial de la referencia de 0,05533 puntos porcentuales;
es una mejora del margen bajo los controles medidos, sin afirmar que la
superficie continua esté exactamente identificada.

El candidato anterior con masa inercial daba 1,94783–1,99520%. El nuevo
error frente a la grilla más fina baja aproximadamente a la mitad. Tras
10 ms el pico de error es roundoff (<1,41e−16 del pico inicial). El cero
exacto del kernel geométrico y el decaimiento del modelo racional no son
el mismo mecanismo; se contrastan sus fuerzas sin enmascaramiento.

## Banda alta: todavía no pasa

La referencia geométrica usa las mismas celdas 128×96 y modos bajos, con seis
puntos adicionales independientes. Igualar el límite infinito no iguala
automáticamente la banda intermedia.

| Frecuencia | Error complejo del candidato fino |
| --- | ---: |
| 1 kHz | 1,42136% |
| 1,2 kHz | 1,42013% |
| 2 kHz | 0,43877% |
| 3 kHz | 1,02069% |
| 4,5 kHz | **2,31845%** |
| 6 kHz | **5,54673%** |

El control diagnóstico de 2% falla en los últimos dos puntos. A 6 kHz la
etapa previa daba 20,63944%; la reducción es real, pero no basta para promover
la carga. La reactancia de la superficie baja es diminuta allí, por lo que
sus cocientes relativos son grandes; se publica también el error complejo
y resistivo para no confundirlos con un error de amplitud total equivalente.

## Tabla acoplada y estado persistente

El banco mantiene masas modales unitarias, las mismas matrices de rigidez y
pérdida material supuesta 0,046ω del ensayo anterior. En midpoint, el puerto
se elimina con Z(2/dt) y la fuerza constante de sus estados vivos. La solución
usa un Schur pequeño; después se actualizan todos los estados del aire.

El nuevo límite finito permite un impulso de velocidad sin cola inercial
instantánea. Se conserva exactamente el estado del aire al aplicar un segundo
impulso a 8 ms. El trabajo mecánico de cada salto entra en el balance externo.
Se ensaya el mismo puerto de tabla y amplitudes 0,002 / 0,0015 durante 24 ms,
a 192 y 384 kHz. No es contacto no lineal de martillo ni un relanzamiento de
acción; los resultados numéricos están en `resumen.json` y `Board*.csv`.

- Refinar 192→384 kHz cambia velocidad 0,035315% y desplazamiento 0,008664%:
  pasan el límite previo de 0,1% para ambos.
- En los controles 0/8/24 ms, balance global <3,58e−13 y presupuesto de aire
  <5,65e−14 relativos al trabajo externo. Pasan el límite 1e−9.
- La velocidad difiere del baño Fine384 un 0,11747% en la corrida fina;
  el desplazamiento, 0,05096%. Se conserva este baño como control de su propio
  modelo, sin convertirlo nuevamente en verdad exacta de banda ancha.
- Hay 2209 pasos con trabajo de puerto negativo a 384 kHz: el aire devuelve
  energía almacenada mientras su pérdida radiada sigue siendo no negativa.
- La corrida fina tarda ~127 s para 24 ms físicos. **No es tiempo real.**

Los controles de balance de esta corrida se evalúan a 0/8/24 ms; el test con
estados arbitrarios verifica por separado la identidad de un paso. No se
afirma haber medido energía en todos los pasos del banco grande.

## Decisión y seguimiento

La carga recupera margen de memoria y un límite alto correcto con pasividad
explícita. Queda pendiente resolver la transferencia de 4,5–6 kHz, probar
contacto no lineal y contactos reiterados, y extender notas/velocidades/banda.
No se elige para fábrica porque el éxito del banco bajo no garantiza cuerpo
o ataque más realistas en el audio del Steinway. Pérdidas, ambas caras/borde,
unísono, P4 y acción/pedal siguen abiertos en el plan.

Siguiente: refinar el factor temporal/angular contra geometría y repetir
los mismos controles de masa, límite, memoria y tabla; después trasladar
este puerto certificado al contacto SI con estados persistentes.

## Reproducción y archivos

`C:\SHARED\Fisica-pendiente-2026-10-04\P6-carga-limitada`: informe, matrices
de relajación, fuerzas, trayectoria de tabla, hashes, fuentes y pruebas.
Los factores y referencias anteriores permanecen congelados. No se duplica
el FIR grande en cada NPZ: se identifica por ruta/hash y se reconstruye con
el código archivado. El rfplugin 0.172.0 instalado conserva su SHA anterior.

```powershell
python -m unittest discover -s tools/tests -p test_concert_bounded_air.py -v
python tools/audit-concert-grand-bounded-air.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --bath C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos --contact C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire/T72-768000-Air.npz --rational C:/SHARED/Fisica-pendiente-2026-10-03/P6-carga-racional --geometry C:/SHARED/Fisica-pendiente-2026-10-04/P6-kernel-geometrico --output C:/SHARED/Fisica-pendiente-2026-10-04/P6-carga-limitada --board-rates 192000 384000
```
