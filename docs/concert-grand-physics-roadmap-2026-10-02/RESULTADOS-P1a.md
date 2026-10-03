# P1a — piloto de puente direccional

Fecha: 2026-10-02. Estado: medido, sin mejora consistente; escucha pendiente.
No hay cambio de calibración ni paquete instalado.

## Qué se probó

Dos coordenadas: vertical colectiva y horizontal agregada heredada. La fuerza
horizontal recibe una contribución de momento por altura del puente. Cada modo
contribuye una matriz simétrica mediante producto exterior de sus proyecciones.
La conductancia es semidefinida positiva; carga y excitación de tabla usan la
misma proyección de momento. No es un solver dinámico completo de cuerda/tabla.

Alturas globales supuestas: 10, 25 y 40 mm. Longitud de línea de puente heredada:
1,4 m. Orientación y geometría no medidas. Se mantienen tabla sintética,
afinación, martillo, pérdidas intrínsecas, mezcla y micrófonos de fábrica.
Sin normalización individual, EQ ni detune adicional. Ganancia de exportación 0 dB.

Programa Concert Grand, 44,1 kHz, bloque 256, calentamiento 200 ms. C4/C5/C6/F#6,
velocidades 28/60/92/124, tecla sostenida 2 s y liberación 2 s. Cuatro controles
adicionales por perfil con velocidad 124 y tecla sostenida 12 s, salida 14 s.
Referencias bitKlavier Stage Lid ORTF con nota grabada exacta y procedencia
verificada; ninguna transposición en estos 16 casos. Corpus ya usado antes,
no validación ciega ni identificación única de mecanismos.

## Medidas antes/después

Score v2 de la auditoría existente: error compuesto espectral/temporal;
**menor es mejor**, no es una escala de percepción ni fidelidad universal.

| Perfil | Error medio, 16 casos | Peor caso |
| --- | ---: | ---: |
| Base física | 6,83418 | 9,06559 |
| Palanca 10 mm | 6,83258 | 9,13152 |
| Palanca 25 mm | 6,88337 | 9,25034 |
| Palanca 40 mm | 6,99100 | 9,39098 |

La diferencia media de 10 mm es mínima y el peor caso empeora. No se selecciona
una variante con esa medida. Con 25 mm, a velocidad 124, el error absoluto del
fundamental tardío aumenta 0,733 dB en C4, 0,182 en C5 y 0,270 en C6; disminuye
0,764 en F#6. No hay recuperación consistente del cuerpo. El error medio de
forma del ataque pasa de 1,86208 a 1,85993 dB y el de nivel de ataque de 2,69646
a 2,69935 dB: cambios pequeños, no evidencia de resolver el ataque.

Movimiento relativo del balance entre parciales: intervalo 0,25–1,8 s a velocidad
124, frecuencias nativas fijadas por la base, soporte armónico definido por la
referencia. Se resta el cambio simultáneo de nivel y el desplazamiento fijo de
cada parcial; se muestra la mediana del rango P95–P5 de ese cambio, en dB.

| Nota | 10 mm | 25 mm | 40 mm |
| --- | ---: | ---: | ---: |
| C4 | 0,066 | 0,181 | 0,307 |
| C5 | 0,245 | 0,472 | 1,299 |
| C6 | 0,334 | 0,692 | 0,988 |
| F#6 | 0,833 | 1,726 | 2,633 |

Es una descripción respecto de la base, no un detector de flanger audible ni
prueba de acercamiento al Steinway. Puede incluir curvas débiles. El conteo de
mínimos tardíos de más de 3 dB es cero en estos perfiles después de 1,5 s;
eso tampoco certifica ausencia de modulación perceptible. A mayor palanca se
introduce más movimiento de color, particularmente en los agudos. Evitar volver
a aceptar un sostenido sólo por nivel o por un conteo de mínimos favorable.

## Verificación

- Seis pruebas del módulo matricial pasan: tres previas y tres nuevas. Las
  nuevas comprueban palanca nula contra carga escalar, pasividad/conservación
  de diferencias verticales oscuras y proyección común del momento.
- Nueve matrices contrastadas con SciPy `expm` en float64 a 16/44,1/96 kHz:
  error máximo complejo `6,943817362976192e-8`, norma singular no superior a
  `1 + 1e-6`. Oráculo de la composición Strang, no de un solver físico completo.
- Interruptor apagado: 16 audios crudos idénticos al renderer congelado anterior.
- Huellas nativas de fábrica: `232b085ed4248398` y, con pedales,
  `4371ab4adc1542ab`; ambas pruebas pasan.
- Audios finitos y sin clipping en todos los casos renderizados. No se amplió
  todavía a piezas, reiteraciones, rendimiento ni WASM con el prototipo activo.
- Paquete 0.172.0 en SHARED conserva SHA256
  `ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

## Archivos y reproducción

Renderer nativo congelado: `local/concert-grand-directional/build/renderer.exe`,
SHA256 `18b16e5f8f5739eec4420302be206c3a55cc1e6c4191456ae0a9b49736505d33`.
Código diagnóstico detrás de `cfg(test)`, parámetro
`DIRECTIONAL_BRIDGE_HEIGHT_M`; valor cero conserva la ruta anterior.

Herramienta: `tools/audit-concert-grand-directional.py`. Informe íntegro con
huellas, eventos, métricas, soporte, curvas y tiempos de comparadores:
`C:\SHARED\Fisica-pendiente-2026-10-02\P1a-puente-direccional\report.json`.
Resumen persistente en [resumen-P1a.json](resumen-P1a.json); matrices de contraste
en [matrix-probes.csv](matrix-probes.csv).

```powershell
$env:PYTHONPATH='C:/Users/kalex/.codex/worktrees/concert-grand-refit/rackforge/target/refit-venv/Lib/site-packages'
$env:PYTHONDONTWRITEBYTECODE='1'
$env:OPENBLAS_NUM_THREADS='1'
$env:OMP_NUM_THREADS='1'
& 'C:/Users/kalex/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe' tools/audit-concert-grand-directional.py --binary local/concert-grand-directional/build/renderer.exe --expected-sha256 18b16e5f8f5739eec4420302be206c3a55cc1e6c4191456ae0a9b49736505d33 --matrix-probes local/concert-grand-directional/matrix-probes.csv --work local/concert-grand-directional/renders --output C:/SHARED/Fisica-pendiente-2026-10-02/P1a-puente-direccional
```

Cada comparador C4/C5/C6/Fsharp6 dura 36 s: velocidades 28, 60, 92, 124; en
cada una, base física / palanca 25 mm / Steinway, tres segundos por entrada.
La variante intermedia se fijó antes de mirar las medidas. No es la B anterior
rechazada por flanger. No se presenta como candidato aprobado.

## Decisión y siguiente paso

Sin selección automática ni mejora demostrada. Preservar audios para escucha.
Este resultado limita la hipótesis de palanca sobre la tabla heredada; no
descarta toda admitancia direccional real. P1b sigue pendiente: separar los
estados horizontales por cuerda con un control de equivalencia energética antes
de ensayar excitación diferencial. P2 sigue pendiente para sustituir la base
de tabla sintética por una representación con rigidez direccional y costillas.
