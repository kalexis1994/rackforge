# P6: contacto SI con 59 modos de tabla y aire persistente

Fecha: 2026-10-04. Base: `1bfad0a`, carga ampliada verificada en
[el ensayo de 59 modos](RESULTADOS-P6-tabla-1200.md). El paquete 0.172.0 no
cambia en esta investigación offline.

## Construcción y comparación congelada

Se reutiliza `BoundedContact`; se amplían su soporte físico P2 y la misma
proyección recíproca C/Cᵀ a los 59 modos bajo 1200 Hz. El orden de frecuencias,
la masa estática y la resistencia límite deben coincidir con el coeficiente
verificado. No se añade masa instantánea de aire. El puerto conserva 19293
estados de onda/relajación y los 9216 canales del factor de radiación.

Se mantienen C4/MIDI 60, velocidad 124, 72 modos transversales, 144 axiales,
los parámetros SI de cuerda y martillo, la proyección de puente y pérdidas
0,046 del soporte anterior. El fieltro tiene pérdida de tasa cero en este
caso; la disipación del fieltro se verifica en un fixture independiente.
Cambiar de base modifica la respuesta mecánica sin reajustar la dureza,
añadir detune ni igualar el nivel de cada nota.

El primer golpe transcurre durante 0–4 ms. Después se relanza sólo el
martillo separado; se mantienen exactamente cuerda, tabla y ambos estados
acústicos durante el segundo golpe, 4–8 ms. El trabajo externo del martillo
se registra. Es una repetición de contacto prescrita, no una acción de tecla.
Se ejecutan ambos contactos a 384 y 768 kHz, con captura a 128 kHz y
almacenamiento energético independiente cada 0,5 ms. Trabajo y pérdidas se
acumulan en cada paso; el fixture comprueba también identidades por paso.

Se congelan los umbrales previos: balance global/aire/tabla 1e−9;
energía/trabajo, fuerza e impulso 0,2%; duración de contacto 0,1%;
trayectorias RMS de puente y tabla 1%. La nueva base no cambia esos límites.

## Observador con estados vivos

Una comprobación adicional parte del estado del primer golpe después del
relanzamiento, con memoria acústica viva. Avanza 64 pasos a 384 kHz, una vez
con radiación escalar y otra agrupada, sin modificar el estado usado para
el banco. Compara todos los estados y trabajos mecánicos/acústicos en cada
paso, y la radiación con tolerancia relativa 1e−12. Su alcance es un prefijo
de 0,166667 ms; no equivale a una comparación escalar de las cuatro trayectorias.
La equivalencia escalar completa anterior sigue referida a 35 modos.

La prueba de la nueva base pasa: estados y trabajos idénticos bit a bit en
cada paso; error relativo de radiación 3,78e−15. La memoria inicial de ondas
y relajación es no nula. Pasaron también los 16 tests dirigidos, cinco de
contacto, siete de aire racional y cuatro de aire limitado.

## Sensibilidad a la base estructural

Se contrasta con los cuatro casos congelados de
`resumen-P6-contacto-compacto.json`. Los parámetros físicos, sus entradas,
el asset y las huellas de las trayectorias deben coincidir antes del ensayo.
Se comparan duración/fuerza/impulso, partición energética y trayectorias del
puente y los 35 modos comunes. Se registra la energía final de los 24 modos
adicionales en sus coordenadas físicas de masa normalizada.

Este contraste mide sensibilidad a truncar la base de tabla. Sus diferencias
no son un error de refinamiento temporal ni un score contra el Steinway.
Un solo escalón 35→59 no demuestra convergencia estructural global.

## Resultados de los cuatro contactos

Los cinco gates pasan: energía, refinamiento frío, refinamiento reiterado,
continuidad de la secuencia persistente y equivalencia del prefijo del
observador. Los umbrales se mantuvieron sin cambios.

| Cambio 384 → 768 kHz | Primer golpe | Segundo golpe | Límite |
| --- | ---: | ---: | ---: |
| Velocidad RMS del puente | 0,015692% | 0,037503% | 1% |
| Posición RMS del puente | 0,002685% | 0,006123% | 1% |
| Velocidad RMS de tabla | 0,010241% | 0,017483% | 1% |
| Posición RMS de tabla | 0,002447% | 0,004952% | 1% |
| Mayor cambio de energía/trabajo | 0,070808% | 0,005345% | 0,2% |
| Duración del contacto | 0,000458% | 0,000413% | 0,1% |
| Fuerza máxima | 0,002756% | 0,023250% | 0,2% |
| Impulso | 0,000076% | 0,001170% | 0,2% |

La duración fina es 1,816846 ms / 2,249329 ms. El trabajo externo del
relanzamiento es 0,0283254612 J; la continuidad energética cierra a cero
al redondeo guardado en ambas tasas, con estados preservados bit a bit.
El presupuesto acumulado de los dos contactos da 9,22e−17 / 5,14e−13.
Máximos de los cuatro casos: global **5,82e−13**, aire **1,56e−16**,
tabla **3,86e−16**, con el alcance de checkpoints indicado arriba.

| Partición al final de cada contacto, 768 kHz | Primer golpe | Segundo golpe |
| --- | ---: | ---: |
| Energía de cuerda, J | 0,0281450549 | 0,0463587881 |
| Energía de tabla, J | 0,0001431281 | 0,0003331025 |
| Energía de martillo, J | 0,0186082578 | 0,0284288513 |
| Energía almacenada en aire, J | 0,0000104239 | 0,0000250501 |
| Pérdida material del contacto, J | 0,0000267237 | 0,0000714305 |
| Radiación acumulada del contacto, J | 0,0000001306 | 0,0000151035 |
| Energía final en los 24 modos nuevos, J | 0,0000035751 | 0,0000084441 |

## Diferencia frente a 35 modos

Comparación a la tasa fina con los parámetros y eventos idénticos; porcentajes
RMS de diferencias para trayectorias y cambios firmados para cantidades finales.

| Cambio 35 → 59 modos, 768 kHz | Primer golpe | Segundo golpe |
| --- | ---: | ---: |
| Velocidad RMS del puente | 19,361732% | 16,732672% |
| Posición RMS del puente | 6,501176% | 4,843677% |
| Velocidad RMS de los 35 modos comunes | 0,289493% | 0,295166% |
| Energía final de tabla | +2,477252% | +2,518698% |
| Trabajo hacia la tabla | +4,802041% | +3,957393% |
| Radiación acumulada del contacto | +5,834272% | +6,407864% |

El primer contacto conserva fuerza, duración e impulso hasta redondeo, mientras
cambia la respuesta del soporte. En el segundo, la fuerza máxima cambia
−0,029218% y el impulso −0,007773%. Los 24 modos adicionales reciben energía.
La diferencia de velocidad local supera ampliamente el error temporal medido:
la truncación de tabla sí importa en este ensayo, aunque su energía total
cambie mucho menos. Eso no certifica convergencia con 59 modos.

La velocidad del punto del puente no es la presión captada por un micrófono.
El siguiente control será observar en SI las dos trayectorias, con la misma
geometría, distancia y ganancia, para medir cuánto de esta diferencia local
se convierte en presión radiada. Sólo después corresponde decidir más
resolución estructural, extender notas/intensidades y preparar escucha.

## Archivos y reproducción

Medidas, procedencia, comprobación del observador y resumen tienen su única
copia en el repo: `medidas-P6-contacto-compacto-1200/` y
`resumen-P6-contacto-compacto-1200.json`. Las cuatro trayectorias NPZ únicas
se guardan en `C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto-1200`.
No se duplican los coeficientes de aire ni se generan CSV de los NPZ,
informes en SHARED, ZIP de fuentes o audio no solicitado.

```powershell
python tools/audit-concert-grand-bounded-contact.py --input C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --coefficients C:/SHARED/Fisica-pendiente-2026-10-04/P6-tabla-1200/Wave64000-Cells128x96-Angle96x96-Modes59.npz --mass C:/SHARED/Fisica-pendiente-2026-10-04/P6-tabla-1200/Wave64000-Cells128x96-Angle96x96-Modes59.npz --output C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto-1200 --transverse 72 --rates 384000 768000 --relaunch --cutoff 1200 --baseline-commit 1bfad0a --load-report docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-tabla-1200.json --basis-reference docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-contacto-compacto.json --basis-data C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto/batched --observer-probe-steps 64
```

Usar Python/NumPy/SciPy del entorno registrado, con un hilo BLAS. Para repetir
desde cero, escoger destinos nuevos mediante `--output`, `--reports` y
`--summary`: el banco rechaza sobrescribir casos medidos. `--phase summary`
recalcula las comparaciones del banco existente sin repetir la integración.
Los informes guardan versiones, huellas y el alcance de cada control.

## Alcance

El banco sigue siendo una cuerda vertical C4/124 durante dos contactos cortos.
No identifica por sí solo las pérdidas reales, la geometría del Steinway,
la resonancia de ambas caras/borde ni el unísono. La observación SI para
escucha y el banco de notas/intensidades siguen pendientes, junto con P4,
graves y acción/pedal. No se infiere una mejora audible de los balances.
