# Cuerpo del golpe señalado en 00:57

## Identificación

Es C6 (MIDI 84), velocidad 124, el cuarto golpe del grupo C6 en las tres
secuencias de 72 s. Los extractos se toman directamente de 57–60 s de los WAV
entregados; no se reconstruye otro golpe. Se comparan las raíces originales,
sin transposición, del mismo sampler Steinway D Stage/Lid ORTF histórico.
El experimento de movilidad sigue siendo no aprobado, y el plugin instalado
no se modifica.

## Diferencias medidas

Se usa energía de ambos canales, sin sumarlos a mono y sin igualar niveles
por nota. Las ventanas se alinean al inicio energético; en C6/124 éste es
0 ms en el modelo y aproximadamente 1 ms en el sampler. La herramienta
conserva también los inicios con umbrales −50/−40/−30 dB. Estas diferencias
pequeñas de alineación no se interpretan como tiempos físicos de martillo.

| Medida | Actual | Movilidad experimental | Steinway |
| --- | ---: | ---: | ---: |
| H1–H3, primeros 20 ms | −17,68 dB | −17,00 dB | −16,45 dB |
| H2/H1, 20–50 ms | −27,26 dB | −26,10 dB | −15,93 dB |
| Banda 180–600 Hz, primeros 20 ms | −53,69 dB | −53,57 dB | −34,75 dB |
| Banda 600–900 Hz, primeros 20 ms | −43,06 dB | −43,1 dB | −30,69 dB |

La fundamental domina H1–H3 en esta nota: el nivel inicial del núcleo tonal
está relativamente próximo, mientras H2 tiene aproximadamente **11,3 dB menos
presencia respecto a H1** en el modelo actual (10,2 dB menos en el experimento).
El Steinway tiene además energía transitoria por debajo de la fundamental,
que aquí es aproximadamente 1046 Hz. Por lo tanto, subir el volumen global
no igualaría la distribución espectral. Esto es compatible con la percepción
de un golpe con poco cuerpo, sin asignar todo ese cuerpo a una única fuente.

La banda subfundamental incluye presión radiada por otras resonancias y
posibles componentes mecánicos/de captación de la grabación. Estos WAV no
permiten identificar su origen de forma única. No se introduce ruido grave
para satisfacer una métrica ni se identifica material del fieltro a partir
de ella. Los valores son niveles de energía espectral bajo este procedimiento,
no fuerzas, distancias ni intensidades acústicas calibradas en SI.

## Localización dentro del motor

Se captura un golpe nuevo de C6/124 con el renderer congelado
`57a6c88d490e527f63ee9d9cfcdbb44a8bcd6b94f148a1c04a1fe7ca8fcb12e9`,
programa Concert, 44,1 kHz, 256 cuadros y preparación de 200 ms.
Los probes registran las etapas y el contacto sin alterar sus coeficientes.

En 20–50 ms, H2/H1 de la fuerza sumada de las cuerdas es −19,54 dB;
el drive total de tabla da −19,58 dB. La observación directa de la tabla da
−36,22 dB en una cápsula y −23,59 dB en la otra. La salida estéreo termina
alrededor de −27,26 dB. Las etapas tienen unidades distintas: se comparan
**relaciones de parciales dentro de cada señal**, no amplitudes absolutas.

Esto localiza una atenuación relativa importante después de la excitación
de cuerdas, en la respuesta/observación de tabla; no demuestra que una
geometría concreta de micrófonos o un único coeficiente sea el causante.
Las frecuencias propias, sus formas espaciales, la propagación y la carga
contribuyen a esa transferencia. El análisis del martillo y del camino
acústico deben mantenerse separados.

H2 y H3 del golpe no se retiran por el umbral de componentes: las trazas
registran amplitudes post-contacto 0,03593 y 0,00725, ambas marcadas por encima
del piso de culling. Las tasas de carga por cuerda de H1/H2 son aproximadamente
1,282/1,175 por segundo. No hay evidencia aquí de que se haya borrado H2 o de
una mayor tasa de carga de H2 que explique por sí sola toda la diferencia.

## Ablación de la transmisión del contacto

Se ensayan C5/C6/F#6 a 60/124, seis pares, con el mismo renderer y raíces
grabadas. Sólo cambia la ruta feed-forward `ContactPulse`, sin cambiar el
fieltro, detune o densidad. La configuración actual tiene ganancia 0,006 y
dos polos de paso bajo a 180 Hz; no es un cierre mecánico recíproco completo.

| C6/124, primeros 20 ms | 25–180 Hz | 180–600 Hz | H2/H1, 20–50 ms |
| --- | ---: | ---: | ---: |
| Actual, esquina 180 Hz | −37,75 dB | −53,69 dB | −27,26 dB |
| Sin transmisión del contacto | −64,72 dB | −61,48 dB | −27,26 dB |
| Esquina 360 Hz | −36,25 dB | −48,76 dB | −27,26 dB |
| Esquina 600 Hz | −35,91 dB | −46,68 dB | −27,26 dB |
| Steinway | −39,40 dB | −34,75 dB | −15,93 dB |

Ampliar esa ruta recupera cerca de 7 dB en 180–600 Hz, todavía deja cerca de
12 dB de diferencia y aumenta el extremo grave que ya estaba por encima de
la referencia. No recupera H2. Por eso no se aplica la esquina de 600 Hz
como solución global ni se modifica una sola tecla. No es una nueva
calibración aceptada.

## Cambio concreto en el refit

`tools/fit-concert-grand-current-physics.py` añade **score version 2**, por
defecto. Mide el cuerpo tonal del impacto desde 20 ms, con una ventana de
80 ms o al menos ocho ciclos de la fundamental para resolver mejor el grave.
H1, H2 y H3 se observan individualmente, con una máscara fija de referencia
a −30 dB. En C6/124 H2 es fiable; H3 no aporta evidencia suficiente en esa
ventana y no se inventa un objetivo a partir de él.

El componente de impacto combina nivel de H1 (25 %) y error de las relaciones
fiables H2/H1 y H3/H1 (75 %). Aporta 25 % del total nuevo, y el score anterior
aporta 75 %. Tiene su propia guarda de peor caso con tolerancia de 0,5 dB.
Estos pesos son de ingeniería; no constituyen una validación perceptiva.
El subfundamental se mantiene como diagnóstico, no se usa como objetivo para
recompensar artificialmente ruido mecánico o un bass boost.

En C6/124, el nuevo componente mide **9,93 dB de error**, con −12,12 dB de
error de H2/H1 en 20–100 ms. Ampliar sólo `ContactPulse` deja ese componente
prácticamente igual. La versión 1 del total queda accesible con
`--score-version 1`; sus componentes anteriores se conservan sin alteración.
Se comprobó igualdad exacta de `legacy_total` con los seis resultados previos
de la ablación (diferencia máxima 0). No se comparan numéricamente scores
de distinta versión como si usaran la misma escala.

Se incrementa la versión de las features y firmas de caché: los targets y
mediciones anteriores no se reutilizan como si contuvieran este nuevo término.
El informe histórico del refit global sigue describiendo versión 1 y requiere
ese argumento para repetir sus ponderaciones.

## Archivos

`C:/SHARED/Refit-global-2026-10-01/C6-golpe-57s` contiene:

- `C6-v124-actual.wav`, `C6-v124-movilidad-regional.wav`, `C6-v124-Steinway.wav`:
  los tres extractos de 57–60 s, sin normalización.
- `mediciones.json`: doce pares C5/C6/F#6 a 28/60/92/124, tres perfiles,
  ventanas de ataque y cola, hashes, inicios y resumen de stages.
- `diagnostico/`: audios y reporte de las cuatro ablaciones de transmisión.
- `transmision/`: ventanas detalladas de las ablaciones.
- `score-v2.json`: seis pares evaluados con el término nuevo de impacto.

Los CSV completos y el renderer se conservan en `local/` fuera de Git.
Herramienta de análisis: `tools/audit-concert-grand-impact-body.py`.
Ejemplo:

```powershell
python tools/audit-concert-grand-impact-body.py --input C:/SHARED/Refit-global-2026-10-01/notas --output C:/SHARED/Refit-global-2026-10-01/C6-golpe-57s --stage-dir local/concert-grand-impact-body/stages
```

Queda localizado un caso de aceptación útil para la próxima corrección
integral de la transferencia acústica y del ataque; todavía no se declara
resuelto el cuerpo del instrumento. No hay commit ni paquete nuevo.
