# P6 — Primera frase musical con las notas físicas

2026-10-05. El usuario escuchó el C4 de un segundo y pidió contexto musical
para juzgar el sonido. Se prepara un arpegio monofónico de audición:
**C4–E4–G4–C5–G4–E4–C4**, a velocidad MIDI 124, con un segundo de fuente
por nota y ataques separados 1,2 s. Se preservan ganancia y micrófonos.

## Qué representa esta escucha

Cada altura procede de un contacto no lineal de su propia cuerda, con su
longitud, masa, martillo, fieltro y punto de proyección sobre la tabla.
No se cambia el pitch del C4 para fabricar E4/G4/C5. Se mantienen 72 modos
por dirección transversal, 144 axiales, 59 de tabla, carga refinada y
38232 estados acústicos, igual que en la primera nota física.

La frase ensambla notas frías independientes, **sin superponer sus cuerpos**.
Sirve para escuchar el cambio de timbre entre alturas en un motivo musical.
No representa polifonía sobre una tabla compartida, resonancia simpática
entre las notas, reiteraciones sobre una cuerda viva, pedal ni acción.
El final de cada nota conserva el fade de edición de 8 ms del banco; no se
presenta como apagador físico. C4 reutiliza exactamente su WAV aprobado.

Esta elección permite una primera escucha musical con el piloto existente;
no selecciona programas ni instala otro paquete. Las piezas MIDI completas
requieren ampliar el sistema de estados y acción, no sólo colocar WAVs.

## Obtención de parámetros

El inspector congelado P3 `b1df5706…` sólo recorría 42/60/72/84/90. Pedir
64 o 67 devolvía CSVs con cabecera y sin datos. Se retiraron esas cabeceras
vacías y se amplió **únicamente el test ignorado** de exportación para que
`CG_P3_PROBE_NOTE` admita cualquier tecla 21–108. El recorrido por defecto
se conserva. No se cambia el código del motor que compila sin tests.

Nuevo inspector de parámetros en
`local/concert-grand-melody/build/inspector.exe`, SHA256
`80aa62910e4c1347ae07a1fe7e42024ce25bcdb30ab0766ac1b3152299737577`.
Compilado directamente con Rust 1.98.0 MSVC y el SDK como rlib, sin crear
otro árbol de dependencias Cargo ni copiar fuentes a un ZIP.

E4/G4 se exportan con el inspector ampliado; C5 usa el exportador congelado.
Los nueve parámetros primitivos de C4 coinciden **exactamente** con los
inputs del piloto anterior. Se comprobaron realmente, en invocaciones
separadas y con `--ignored`, los tests `bench::render_fingerprint` y
`bench::render_fingerprint_pedals`: pasan, con huellas `232b085ed4248398`
y `4371ab4adc1542ab`. La exportación temporal del C4 se retira después de
registrar la comprobación para evitar otra copia de los mismos inputs.
Procedencia canónica: `medidas-P6-frase/export.json`.

Se importan los parámetros físicos anteriores a la normalización audible.
Las amplitudes y fases de las voces de fábrica **no** inicializan la cuerda:
cada contacto SI empieza desde reposo con la velocidad incidente de martillo.

## Controles por nota nueva

E4/G4/C5 repiten contactos fríos a 384/768 kHz. Se evalúan almacenamiento
y radiación angular independiente para cerrar la energía del ataque a
1e−9 relativo. Se comparan duración del contacto (0,1%), fuerza/impulso
(0,2%) y B/V de tabla (1%). La herramienta admite refinamientos adicionales
si ese par falla; no relaja los criterios.

Desde el contacto fino se conserva el estado completo y se continúa hasta
un segundo a 64/128 kHz con el integrador polinómico/exponencial. Se comparan
presión compleja completa en 60–6000 Hz y B/V (1%), captura 128→64 kHz
(0,1%) y pérdida inferida positiva. La radiación larga sigue siendo inferida,
no una segunda integral angular. La salida usa la trayectoria de 128 kHz,
estéreo 48 kHz y **0,25 full scale/Pa**, igual que el C4 anterior.

Pasan los cinco controles de las tres notas nuevas, seleccionando contacto
768 kHz y sostenido 128 kHz para la escucha:

| Nota | Presión 64 vs 128 | Posición tabla | Velocidad tabla | Presión de captura | Pico WAV |
|---|---:|---:|---:|---:|---:|
| E4 | 0,122554% | 0,026657% | 0,062412% | 0,00001781% | 0,515757 |
| G4 | 0,223585% | 0,017735% | 0,038861% | 0,00004945% | 0,250469 |
| C5 | 0,150925% | 0,018963% | 0,037974% | 0,00008088% | 0,304725 |

Máximo error del balance independiente de energía en los seis contactos:
5,973e−12, bajo 1e−9. El máximo cambio de duración de contacto es 0,000291%;
fuerza, 0,002862%; impulso, 0,0000576%; B/V de ataque, 0,003131% / 0,013215%.
Las uniones conservan posición y velocidad exactamente. Los seis residuos
de radiación larga inferida son positivos. El banco nuevo tardó hasta
1045,922 s (17,4 min) por proceso, ejecutando las tres notas a la vez;
ese tiempo incluye ambos ataques, ambas continuaciones y sus controles.

Frase final: **8,603479 s**, 48 kHz estéreo float32, pico 0,515757 y ganancia
común 0,25/Pa. El ensamblado no modifica muestras dentro de cada nota:
retira 100/300 ms de silencio de sus WAVs y las coloca separadas 1,2 s.
Preserva retardos y el fade existente. SHA256 y eventos en `frase.json`.

WAV único de la frase:
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-frase/Arpegio-C4-E4-G4-C5-Piloto-fisico.wav`.

## Archivos y reproducción

Informes y fuentes únicos en el repo:

- `medidas-P6-frase/export.json`: igualdad C4, huellas y exportador.
- `medidas-P6-frase/nota-64.json`, `nota-67.json`, `nota-72.json`: parámetros,
  controles de ataque/sostenido, hashes y WAV de cada altura nueva.
- `medidas-P6-frase/frase.json`: eventos, fuentes verificadas y WAV final.

Datos únicos en `C:/SHARED/Fisica-pendiente-2026-10-05/P6-frase`:
`input/<nota>-124` para CSVs nativos nuevos; `notas/<nota>-124` para los
contactos y coasts; `notas/<nota>-124.wav` para los tres WAVs nuevos.
El C4 queda en su carpeta P6-nota-1s y se lee desde allí, sin copiarlo.
El factor acústico y el kernel conservan su única copia anterior.

Herramientas:

1. `tools/render-concert-grand-physical-bank-note.py --note <64|67|72>`:
   inputs/asset/coefficients/kernel como en el C4 anterior, `--baseline-report`
   es `resumen-P6-contacto-1200-refinado.json`. Usar `--input` específico y
   destinos nuevos `--output`, `--audio`, `--report`. Defaults: 124, 1 s, 0,25.
2. `tools/assemble-concert-grand-physical-phrase.py --c4-report <render-C4>
   --note-reports <nota-64> <nota-67> <nota-72> --audio <WAV> --report <JSON>`.
   Verifica gates/hashes, misma ganancia/micrófonos y notas separadas. Sólo
   quita silencios de escucha y coloca cada nota en su instante. No agrega
   EQ, resampling, fades o ajuste de nivel.

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1; un hilo BLAS por proceso. Las
tres notas se calculan en procesos independientes con destinos separados.
Se conservan el paquete 0.172.0 y los cambios previos ajenos del host.

El usuario rechazó esta escucha el 2026-10-05: poca definición, predominio
grave, falta de brillo y cortes secos. No queda aceptada como candidato.
Los controles temporales pasan, pero falta convergencia de la banda alta de
la tabla. El note-off y los diagnósticos siguientes se documentan en
[release y banda](RESULTADOS-P6-release-y-banda.md).
