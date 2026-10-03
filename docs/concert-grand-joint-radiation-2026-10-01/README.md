# Calibración conjunta de tabla y radiación

## Alcance físico

El ensayo usa el motor completo: cuerdas, contacto, carga de puente, tabla,
observación, fuentes auxiliares y sala. Conserva las masas positivas compartidas
por movimiento y carga. No añade EQ por nota ni ruido de golpe para llenar
el cuerpo; duplex, knock, thump y detune siguen desactivados.

La base espacial sigue siendo una tabla reducida. No se ha identificado la
geometría, los materiales o los micrófonos reales del Steinway a partir de
grabaciones editadas. Los coeficientes ajustados son una calibración de esta
representación, no constantes materiales deducidas de forma única.

## Corrección verificada

La observación anterior trataba Mic Distance como altura sobre la tabla.
La sala lo trataba como alcance lateral y calculaba otra altura: a 0,6 m de
distancia, aproximadamente 0,12 m sobre la tabla. `BOARD_ROOM_GEOMETRY=1`
transfiere las posiciones y ejes polares de las cápsulas de sala a la tabla,
incluyendo el signo del lóbulo posterior. Un único trayecto mínimo se retira
de todos los modos y ambas cápsulas. No se alinean las fases por modo.

Esto unifica las coordenadas de recepción. No sustituye todas las rutas
auxiliares ni la fuente aproximada de sala por un problema acústico completo.
El interruptor permanece apagado por defecto y no cambia el paquete instalado.

## Comprobaciones numéricas

- 132 pruebas nativas aprobadas, 75 ensayos offline ignorados.
- Compilación WASM aprobada y 2 pruebas de equivalencia entre instancias aprobadas.
- Las huellas de audio de fábrica se mantienen.
- Búferes de retardos dimensionados por soporte; crecimientos y reset conservan
  respectivamente historia y núcleo. Se prueba el alcance hasta 192 kHz.
- Suma compartida contrastada con convoluciones independientes, incluyendo
  128 modos con cancelación y acumulación f64. No hay asignación en el tick.
- Frente al observador individual f32, el render C6/110 cambia 0,0267 % en
  norma relativa; la diferencia RMS es −107,64 dBFS. No se declara igualdad
  bit a bit entre estos dos órdenes de suma.
- La malla 40×24 es insuficiente para este micrófono cercano: un componente
  cambia 4,389 dB al duplicarla. Se calibra con 80×48; frente a 160×96 el
  cambio máximo de H1..H6 sobre 21 alturas C2..C7 es 0,338 dB.
- Retardos lineales y aceleración por diferencia introducen hasta 3,408 dB
  frente a la integral continua en este ensayo, especialmente por encima
  de 10 kHz. Esto sigue siendo una aproximación discreta explícita.
- Capturas de 1,7 s dan exactamente las mismas features que las de 4 s en
  54 pares comprobados. Las ventanas terminan antes de 1,5 s. Las audiciones
  conservan cuatro segundos y release; no se mide release con la captura corta.

El FIR es un prototipo nativo para investigación. La suma compartida reduce
su costo, pero no constituye una implementación portable lista para el plugin.

## Diseño del ajuste

Primero se probaron 32 perfiles en 12 pares; los cuatro finalistas se
extendieron a 42 pares. El mejor promedio cayó de 6,174 a 4,860 en el piloto,
pero empeoró notas intermedias al ampliar el teclado. Otra búsqueda de 16
perfiles con frecuencias de tabla fijas tampoco pasó las guardas.

La búsqueda final usa la geometría corregida y malla duplicada: 16 puntos
Sobol, un centro y un control causal. Hay una sola sensibilidad de presión;
se ajustan distancia, pérdida, mezcla espacial, movilidad media, razones
suaves grave/agudo y pérdida extra de unísono. Densidad de modos fija en 1.
No se cambian fieltro, ataque, afinación ni parámetros por nota.

Entrenamiento: 21 raíces grabadas exactas de C2 a C7, cada tres semitonos,
a velocidades 60 y 110. Validación reservada de esta búsqueda: 28, 92 y 124
sobre las mismas alturas. Son reservas internas del corpus ya conocido,
no grabaciones independientes ni una evaluación ciega.

Score v2: niveles/decays de H1, relaciones H2..H6/H1, bandas del ataque y
cuerpo, y H1/H2/H3 en 20–100 ms (ventana mayor en graves). Las guardas
exigen mejora media, por registro y peor caso, con límites de regresión
por par. Estas métricas no sustituyen la audición.

## Reproducción

Renderer congelado: SHA256
`32de924a5150f9c0c54f16d688417303370ef0a53e52173050c133177e3335d3`.
Datos, versiones intermedias y raw audios:
`local/concert-grand-joint-radiation/`. No se sobreescribieron resultados
anteriores. Ganancia de exportación 0 dB; sin normalización por nota.

```powershell
python tools/fit-concert-grand-radiation.py --output local/concert-grand-joint-radiation/near-grid2-profiles.json --center docs/concert-grand-joint-radiation-2026-10-01/center.json --fixed-density 1 --room-geometry --shared-fir --quadrature-scale 2 --count 16
python tools/fit-concert-grand-current-physics.py --binary local/concert-grand-joint-radiation/build/precise-renderer.exe --expected-sha256 32de924a5150f9c0c54f16d688417303370ef0a53e52173050c133177e3335d3 --work local/concert-grand-joint-radiation/near-grid2-render --output local/concert-grand-joint-radiation/near-grid2-training --profiles local/concert-grand-joint-radiation/near-grid2-profiles.json --notes 36,39,42,45,48,51,54,57,60,63,66,69,72,75,78,81,84,87,90,93,96 --velocities 60,110 --measure-seconds 1.7
```

## Resultado final: rechazado

Finalizado el 2 de octubre de 2026. Ninguno de los 18 perfiles experimentales
pasa las guardas. Se congeló `joint-radiation-013`, el menor objetivo entre
los experimentales, **solamente para diagnóstico y audición**. El menor
objetivo global sigue siendo el actual. La selección se registró antes de
evaluar las 63 combinaciones reservadas; no se retocaron parámetros después.

| Score v2; menor es mejor | Actual | Experimental |
| --- | ---: | ---: |
| Entrenamiento: media, 42 pares | 5,516 | 6,241 |
| Entrenamiento: peor par | 9,078 | 12,826 |
| Validación: media, 63 pares | 5,410 | 6,483 |
| Validación: peor par | 9,066 | 14,152 |
| Validación: registro grave | 4,801 | 6,929 |
| Validación: registro medio | 5,852 | 5,713 |
| Validación: registro agudo | 5,727 | 6,635 |

La regresión máxima reservada es 7,775 puntos de score, en D#3/28.
Sólo el registro medio mejora ligeramente. Incluso donde se recupera H2,
no se consigue al mismo tiempo el nivel y la caída correctos de H1.

| C6/124, 20–100 ms | Actual | Experimental | Steinway |
| --- | ---: | ---: | ---: |
| H1, dB | −20,430 | −25,149 | −17,066 |
| H2, dB | −48,389 | −34,976 | −32,902 |
| H2/H1, dB | −27,960 | −9,827 | −15,837 |

El segundo armónico sube unos 13,4 dB, pero H1 pierde otros 4,7 dB. La
relación sobrepasa la referencia mientras la fundamental queda 8,1 dB corta.
Esto no constituye una recuperación integral del cuerpo.

La cuadratura se comprobó también en este perfil seleccionado: 80×48 frente
a 160×96 difiere hasta 1,352 dB, en A6/H6 a 10,56 kHz; por debajo de 5 kHz
el máximo es 0,143 dB. La discretización temporal difiere hasta 4,298 dB.
Los límites de 0,338/3,408 dB anteriores describen el centro congelado, no
garantizan precisión uniforme en toda la búsqueda. No se promociona el
prototipo ni se supone que la malla explique todas sus regresiones.

`results.json` conserva métricas, guardas y selección; `profiles.json` los
perfiles exactos; `selected-transfer.json` la convergencia del seleccionado;
`verification.json` la comprobación de capturas cortas y orden de suma.
Las variantes de geometría, masas y pérdidas probadas aquí no permiten una
mejora global con esta representación. Esto descarta estas calibraciones;
no demuestra un límite de la síntesis física ni identifica por sí solo
qué extensión de la base espacial resolverá todas las diferencias.

## Audición

Carpeta `C:/SHARED/Tabla-radiacion-conjunta-2026-10-01`:

- `Experimental-intensidades.wav`: prototipo solo.
- `actual-intensidades.wav`: modelo actual.
- `Steinway-intensidades.wav`: referencia de muestras exactas, sin transposición.
- `C6-fuerte-*.wav`: extractos individuales de la nota problemática.
- `selection.json`, `report.json`, `audition.json`, `LEEME.txt`: parámetros,
  fuentes, métricas y agenda reproducibles.

Las secuencias duran 72 s: F#2, C3, C4, C5, C6 y F#6, cada una a velocidades
28, 60, 92 y 124. C6/124 empieza a 57 s. Hay 2 s de tecla, 0,5 s de release
y 0,5 s de silencio por pulsación. El promedio del pequeño conjunto audible
sí baja de 5,885 a 5,737; esa mejora no se generaliza a las 21 alturas, como
muestra la validación. Por eso no se usa este subconjunto para aprobarlo.

No se hizo commit, empaquetado ni instalación. Los interruptores de fábrica
siguen apagados y las dos huellas de audio conservan exactamente
`232b085ed4248398` y `4371ab4adc1542ab`.

## Fundamento publicado

- [Elie, Cotté y Boutillon (2022)](https://www.pure.ed.ac.uk/ws/portalfiles/portal/375600405/aacus210096.pdf): separación de base modal, contacto/cuerdas, dinámica de tabla y radiación.
- [Ege y Boutillon](https://arxiv.org/abs/1305.3057): representación reducida de la tabla, con comportamiento de placa y localización entre costillas.
- [Bank, Zambon y Fontana (2010)](https://home.mit.bme.hu/~bank/publist/taslp-piano/index.html): síntesis modal en tiempo real y transferencia de tabla.

Estas publicaciones justifican distinguir movimiento y observación. No
demuestran que esta parametrización reducida identifique un Steinway real.
