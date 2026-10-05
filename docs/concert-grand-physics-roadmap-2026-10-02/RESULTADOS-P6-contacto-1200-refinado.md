# P6: contactos de 59 modos con la carga refinada

Fecha: 2026-10-05. Base: `f124550`, selección/carga de 59 modos e
inicializaciones a resolución acústica común. El paquete 0.172.0 conserva
su calibración. Esta etapa mide el primer banco temporal para la futura
comparación estructural 59→90.

## Configuración congelada

Se mantienen C4/MIDI 60, velocidad 124, una cuerda vertical, 72 modos
transversales y 144 axiales. Los 59 modos físicos de tabla llegan a
1193,507913 Hz. La misma superficie 192×144 y factor a 128 kHz que se
validaron en la carga refinada proporcionan 38232 estados acústicos.
El archivo padre de 90 modos se lee como fuente del prefijo físico, sin
duplicar coeficientes; la relajación se reconstruye en dimensión 59.

Los parámetros de cuerda y fieltro, proyección de puente y pérdidas
materiales se conservan. `BoundedContact` utiliza las mismas coordenadas
mecánicas de masa mixta y los puertos recíprocos C/Cᵀ, sin masa de aire
instantánea. El primer contacto dura 0–4 ms; el segundo, 4–8 ms.
Sólo se relanza el martillo separado: cuerda, tabla y estados de onda y
relajación permanecen vivos. El trabajo externo del relanzamiento se registra.
Es un ensayo de contacto prescrito, no una acción de tecla o MIDI completo.

Se integran ambos contactos a 384 y 768 kHz. La captura de posición y
velocidad físicas de tabla es a 128 kHz; energía almacenada se evalúa
independientemente cada 0,5 ms, con trabajos/pérdidas acumulados por paso.
El fieltro conserva pérdida de tasa cero en este caso; su disipación se
verifica en el fixture independiente del núcleo.

## Controles y umbrales

Se conservan los criterios del banco anterior:

- Presupuesto energético global, de aire y de tabla: relativo menor a 1e−9.
- Cambios al refinar de energía/trabajo, fuerza e impulso: menores a 0,2%.
- Cambio de duración de contacto: menor a 0,1%.
- Diferencias RMS de posición/velocidad de puente y tabla: menores a 1%.
- Secuencia: estados preservados y continuidad/presupuesto acumulado menores a 1e−9.
- Observador: prefijo vivo de 64 pasos, estados y trabajos idénticos bit a bit,
  error relativo de radiación menor a 1e−12 entre cálculo escalar y agrupado.

El prefijo parte del estado vivo relanzado del primer contacto a 384 kHz.
No sustituye una comprobación escalar de las cuatro trayectorias completas.
Agrupar radiación cambia el coste de observación, no las fuerzas o la dinámica.

## Resultados

Los cuatro contactos se completaron. **Pasan los cinco controles del banco**:
energía, refinamiento frío, refinamiento reiterado, secuencia persistente y
equivalencia del prefijo vivo del observador. Se conservan los umbrales.

| Cambio relativo 384 → 768 kHz | Primer golpe | Segundo golpe | Límite |
| --- | ---: | ---: | ---: |
| Posición RMS del puente | 0,002685% | 0,006123% | 1% |
| Velocidad RMS del puente | 0,015692% | 0,037502% | 1% |
| Posición RMS de tabla | 0,002447% | 0,004952% | 1% |
| Velocidad RMS de tabla | 0,010240% | 0,017481% | 1% |
| Máximo de energía/trabajo | 0,076833% | 0,005345% | 0,2% |
| Duración de contacto | 0,000458% | 0,000413% | 0,1% |
| Fuerza máxima | 0,002756% | 0,023250% | 0,2% |
| Impulso | 0,000076% | 0,001170% | 0,2% |

El máximo de energía/trabajo frío corresponde a radiación acumulada, que
es pequeña en esta ventana; el del segundo golpe, a energía final del
martillo. El balance global máximo es 5,81e−13 relativo; de aire, 1,66e−16;
de tabla, 4,14e−16. No se restaura ni recorta energía para obtener estos valores.

| Valor a 768 kHz | Primer golpe | Segundo golpe |
| --- | ---: | ---: |
| Tiempo de separación registrado | 1,816845608 ms | 2,249328929 ms |
| Fuerza máxima | 25,555378 N | 25,680414 N |
| Impulso | 0,023868568 N·s | 0,026045244 N·s |
| Energía final de tabla | 1,43117865e−4 J | 3,33093109e−4 J |
| Energía final de aire | 1,04385984e−5 J | 2,50179658e−5 J |
| Radiación acumulada | 1,27052343e−7 J | 1,51585575e−5 J |

La energía inicial del contacto frío es 0,04693371904404984 J. El segundo
parte de los estados vivos y suma trabajo externo de martillo
0,028325461201695032 J a tasa fina. La continuidad energética en la unión
tiene error relativo 9,22e−17 en ambas tasas; el presupuesto acumulado de
los dos contactos da 2,67e−15 a 384 kHz y 5,18e−13 a 768 kHz.

El prefijo vivo de 64 pasos dura 0,166667 ms. Conserva exactamente los
estados y trabajos entre observadores, con diferencia relativa de radiación
3,55e−15. Parte de estados de onda/relajación no nulos y no modifica la
trayectoria usada por el banco.

Las integraciones suman 1084,813 s, unos 18,1 minutos, además de carga y
comprobación escalar del prefijo. Los cuatro NPZ ocupan unos 3,28 MB. El
núcleo y el selector mantienen sus fuentes verificadas en la etapa anterior;
esta etapa ejecuta sus controles sobre las cuatro trayectorias reales, sin
recalibrar parámetros ni ampliar pruebas redundantes.

Este resultado valida refinamiento temporal y conservación en esta
configuración. No mide todavía la diferencia de presión entre 59 y 90
modos, ni afirma convergencia estructural o acercamiento audible al Steinway.

## Archivos y reproducción

Medidas y procedencia tienen única copia en el repo, en
`medidas-P6-contacto-1200-refinado/`; resumen:
`resumen-P6-contacto-1200-refinado.json`. Las trayectorias NPZ únicas se
guardan en `C:/SHARED/Fisica-pendiente-2026-10-05/P6-contacto-1200-refinado`.
No se generan WAV, CSV de esas trayectorias, informes en SHARED ni otro
archivo de coeficientes.

```powershell
python tools/audit-concert-grand-bounded-contact.py --input C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --coefficients C:/SHARED/Fisica-pendiente-2026-10-05/P6-tabla-1500-refinada/Wave128000-Cells192x144-Angle96x96-Modes90.npz --mass C:/SHARED/Fisica-pendiente-2026-10-05/P6-tabla-1500-refinada/Wave128000-Cells192x144-Angle96x96-Modes90.npz --cells 192 144 --cutoff 1200 --transverse 72 --rates 384000 768000 --load-report docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-tabla-1200-refinada.json --relaunch --observer-probe-steps 64 --baseline-commit f124550 --output C:/SHARED/Fisica-pendiente-2026-10-05/P6-contacto-1200-refinado --reports docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-1200-refinado --summary docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-contacto-1200-refinado.json
```

El banco rechaza reemplazar casos medidos. Para repetir integración, usar
destinos nuevos. `--phase summary` recalcula comparaciones del banco
existente sin repetir trayectorias. Python 3.12.14, NumPy 2.3.5,
SciPy 1.18.1 y un hilo BLAS; las huellas de inputs, fuentes y datos se
conservan en el resumen.

## Alcance y siguiente

Este banco sigue limitado a C4/124 y dos contactos cortos. No identifica
pérdidas reales ni la geometría del Steinway. Sigue el banco equivalente de
90 modos, comparado con estas trayectorias a la misma resolución, y después
observación SI sobre la superficie refinada. Sostenido, banco de notas/
intensidades, escucha, ambas caras/borde, unísono, P4, graves, acción/pedal
y reducción de coste para el plugin permanecen abiertos.

### Estado de escucha

El usuario preguntó si ya se puede escuchar este modelo nuevo. **Todavía
no hay una nota sostenida ni un `.rfplugin` que use esta física**. El banco
actual abarca una cuerda y 4/8 ms de contactos; cortar esa ventana para
convertirla en WAV no representa su sostenido y puede introducir un
transitorio del propio corte. El paquete instalado conserva el sonido anterior.

Para una primera escucha útil falta prolongar la fuente física y observar
su presión temporal con la superficie y grilla correctas. El primer objetivo
audible debe ser una nota sostenida controlada, antes de piezas MIDI o de
integración en fábrica. Se mantiene esta limitación explícita al interpretar
los controles numéricos.
