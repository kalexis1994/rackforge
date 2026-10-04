# P6 — Resolución temporal y polar del aire

2026-10-04. La carga limitada y la tabla persistente anteriores quedaron en
**5834c39**, por pedido del usuario. Se refina la aproximación del aire contra
la misma geometría. **Sin cambio del plugin ni mejora audible demostrada.**

## Cuadratura más rápida, misma proyección

Cada celda proyectada ocupa pocos bins de retardo. El generador anterior
construía la CDF en todos los bins para todas las celdas. La versión sparse
guarda sólo intersecciones; integra la misma distribución de la suma de dos
uniformes con tramos triangulares/lineales estables. Conserva área, promedio
de cada celda, diferencias temporales y direcciones opuestas por inversión.

En el factor de producción congelado 32 kHz/48×96, la diferencia relativa
contra el generador denso es **1,22437e−14**, bajo el criterio previo de 1e−10.
La suma DC y la inversión opuesta también pasan. Un test verifica equivalencia
en formas no uniformes y dos tasas. El test físico independiente de radiación
de una superficie uniforme usa ahora la implementación sparse.

La generación de ese control tarda 6,7–7,0 s. El perfil de la etapa densa
reportaba unos 190 s, incluyendo generación y sus verificaciones; no son
benchmarks de coste de fábrica directamente equivalentes. La generación más
rápida permite probar refinamientos sin cambiar los blancos ni ajustar samples.

## Intento temporal rechazado y diagnóstico angular

Duplicar 32→64 kHz manteniendo 48 puntos polares y 96 azimutales mejora la
banda baja, pero **empeora** la fuerza y el control de 6 kHz. La variante se
conserva como rechazada: 2,11772% RMS frente a geometría fina y 12,64950% de
error complejo a 6 kHz. Pasividad y masa no garantizan convergencia del modelo.

Se separa entonces la cuadratura angular de cualquier histograma o mapa de
Cayley: integral exacta de cada celda en el campo lejano, proyectada como
Gram hemisférico y comparada con la resistencia Rayleigh directa a 6 kHz.
La evaluación por bloques evita almacenar todas las fases; un test demuestra
su equivalencia con la evaluación densa para bloques de tamaños distintos.

| Puntos polares | Azimutales | Error resistivo relativo, 6 kHz |
| ---: | ---: | ---: |
| 48 | 96 | 12,575874% |
| 96 | 96 | 0,000554258% |
| 48 | 192 | 12,575857% |
| 96 | 192 | 0,000002221% |

La evidencia identifica el muestreo polar como el error dominante de este
control. Duplicar sólo azimut no lo resuelve. El refinamiento temporal anterior
dejaba más expuesto ese error; no se acepta simplemente por usar más tasa.
Esta atribución es del banco numérico, no del timbre escuchado en el plugin.

## Candidato 64 kHz / polar 96 / azimut 96

Se aumenta la cuadratura polar y se conservan la superficie 128×96, los 35
modos P2, las masas y resistencia límite congeladas, la misma construcción
positiva real, el completado por relajación y todos los umbrales. No se busca
una combinación de coeficientes que se parezca al Steinway.

- 11445 estados, frente a 352026: 30,76× menos. Los 9216 canales de salida
  del factor incrementan el coste de evaluar pérdidas; el recuento de estados
  no implica velocidad de ejecución equivalente ni tiempo real.
- Relajación: 9,6941–11,3802 µs. Polos de ondas estables en −128000 s⁻¹.
- Masa realizada: 2,56e−16 relativo; límite resistivo realizado: 2,79e−15.
  Se miden desde B diag(τ) Bᵀ / BBᵀ, no desde los blancos almacenados.
- Máximos Z/R/X bajo 800 Hz: 0,26452% / 0,21300% / 0,31161%.
  Laplace: 0,24636%. Velocidad cargada: 0,11768%. Todos pasan.
- Balance con estados aleatorios y pasos a 48/192/768 kHz: <3,62e−16;
  pérdida radiada no negativa. Se conserva el certificado energético.

## Memoria: mismos puertos, umbrales y normalizador

La velocidad C4 de 4 ms y la parada exacta permanecen congeladas. Los errores
siguen normalizados por el RMS/pico inicial del baño original. No se cambia
la ganancia ni el denominador para aceptar el candidato. El control dura 64 ms;
es puerto prescrito, no una nota de audio libre acoplada.

| Kernel geométrico | RMS de fuerza del candidato |
| --- | ---: |
| 128×96 | 0,51510% |
| 192×144 | 0,56782% |
| 256×192 | **0,59509%** |

Pasan los tres controles de 2%. El pico de error tras 10 ms es roundoff,
<1,37e−16 del pico inicial, sin máscara de salida. El candidato 32 kHz anterior
daba 0,88913–1,01116%; el margen mejora también frente a las grillas refinadas.

## Transferencia alta y barrido independiente

Los seis puntos altos iniciales pasan. Además se evalúan **61 frecuencias**
uniformes entre 1 y 6 kHz contra el kernel geométrico continuo integrado con
Gauss 16 a 128 kHz. Esa referencia temporal ya pasó los controles independientes
de Duffy/Laplace y refinamiento temporal; no se ajusta con el candidato.

| Frecuencia | Error complejo, celdas 128×96 |
| --- | ---: |
| 1 kHz | 0,35712% |
| 1,2 kHz | 0,35602% |
| 2 kHz | 0,10545% |
| 3 kHz | 0,26278% |
| 4,5 kHz | 0,63625% |
| 6 kHz | **1,12750%** |

El máximo de todo el barrido es 1,12750%, a 6 kHz; el resistivo, 0,70203%.
El candidato anterior daba 5,54673% complejo a 6 kHz. El criterio sigue en 2%.
En los seis puntos altos de las grillas 192×144 y 256×192 los máximos son
1,06807% y 1,06612%, también bajo 2%. El cambio máximo geométrico frente a
128×96 es 0,49878%, inferior al margen observado del candidato.

La reactancia geométrica es muy pequeña en alta frecuencia: sus cocientes
relativos grandes se conservan en los CSV, junto a Z y R. No se interpretan
como igual porcentaje de error en la amplitud total. Los controles pasan
**en los puntos y discretizaciones medidos**, sin certificar todo el espectro
continuo ni frecuencias superiores a 6 kHz.

## Pruebas, alcance y siguiente paso

Pasan **24 tests** de factor, carga limitada, geometría e impedancia de
superficie, incluidos los dos tests nuevos de equivalencia. La optimización
no modifica coeficientes ni resultados anteriores almacenados. Se archivaron
también el intento temporal fallido y la evidencia angular que condujo al
candidato correcto; los umbrales no se relajaron.

La carga está lista como **candidato de investigación para el contacto SI**.
El ensayo de tabla persistente de la etapa anterior pertenece al factor de
32 kHz. No se presenta como una nueva trayectoria de tabla/martillo de este
factor de 64 kHz: su acoplamiento no lineal y sus refinamientos siguen pendientes.

La transferencia hasta 6 kHz corresponde a **las mismas formas de superficie
de los 35 modos estructurales bajo 800 Hz**. No se han agregado los modos de
tabla altos que también necesita un piano completo, ni identificado pérdidas,
segunda cara, borde real, sala o unísono. No se promueve audio de fábrica.

Siguiente: llevar este puerto pasivo al martillo/cuerda/puente SI, comparar
contacto frío y reiterado con estados persistentes, y medir balances y precisión
con los criterios previos. Luego ampliar la base estructural, notas/velocidades
y observación SI antes de escuchar. P4, graves y acción/pedal siguen en el plan.

## Datos y reproducción

`C:\SHARED\Fisica-pendiente-2026-10-04\P6-resolucion-aire` contiene controles
y factor rechazado; `Polar96` contiene los coeficientes y medidas del candidato.
`band.json`, `band-sweep.csv/.npz` y `angular-probe.json` conservan los blancos
y errores. Fuentes, hashes y pruebas acompañan al resumen. El rfplugin instalado
conserva su SHA y sonido anteriores.

```powershell
python tools/audit-concert-grand-air-resolution.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --control C:/SHARED/Fisica-pendiente-2026-10-03/P6-carga-racional/Wave32000-Cells128x96-Angle48x96-Tail.npz --output NUEVO_DESTINO --angular 96 96
python tools/audit-concert-grand-bounded-air.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --bath C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos --contact C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire/T72-768000-Air.npz --rational NUEVO_DESTINO --geometry C:/SHARED/Fisica-pendiente-2026-10-04/P6-kernel-geometrico --output NUEVO_DESTINO --profiles Wave64000-Cells128x96-Angle96x96-Sparse
python tools/audit-concert-grand-air-band.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --coefficients NUEVO_DESTINO/Wave64000-Cells128x96-Angle96x96-Sparse.npz --geometry C:/SHARED/Fisica-pendiente-2026-10-04/P6-kernel-geometrico --output NUEVO_DESTINO
```

Usar otro destino para el refinamiento angular 48×96, preservando el rechazo.
El generador impide sobrescribir un manifiesto congelado. `--redo-probe` en
el barrido permite repetir la evidencia angular en lugar de reutilizarla.
