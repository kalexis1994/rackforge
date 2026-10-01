# Patrón de afinación del unísono — candidato 11

## Estado

El usuario prefirió ligeramente la audición 10. Se congelaron su ejecutable y
procedencia en `local/concert-grand-coherent-unison/frozen-10` y se usó como
referencia de esta etapa. Los doce WAV de control C4/F4/G4/C5 × velocidades
60/84/124 siguen siendo idénticos, byte por byte, a los del render de la etapa 10.

Se preparó el candidato 11 para escuchar. Reduce indicadores de recurrencia
respecto al 10 y conserva su cuerpo en los primeros segundos. No pasó todas
las guardas: D#4 a velocidad 36 pierde 2,38 dB entre 1,5–3,5 s y F4 pierde
7,61 dB entre 7,5–11,5 s. No se instaló ni se eligió como reemplazo definitivo.

## Qué se cambió

Una única regla de afinación para todo el teclado: la cuerda intermedia se
aproxima a una de las externas, conservando la separación de las externas y el
centro geométrico de afinación del conjunto. El parámetro efectivo
`STRING_TUNING_CENTER` pasa de 0,5 a 0,25. Se mantienen la pequeña variación
determinista por tecla y la ley de inarmonicidad propia de cada cuerda.

No se añadieron pérdidas para apagar los batidos. El cambio afecta a cómo se
excitan y decaen los modos colectivos con el acoplamiento que ya tiene el 10.
Es un parámetro de afinación exploratorio, no una medida identificada del
Steinway. La sensibilidad de la doble caída a diferencias muy pequeñas de
afinación se ilustra en los cálculos de [Woodhouse](https://euphonics.org/7-3-multiple-strings-and-double-decays/).

Se exploraron centros 0,15/0,25/0,35/0,65/0,75/0,85 y se añadió 0,20 después.
El 0,15 ganó la primera selección por recuento, pero mostró regresiones al
ampliar la prueba. Se eligió 0,25 para esta audición por el equilibrio de
recurrencia, profundidad y conservación de nivel en los dos grupos. Las notas
adicionales se reutilizaron para refinar la elección; ya no son una validación
ciega. Sigue pendiente otra validación independiente antes de promoverlo.

## Resultados relativos a 10

Todos los mínimos usan recuperación de al menos 3 dB, con el mismo conjunto de
parciales y pisos de detección. No equivalen a defectos audibles independientes.

| Corpus, ventana 1,5–3,5 s | Mínimos 10 → 11 | Suma de profundidades 10 → 11 | Cambio medio de nivel |
|---|---:|---:|---:|
| C4/F#4/A4/C5, velocidades 60/84/124 | 27 → 23 | 282,43 → 189,56 dB | −0,05 dB |
| D#3/A3/D#4/D#5/F#5/C6, velocidades 36/108 | 17 → 14 | 168,52 → 93,26 dB | +1,15 dB |

La suma de profundidades es un diagnóstico; no es una reducción en dB del
sonido completo. La nota adicional con mayor pérdida fue D#4 a velocidad 36.
Su nivel medio en esa ventana pasó de aproximadamente −59,13 a −61,51 dBFS,
con la misma ganancia. Falló la guarda de pérdida máxima de 2 dB.

Se renderizaron además C4/F4/G4/C5 a velocidad 84, doce segundos sostenidas,
seguidos de una liberación real y dos segundos de cola. En esas cuatro notas:

| Ventana | Mínimos 10 → 11 | Cambio medio de nivel |
|---|---:|---:|
| 0,5–1,5 s | 16 → 12 | +0,001 dB |
| 1,5–3,5 s | 17 → 10 | +0,91 dB |
| 3,5–7,5 s | 7 → 4 | +1,14 dB |
| 7,5–11,5 s | 0 → 0 | −2,09 dB |

F4 sube 1,03 y 2,10 dB en las dos ventanas centrales, pero cae 7,61 dB en la
última. Esto es una redistribución temporal con una regresión tardía, no una
mejora completa de la cola. El piso de detección puede excluir mínimos débiles;
el cero final no demuestra ausencia de batidos. F4/G4 no se compararon con
originales transpuestos. El archivo largo permite juzgar la cola y el release.

## Prueba espacial que no se incorporó al 11

Se implementó una aproximación local con fuerza común y primer momento de las
fuerzas de las cuerdas. La matriz de conductancias es una suma de productos
externos positivos; su reacción se integra mediante una contracción y rotación
en dos coordenadas ortogonales. La radiación añade el mismo momento al gradiente
de las formas modales existentes, conservando la fuerza neta.

Es una aproximación espacial lineal sobre la interpolación actual de una línea
de puente asumida de 1,4 m, no una geometría medida ni una resolución completa
de cuerda y tabla. Se ensayaron separaciones adyacentes de 3/6/9/12 mm como
hipótesis. Se mantuvo pasividad y consistencia entre proyección y reacción.

Con 6 mm, los mínimos cambiaron de 27 a 26, la profundidad total de 282,43 a
283,81 dB y el nivel medio tardío +0,02 dB. No mostró una mejora convincente.
El 11 mantiene `SPATIAL_UNISON_GAP_M = 0`; esta implementación está solo en el
prototipo y desactivada para la audición. No atribuir al instrumento real las
limitaciones de esa geometría simplificada.

## Archivos y verificación

En `C:\SHARED`:

- `10/11-UNISONO-intensidades.wav`: C4/F4/G4/C5, cada nota a velocidades 60/84/124;
  cuatro segundos sostenidos por fragmento y 0,4 segundos de silencio.
- `10/11-UNISONO-sostenidas-12s.wav`: las cuatro notas a velocidad 84, doce
  segundos sostenidos, dos segundos de release y 0,4 segundos de silencio.
- `UNISONO-experimento.json`, `UNISONO-LEEME.txt` y `11-UNISONO-modelo.json`.

Estos archivos 10 y 11 usan una única ganancia y trim de −0,2593 dB. No se
normalizaron notas ni perfiles por separado. Los WAV originales de la etapa
10 siguen disponibles con su trim anterior de −0,2721 dB.

100 pruebas nativas aprobadas, 74 especiales ignoradas por la ejecución normal.
Se comprobaron contracción de la matriz espacial, respuesta de la coordenada
común, equivalencia entre fuerzas separadas y su momento, y conservación de
afinación y separación al variar el centro. La prueba especial de reiteraciones
y carga compartida pasó con el 11 en 44,1/48/96 kHz y bloques 256/512. Los dos
fingerprints predeterminados se conservaron. Los cuatro WAV exportados son
finitos, PCM24 a 44,1 kHz y con pico inferior a 0,9.

El ejecutable y metadatos del 11 se congelaron en `frozen-11`. `research.patch`
preserva el código respecto al worktree histórico; los CSV y JSON de esta
carpeta preservan resultados y hashes. Las pruebas no confirman fidelidad
perceptiva ni rendimiento ASIO en vivo. La aceptación del 11 queda pendiente de
escucha y de resolver sus regresiones de cola.
