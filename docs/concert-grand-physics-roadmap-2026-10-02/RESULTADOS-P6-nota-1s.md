# P6 — C4/124 sostenido durante un segundo

Fecha: 2026-10-05. Base `f124550`, rama `codex/concert-grand-restrike`.
Por pedido del usuario se extiende la primera nota física para poder
escuchar el cuerpo y el sostenido. Se conservan parámetros, modos y carga;
no se hace un refit ni se modifica el motor de fábrica.

## Fuente y continuidad

Se reutilizan el ataque frío validado de 4 ms y las dos continuaciones
polinómicas de [250 ms](RESULTADOS-P6-coast-exponencial.md), a 64/128 kHz.
Cada tasa continúa desde sus cuatro estados finales: posición, velocidad,
onda y relajación. Los 750 ms nuevos se guardan como segmentos de extensión;
no se copian los prefijos ni el factor acústico de 90 modos seleccionado
en 59. Se comprueba que la energía inicial de cada extensión coincide con
la energía final de su prefijo, hasta 1e−12 J.

La fuente completa de 64 kHz se subdivide exactamente como interpolación
Hermite C1 para evaluarla a 128 kHz junto al ataque. No se reintegra ni se
añaden oscilaciones: es el mismo polinomio continuo entre sus extremos.
Posición y velocidad deben coincidir en ambas uniones hasta 1e−12 relativo.
Las tres pruebas nuevas comprueban la aceleración de una trayectoria cúbica
continua contra integración independiente, ausencia de impulso de unión
para velocidad constante y rechazo de un empalme discontinuo. Pasan.

## Medición y salida

Se compara la presión compleja **de la nota completa** a ambas tasas en
60–6000 Hz sobre todos los bins del FFT de audio, sin ajustar ganancia.
También se comparan posición y velocidad de tabla en la grilla común.
Se mantienen los criterios de 1% de presión/B/V y pérdida inferida no
negativa hasta 1e−9 relativo. La captura de la fuente seleccionada se
comprueba aparte al reducir 128→64 kHz, con límite 0,1%.

El WAV usa la trayectoria más fina, 128 kHz, la superficie y los micrófonos
anteriores, estéreo a 48 kHz y ganancia común **0,25 full scale/Pa**.
Se preserva el retardo de propagación. Se agregan 100 ms iniciales y 300 ms
finales de silencio, y un fade terminal de edición de 8 ms. El fade no
representa soltar la tecla ni el apagador. No hay EQ, sala ni compresor.

Se completan ambas tasas. Pasan los cuatro controles:

| Control | Resultado | Límite |
|---|---:|---:|
| Presión, integración 64 vs 128 kHz | 0,133475% | 1% |
| Posición de tabla, 64 vs 128 kHz | 0,043449% | 1% |
| Velocidad de tabla, 64 vs 128 kHz | 0,095684% | 1% |
| Presión, captura 64 vs 128 kHz de la fuente fina | 0,00002939% | 0,1% |

Todas las diferencias de posición/velocidad en las uniones son exactamente
cero. La energía inicial de cada extensión coincide con el prefijo. No
hubo nuevos contactos ni retornos al tangente completo. Las iteraciones
máximas de la extensión fueron 8/7 a 64/128 kHz.

| Medida desde el fin del ataque (4 ms) hasta 1 s | 64 kHz | 128 kHz |
|---|---:|---:|
| Energía inicial | 0,046906871 J | Igual |
| Energía final | 0,029261723 J | 0,029260742 J |
| Calor material acumulado | 0,013094014 J | 0,013096733 J |
| Radiación inferida acumulada | 0,004551134 J | 0,004549396 J |
| Dinámica de los 750 ms nuevos | 164,203 s | 296,297 s |

Ambos residuos de radiación de la extensión son positivos. El cálculo
completo de los dos controles y la exportación tardaron **555,516 s nuevos**,
incluyendo inicialización (27,313 s), cuatro evaluaciones de almacenamiento,
guardado, FFT y salida. No incluye volver a calcular ataque/prefijos. Una
sola trayectoria de 64 kHz habría requerido unos 219 s de dinámica para
4→1000 ms sumando su prefijo medido; la segunda tasa añade su propio coste.
No se interpreta el tiempo conjunto como coste de un render a una sola tasa.

La fuente seleccionada tiene pico físico 2,021544 Pa. El WAV float32 tiene
pico 0,505248, sin clipping, y duración de archivo 1,403479 s contando los
silencios y la propagación. SHA256 del WAV:
`f58c67734f8b3f35f2a2923390ac1fcc333e710c76fcbe44f2c22e26edd6675c`.
La convergencia 64/128 durante este segundo queda medida; no se extiende
esa aprobación a otras notas, duraciones ni a la física de tres cuerdas.

## Archivos únicos y reproducción

Informe canónico: `medidas-P6-nota-1s/render.json`, con hashes de prefijos,
extensiones, kernel, fuentes y WAV. Fuentes y documentación sólo en el repo.
Datos nuevos y audio solicitado sólo en
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-nota-1s`:

- `Extension-64000.npz`, intervalo 0,25→1 s.
- `Extension-128000.npz`, el mismo intervalo con paso más fino.
- `C4-124-Piloto-fisico-1s.wav`, escucha de la nota completa.

Herramienta: `tools/render-concert-grand-exponential-note.py --duration 1
--gain 0.25`, con los inputs físicos de la primera nota, y además:

| Argumento | Ruta |
|---|---|
| `--prefix-report` | `docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-exponencial/sostenido-250ms.json` |
| `--prefix-data` | `C:/SHARED/Fisica-pendiente-2026-10-05/P6-coast-exponencial-250ms` |
| `--kernel` | `C:/SHARED/Fisica-pendiente-2026-10-05/P6-primera-nota/Microfonos-128k.npz` |
| `--kernel-report` | `docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-primera-nota/microfonos.json` |
| `--output` | `C:/SHARED/Fisica-pendiente-2026-10-05/P6-nota-1s` |
| `--audio` | `C:/SHARED/Fisica-pendiente-2026-10-05/P6-nota-1s/C4-124-Piloto-fisico-1s.wav` |
| `--report` | `docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-nota-1s/render.json` |

Para repetir, elegir destinos nuevos. Python 3.12.14 / NumPy 2.3.5 / SciPy
1.18.1, un hilo BLAS y sin bytecode. Pruebas nuevas:
`python -m unittest discover -s tools/tests -p test_concert_exponential_note.py`.

## Alcance

Es una cuerda vertical C4/124 del piloto no lineal SI, con 72 modos por
dirección transversal, 144 axiales, 59 de tabla y 38232 estados acústicos.
Se conserva el martillo que se aleja después del contacto; no hay unísono,
acción, pedal ni note-off modelado en esta escucha. La radiación angular
larga se infiere de almacenamiento y calor; no se presenta como una integral
independiente de potencia. Siguen pendientes convergencia 59→90, geometría
y pérdidas identificadas, otras notas/velocidades e integración runtime.
El paquete instalado y la primera escucha de 250 ms se conservan.
