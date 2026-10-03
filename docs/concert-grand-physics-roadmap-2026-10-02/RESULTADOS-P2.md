# P2 — tabla ortótropa, costillas y puente

Fecha: 2026-10-02. Prototipo nativo medido; ninguna variante seleccionada.
Audios y datos: `C:\SHARED\Fisica-pendiente-2026-10-02`.

## Modelo estructural

Se construyó un solver Ritz de placa ortótropa con masa y rigidez positivas.
Control analítico: placa simplemente apoyada sin vigas. Piloto: bordes empotrados,
nueve costillas discretas transversales y un puente longitudinal recto centrado.
Las vigas añaden masa y rigidez de flexión mediante la misma función de
desplazamiento de la placa. Los modos tienen masa generalizada unitaria.

Material de referencia: valores publicados de Sitka y maple en
[Boutillon y Ege, tabla 1](https://arxiv.org/html/1305.3057v1#S2.T1).
Panel supuesto de 1,4 × 1,0 m, espesor 8 mm, densidad 392 kg/m³;
Ex=11,5 GPa, Ey=0,47 GPa, Gxy=0,5 GPa, νxy=0,3.
Las dimensiones, posiciones y apoyos **no están medidos en el Steinway**.
Se conserva la pérdida modal heredada 0,046; no es una pérdida identificada
por este paper o por las muestras.

No están incluidos: contorno de piano de cola, puente curvo y puente de bajos,
flexibilidad del aro, corona/pretensión, acoplamiento de membrana por la
excentricidad de las vigas, torsión, ni realimentación de carga del aire.
Es una aproximación estructural explícita, no una réplica del instrumento.

## Verificación estructural

| Control | Resultado |
| --- | ---: |
| Frecuencias de placa simple frente a solución analítica, error relativo máximo | 5,36e−14 |
| Desplazamiento y pendiente en bordes empotrados | <3,22e−14 |
| Error de ortonormalidad respecto a la masa | 1,93e−14 |
| Residuo relativo del problema modal | 4,47e−13 |
| Error máximo de reciprocidad de movilidad | 1,75e−18 |
| Mínimo autovalor de conductancia, puntos/frecuencias probados | +2,04e−7 |

Masa y rigidez pasan Cholesky. Base Ritz enriquecida desde 12×12 hasta 52×32.
En el banco con costillas y puente, 52×32 frente a 44×28: máximo cambio de los
primeros 80 polos 0,0908%, de los 256 polos 0,3415%. Cambio relativo de movilidad
entre tres puntos: 0,0342% a 20–250 Hz, 0,5100% a 250–1000 Hz y 1,3831% a
1000–2500 Hz. No se midió aún convergencia de la cuadratura acústica ni se hizo
esta secuencia de convergencia por separado para las otras dos ablaciones.

## Integración nativa

Sólo bajo `cfg(test)` y mediante `CG_RITZ_BOARD` leído una vez por proceso.
Se sustituyen los modos bajos de tabla; **se conservan todos los modos de
fábrica desde 800 Hz**, con sus coeficientes originales. Banco híbrido:

| Banco bajo | Modos estructurales <800 Hz | Modos altos heredados |
| --- | ---: | ---: |
| Panel | 87 | 121 |
| Panel + puente | 84 | 121 |
| Panel + costillas + puente | 35 | 121 |

La línea de excitación recorre el 10–90% interior del panel a media anchura.
Se guardan 16 proyecciones de puente y una superficie 40×24 por modo. La
distribución lineal de fuerza entre puntos, su expansión coseno y la carga
de cuerda usan las mismas formas. Tres pruebas nativas, una por banco, verifican
masa unitaria, proyecciones de carga/excitación y retención del complemento alto.
La superficie de radiación procede de los mismos modos. El complemento alto
sigue siendo sintético y no comparte toda la geometría física del banco bajo.

Fábrica apagada: 16 renders idénticos a la base congelada y ambas huellas
preservadas (`232b085ed4248398`, `4371ab4adc1542ab`). P1b/direccional están
apagados en estos ensayos para aislar P2. El prototipo no soporta todavía
cambios de geometría en vivo ni está preparado para distribución.

El `report.json` estructural fue generado antes de esta integración y conserva
sus gates pendientes originales. Este documento y `resumen-P2.json` registran
las verificaciones posteriores sin reescribir aquel reporte congelado.

## Ensayo audible con el observador habitual

C4/C5/C6/F#6, velocidades 28/60/92/124, tecla sostenida 2 s, salida 4 s;
cuatro sostenidos de 12 s por banco. 44,1 kHz, bloques de 256, calentamiento
200 ms. Misma ganancia y ajustes, sin nuevo detune, EQ ni normalización.
Referencias bitKlavier Steinway ORTF, notas originales exactas y procedencia
validada. Corpus de diagnóstico ya utilizado; no validación ciega.

| Perfil | Error medio, 16 casos | Peor caso |
| --- | ---: | ---: |
| Base | 6,83418 | 9,06559 |
| Panel | 9,71146 | 12,72176 |
| Panel + puente | 6,13489 | 9,12296 |
| Panel + costillas + puente | 7,33846 | 9,08725 |

Score menor significa menor error en estas medidas, no calidad perceptual
garantizada. Panel + puente mejora el promedio 10,23%, pero empeora el peor
caso y varias medidas de ataque. Con costillas, C4 aumenta mucho de nivel y
sostenido mientras C5 pierde fundamental. No hay una mejora uniforme del teclado.

## Control causal de presión: cambia la atribución

El observador habitual aproxima la transferencia de presión en la frecuencia
propia de cada modo. Un modo forzado por la cuerda puede vibrar a otras
frecuencias; por eso esa aproximación podría sesgar la comparación estructural.
Se repitieron los cuatro casos a velocidad 124 con aceleración y retardos de
propagación de la superficie 40×24, **tanto en la base como en las variantes**.
Es un control dentro del observador Rayleigh de panel rectangular heredado;
no es una solución acústica completa de piano, tapa y sala.

Comparación únicamente entre los mismos cuatro casos fuertes:

| Perfil | Observador habitual | Observador causal |
| --- | ---: | ---: |
| Base | 7,13637 | 8,07326 |
| Panel | 9,77074 | 9,66561 |
| Panel + puente | 6,11377 | 7,13126 |
| Panel + costillas + puente | 7,24789 | 9,21889 |

En C4, Panel + puente baja el error de 9,06559 a 4,24411 con el observador
habitual, pero **lo sube de 6,04755 a 8,03435 con el causal**. La recuperación
aparente no se puede atribuir sólo a mejor física de la tabla.

Con costillas y observador causal, el error del fundamental tardío de C4 baja
de 11,4055 a 2,7275 dB, pero el error del cuerpo del ataque sube de 6,0323 a
13,5851 dB. En C5, el error del fundamental tardío sube de 4,3829 a 29,8830 dB.
Se recupera una parte del sostenido en una nota y se pierde en otra: no justifica
promover el banco ni ajustar individualmente C4/C5 para esconderlo.

Todos los renders medidos son finitos y la salida queda bajo ±1. Eso no
certifica ausencia de saturación interna: el preamplificador heredado puede
colorear antes de ese límite. No se verificaron aún pedales/reiteraciones con
P2, cambios de programa, rendimiento en tiempo real ni WASM.

## Reproducción y archivos

- `tools/concert_grand_orthotropic_plate.py`: solver estructural.
- `tools/audit-concert-grand-orthotropic-plate.py`: controles y bancos.
- `plugins/concert-grand/src/ritz_board.rs`: lector y verificación nativa.
- `tools/audit-concert-grand-ritz-sound.py`: renders, procedencia y métricas;
  `--causal --probe-only` reproduce el control de cuatro notas fuertes.
- Renderer congelado: `local/concert-grand-ritz/build/renderer.exe`, SHA256
  `7cb99ac10cf62eb16bb5bc43ea6ad04d6a24e410717f7e6a81a3d0bc3b453f7f`.
- `source.zip` conserva el código del primer ensayo; `source-causal.zip`
  conserva el código con las opciones del control causal.
- `P2-tabla-estructural`, `P2-tabla-audio`, `P2-tabla-causal` contienen los
  reportes originales y WAVs. `resumen-P2.json` registra hashes y métricas compactas.

Intensidades de la carpeta habitual: 28/60/92/124, tres segundos por entrada.
En la carpeta causal sólo hay velocidad 124. Los comparadores existentes
alternan Base / Panel con costillas y puente / Steinway. Los archivos individuales
permiten escuchar también Panel + puente sin interpretar el comparador.

## Decisión

**Ningún reemplazo de fábrica.** P1b no mejoró consistentemente y P2 muestra
sensibilidad importante a geometría/proyección, con regresiones por registro.
Sigue pendiente verificar la resolución acústica y una geometría global de
puente menos idealizada antes de seleccionar un banco o hacer un refit.
El orden y las físicas P3/P4/P5 siguen registrados en el plan.
