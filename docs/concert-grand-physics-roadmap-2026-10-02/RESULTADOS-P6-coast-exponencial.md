# P6 — Integración exponencial del sostenido, sin reducir la física

Fecha: 2026-10-05. Rama `codex/concert-grand-restrike`, base `f124550`.
El usuario cuestionó la estimación de unos 40 minutos para un segundo de
nota. Esa estimación extrapolaba el coste del integrador anterior; no era
un coste mínimo impuesto por las ecuaciones. Este ensayo modifica el método
numérico del sostenido y conserva los parámetros, modos y estados físicos.

## Cambio matemático

El midpoint anterior comprime la frecuencia de un oscilador lineal:
ω_num = (2/h) atan(hω/2). Con las oscilaciones axiales rápidas, disminuir el
paso de todo el sistema es una solución costosa. Se separa el potencial en
su parte cuadrática constante y su resto cúbico/cuártico no lineal.

La parte cuadrática usa seno y coseno exactos de sus frecuencias. Para un
paso mecánico h, en coordenadas modales normalizadas:

```text
C = cos(hω), S = h sinc(hω), Q = (h²/2) sinc²(hω/2)
q1 = C q0 + S v0 − Q ḡ
v1 = −ω² S q0 + C v0 − S ḡ
ḡ = ∫₀¹ ∇U_nl((1−ξ)q0 + ξq1) dξ
```

Aquí sinc(x)=sin(x)/x. No se sustituye el resto no lineal por un resorte
lineal. Su integral es exacta con dos puntos Gauss; una segunda versión
evalúa el mismo polinomio mediante los campos de los extremos, reutilizando
el primero durante cada solve. Ambas mantienen el residuo completo y la
tolerancia relativa 2e−14. El tangente completo queda disponible si la
iteración deja de contraerse.

Referencia del paso mecánico: Wang y Wu, ecuación (5),
[Long-time oscillatory energy conservation of total energy-preserving methods
for highly oscillatory Hamiltonian systems](https://arxiv.org/abs/1807.01044).
La implementación del puerto disipativo y sus controles son de este repo:
medio paso mecánico → paso completo de velocidad/pérdidas/aire por midpoint
pasivo → medio paso mecánico. La eliminación de variables del paso de aire
deja un solve de 59 coordenadas de tabla; después se actualizan **todos** los
estados de onda y relajación. No se añade masa instantánea de aire.

La diagonalización usa bloques invariantes del Hamiltoniano cuadrático,
verificados contra la matriz completa. No recorta una base: permanecen 72
modos transversales por dirección, 144 axiales, 59 de tabla y 38232 estados
acústicos. Sólo sirve mientras el martillo está separado; un nuevo contacto
requiere el integrador de contacto. No se implementa una acción de teclado.

## Controles independientes

`test_concert_exponential_coast.py`: seis pruebas, pasan en 1,770 s.
Comprueban gradiente/potencial original, fases lineales contra solución
analítica con pasos mayores que el periodo rápido, energía no lineal,
presupuesto total y de aire con radiación angular independiente, convergencia
de orden dos contra DOP853 y rechazo de contacto/pasos inválidos.

`test_concert_polynomial_coast.py`: repite esos seis controles con la integral
polinómica y añade comparación contra Gauss a tres amplitudes. Pasan siete
pruebas en 1,750 s; la diferencia de integral queda bajo 2e−13 relativo.
Son 13 ejecuciones de pruebas, con seis controles compartidos. Se mantiene
el límite energético de 1e−9; conservar energía por sí solo no verifica fase
ni fidelidad de la presión.

## Medición sobre la nota física existente

Se inicia cada ensayo desde **el mismo contacto frío a 4 ms**, conservando
posición, velocidad, onda y relajación. Se reutilizan el factor refinado
seleccionado de 90→59 y el kernel de los mismos micrófonos. No se duplica el
archivo padre de coeficientes, no hay refit, EQ ni ajuste de nivel.

La comparación mide la presión compleja de las dos cápsulas en 60–6000 Hz,
con sondas cada 20 Hz e integral de aceleración de la interpolación Hermite
C1. Compara exactamente el tramo después del ataque, no un recorte de WAV
con fade. También compara posición/velocidad de tabla en el tiempo.
Los criterios de este ensayo son 1% para presión y B/V; pérdida inferida
no negativa hasta 1e−9 de la energía inicial. No son el control distinto de
captura 64→128 kHz del WAV anterior, cuyo límite era 0,1%.

Primero se midió un prefijo de 12 ms a 64/128/256/512 kHz con Gauss:
64 vs 512 cambia 0,012095% en presión; 256 vs 512, 0,0006013%.
El prefijo motivó el ensayo completo, sin darlo por aprobado de antemano.

Después se midieron **246 ms de sostenido**, hasta los mismos 250 ms de nota,
con la integral polinómica:

| Integración | Dinámica medida | Presión vs 256 kHz | Posición vs 256 kHz | Velocidad vs 256 kHz |
|---|---:|---:|---:|---:|
| 64 kHz | 54,724 s | 0,053813% | 0,017496% | 0,039735% |
| 128 kHz | 99,113 s | 0,013612% | 0,003679% | 0,008491% |
| 256 kHz | 174,632 s | Referencia de refinamiento | Referencia | Referencia |

Pasan las comparaciones y ambos controles de selección; **64 kHz es candidato
para este intervalo y esta nota**. El error de presión al doblar 64→128 queda
aproximadamente en un cuarto, compatible con orden dos. Máximo de iteraciones
9/7/5; no se necesitó el tangente completo en ningún caso.

Contra el midpoint anterior a 768 kHz, la presión difiere 0,378119% / 0,344133%
/ 0,331288%. La referencia anterior también tiene error de discretización;
esa diferencia no identifica cuál trayectoria es la solución continua.
El refinamiento propio y el control ODE independiente son necesarios para
justificar la selección. Tampoco identifica la cercanía con un Steinway.

Energía inicial idéntica: 0,04690687099 J. A 64 kHz termina en 0,04038362992 J,
con calor material 0,00486774415 J y radiación inferida 0,00165549693 J.
En los tres ensayos la pérdida inferida es positiva. La radiación angular
completa **no** se integra en este sostenido largo: el balance independiente
se comprueba en los fixtures pequeños, no se atribuye al ensayo de 250 ms.

## Coste y siguiente paso

La continuación anterior hasta 250 ms tardó 578,187 s, incluyendo controles
de almacenamiento y guardado. Los 54,724 s nuevos miden sólo dinámica:
excluyen inicialización, evaluación de energía, espectros y guardado.
No se presentan esos tiempos de alcances diferentes como un speedup exacto.
La nueva dinámica requiere 222,454 s por segundo de fuente en este tramo.
La estimación para un segundo, reutilizando el ataque ya verificado y
añadiendo controles/salida, pasa a **unos 4–6 minutos**. Todavía no se ha
medido ni renderizado ese segundo y tampoco se demuestra tiempo real.

Siguiente: extender C4 a un segundo con 64 y 128 kHz y repetir convergencia
de presión/B/V durante todo el sostenido antes de escoger la trayectoria
audible. Mantener el contacto ya validado; comprobar por separado cualquier
nueva nota, recontacto o cambio de base. No se extrapola la aprobación de
250 ms al piano completo. Siguen abiertos 59→90, geometría/pérdidas
identificadas, integral angular larga, unísono, P4, apagador/acción/pedal y
plugin. El coste del puerto acústico completo sigue siendo importante;
una futura convolución rápida o reducción pasiva requiere su propio control.

## Archivos y reproducción

Informes únicos en el repo:

- `medidas-P6-coast-exponencial/prefijo.json` conserva el ensayo Gauss y el
  hash de la versión anterior del wrapper usada para ejecutarlo.
- `medidas-P6-coast-exponencial/sostenido-250ms.json` conserva los hashes de
  inputs, solver, wrapper y las tres trayectorias polinómicas completas.

Datos únicos:
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-coast-exponencial-prefix` y
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-coast-exponencial-250ms`.
No se genera otro WAV de 250 ms ni se reemplaza la primera escucha.

Ejecutar `tools/audit-concert-grand-exponential-coast.py --solver polynomial
--duration 0.246 --rates 64000 128000 256000`, con los mismos argumentos
físicos de [la primera nota](RESULTADOS-P6-primera-nota.md). Además requiere
`--kernel`, `--kernel-report`, `--reference-trajectory`, `--reference-report`,
`--output` y `--report`: sus rutas exactas se registran en el JSON completo.
Los destinos deben ser nuevos para conservar las mediciones. Python 3.12.14,
NumPy 2.3.5, SciPy 1.18.1 y un hilo BLAS; sin bytecode. Las pruebas usan
`python -m unittest discover -s tools/tests -p <nombre_del_test>`.

No se cambia el motor ni `C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin`.
