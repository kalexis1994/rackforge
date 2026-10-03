# P3 — Intercambio no lineal, unidades y precisión

Fecha: 2026-10-02. Estado: **parcial, sin candidato seleccionado**.

## Decisión

La corrección de los cruces entre cuerdas en el motor existente no recuperó
el cuerpo ni el ataque. Se conserva la fábrica 0.172.0. El avance es una
referencia física verificable y la identificación de un límite numérico concreto;
no se presenta como mejora audible ni como mecanismo identificado del Steinway.

## Auditoría del motor actual

`Voice::tension_step` calcula el cuadrado de una suma de amplitudes verticales
y de la horizontal común, ponderadas por `h/16`. Esto introduce productos entre
cuerdas y direcciones que no son el incremento de longitud individual. Se
corrigió el comentario que afirmaba lo contrario; la aritmética de fábrica se
conservó. Los estados tienen amplitudes empíricas: la norma del fasor no es
energía mecánica en julios. El contacto empieza con unidades físicas, pero hay
normalización y modelado de amplitudes antes de esos estados. Falta explicitar
esa conversión antes de acoplar un nuevo núcleo físico.

La tensión se actualiza cada 32 muestras: a 44,1 kHz, 1378,125 actualizaciones/s
y Nyquist de 689,0625 Hz. Elevar estados al cuadrado genera componentes rápidas;
suavizar después del submuestreo no evita su alias. El piloto de promedio de
fase elimina esa oscilación rápida antes de la actualización como diagnóstico.
No equivale al intercambio físico completo a tasa de audio.

La fuente longitudinal heredada eleva al cuadrado una proyección audible de
pendientes, incluyendo la horizontal proyectada y fuentes auxiliares, y alimenta
cuatro resonadores. No devuelve su trabajo a las cuerdas transversales. Conservar
el radio del polo o limitar amplitudes no demuestra conservación energética.

## Referencia física independiente

Implementación: `tools/concert_grand_nonlinear_string.py`. Una cuerda extensible
con extremos fijos, dos desplazamientos transversales `v,w` y uno axial `u`.
Coordenadas modales normalizadas por raíz de masa, momento conjugado y energía
en julios. El potencial compartido es:

```
epsilon = u_x + (v_x² + w_x²)/2
U = T/2 integral(v_x² + w_x²) dx + EA/2 integral(epsilon²) dx
K = P·P/2
```

Las fuerzas de ambos lados del intercambio salen del mismo gradiente. Se usa
un gradiente discreto AVF con dos puntos de Gauss, exacto para este potencial
cuártico, y una resolución implícita por Newton. Las pérdidas positivas se
contabilizan como trabajo de amortiguación. El principio de balance energético
y acoplamiento recíproco está respaldado por
[Chabassier, Chaigne y Joly, 2013](https://perso.ensta.fr/~touze/PDF/Batwoman/chabassier-jasa.pdf).
Este piloto simplificado no reproduce su modelo Timoshenko completo.

Ensayo controlado: 12 modos por dirección transversal y dos axiales; T=700 N,
E=200 GPa, densidad=7850 kg/m³, L=0,56744147 m, masa lineal=0,007940275 kg/m,
masa modal=0,002252821 kg. Relación axial/transversal **17:1 deliberadamente
resonante**, no medida del Steinway. Se excitan modos transversales 8/9 y
componentes adicionales débiles. Energía inicial 0,003525664 J, duración 12 ms.
Se compara con DOP853 independiente, tolerancias 2e−11/1e−13 y paso limitado.

| Tasa de integración | Máxima deriva relativa de energía | Error final de estado ponderado por energía |
| --- | --- | --- |
| 44 100 | 2,20e−12 | 153,03% |
| 88 200 | 4,06e−15 | 124,06% |
| 176 400 | 4,92e−15 | 42,55% |
| 352 800 | 5,17e−15 | 10,96% |
| 705 600 | 8,09e−13 | 2,75% |
| 1 411 200 | 7,03e−12 | 0,6884% |

El error compara coordenadas y momentos con pesos de energía lineal, no mezcla
metros y velocidades en una norma sin unidades. El último cociente de errores
es 3,9969 al dividir el paso por dos, consistente con convergencia de segundo
orden. La energía axial máxima cambia mucho al refinar el paso: 14,66% a
44,1 kHz frente a 83,80% a 32×. **Conservar energía no garantiza fase ni
transferencia correctas.** El integrador no está listo para el callback.

Gates adicionales: trabajo exacto del gradiente discreto, error relativo
4,08e−16; gradiente/Hessiano por diferencias finitas, 1,31e−11/3,16e−11;
energía espacial calculada independientemente, 3,95e−15; balance con pérdidas,
1,60e−12. La reciprocidad del Hessiano se verifica; su error absoluto 2,61e−8
debe interpretarse frente a las rigideces SI, no como error relativo.

Una ablación que fuerza el movimiento axial sin devolver reacción transversal
crea hasta **382,71% de energía extra en este ensayo**. Ese porcentaje pertenece
al núcleo independiente y a esta excitación resonante; **no es una medición
de energía del plugin de fábrica**. La partición de energía de interacción no
es única; el balance se juzga sobre la energía total.

Faltan puente, martillo integrado, varias cuerdas, apagadores, identificación
geométrica, radiación y validación de coste. El término opcional de flexión
Euler no incorpora cizallamiento/inercia rotacional; P4 sigue pendiente.

## Controles nativos de atribución

Módulo `nonlinear_wires.rs`, sólo bajo `cfg(test)`; sin parámetro público nuevo.
Todos conservan ganancia, contactos, tabla y afinación nominal de fábrica.

| Perfil | Cambio | Score medio, diez casos |
| --- | --- | --- |
| Actual | Fábrica | 6,51305535 |
| Control-polarizaciones | Horizontal individual equivalente P1b | 6,51306569 |
| Geometria-instantanea | Estiramiento separado por cuerda | 6,51313609 |
| Geometria-promediada | Estiramiento medio de fase | 6,51313750 |
| Fuente-por-cuerda | Anterior + fuente axial sin cruces entre cuerdas/direcciones | 6,51332326 |

Menor score es mejor. No hay mejora demostrada; las diferencias medias son
ínfimas. No comparar este promedio de diez casos con el promedio P2 de 16.

Notas exactas F#2, C4, C5, C6, F#6 a velocidades 60 y 124: dos segundos
pulsadas y dos de release, 44,1 kHz/256. Referencias Steinway con procedencia
y hashes verificados. Exportación a ganancia unitaria sobre la ganancia interna
vigente, sin normalización individual. Ocho controles medios/agudos de fábrica
son idénticos por bytes a P2. También se probó una secuencia con reiteraciones,
sustain, medio pedal y retorno completo: cinco resultados finitos, pico <0,392.
Esto no verifica headroom interno, coste WASM ni cambios de programa en vivo.

Los pilotos conservan `h/16`, ganancias empíricas y banco axial compartido
sin reacción. Además excluyen fuentes auxiliares del cálculo de tensión; no
son una ablación exclusiva de un único producto cruzado. La fuente por cuerda
conserva pares de armónicos dentro de cada cuerda y separa direcciones. La
tensión individual puede introducir diferencias transitorias de frecuencia,
aunque no se añadió detune nominal.

Mayor diferencia en F#2 fuerte: RMS relativo −37,76 dB al separar la fuente.
En C4 fuerte, −59,12 dB; los otros medios/agudos quedan alrededor del error
del control equivalente, ~−72 dB. Instantáneo/promediado son idénticos por bytes
en C4/C5/C6/F#6. No se pidió otra elección al usuario sobre variantes tan cercanas.

## Inspección de precisión de polos

CSV `P3-precision/precision.csv`. Primeros ≈120 ms, velocidad 124, después
de calentamiento; cuatro modos de geometría y cinco alturas. Se comparan
coeficientes `rc/rs` cada 32 muestras y ángulo del polo frente al inicial.
La frecuencia observada aquí es la del polo almacenado, no una estimación
del audio completo ni de la carga reactiva del puente.

| Nota | Tensión relativa solicitada máx., fábrica | Cambio máx. de fundamental | Cambio máx. de cualquier parcial vertical físico |
| --- | --- | --- | --- |
| F#2 | 8,70118e−6 | 0,015267 cents | 0,015268 cents |
| C4 | 6,22296e−7 | 0 | 0,0004343 cents |
| C5 | 9,88308e−8 | 0 | 0 |
| C6 | 1,59867e−8 | 0 | 0 |
| F#6 | 7,03045e−9 | 0 | 0 |

En C5/C6/F#6 no se registra **ningún cambio** de coeficientes verticales físicos
en la ventana. En los pilotos por cuerda tampoco cambia ningún parcial de C4.
F#2 sí conserva pequeños cambios en parciales altos, hasta 0,0007774 cents
en el piloto instantáneo. Las variables de tensión aplicada avanzan aunque
el incremento redondeado no modifique el polo: se pierde esa fracción en el
esquema incremental `f32`. No extrapolar a todas las notas/programas/tiempos.

Antes de aumentar ganancias hay que arreglar la representación incremental y
su conversión física. Una suma geométrica correcta en amplitudes normalizadas
no determina por sí sola cuánto se estira una cuerda real.

## Verificación y reproducción

Cuatro pruebas nuevas pasan: independencia entre cuerdas, promedio de fase y
exclusión de auxiliares, productos de pendiente dentro de una cuerda, radios
y polos separados. Inspector offline pasa. Huellas apagadas:
`232b085ed4248398`, `4371ab4adc1542ab`; conservadas en renderer e inspector.

Artefactos en `C:\SHARED\Fisica-pendiente-2026-10-02`:

- `P3-energia/report.json`: trazas y gates del núcleo SI.
- `P3-geometria-audio/`: report.json, intensidades, notas y reiteraciones.
- `P3-precision/precision.csv`: tensión solicitada y polos medidos.
- `resumen-P3.json`: hashes y resultados compactos de los tres controles.

Binarios y fuentes: `local/concert-grand-nonlinear/build/`. `renderer.exe`
con hash `50c79da2221b625d27c7f394e9787fd5895352fed45c5c7d52aff6c992579d67`
es el que produjo los audios. `source.zip` conserva su fuente previa al inspector.
`inspector.exe` y `source-inspector.zip` conservan el inspector posterior;
hashes en el resumen. Las muestras y metadatos anteriores del refit se requieren
para reproducir las comparaciones. No sobrescribir binarios congelados al compilar.

```
cargo +1.98.0-x86_64-pc-windows-msvc test -p rackforge-concert-grand --lib --release nonlinear_wires::tests -- --test-threads=1
python tools/audit-concert-grand-nonlinear-energy.py --output <carpeta>
python tools/audit-concert-grand-wire-geometry.py --binary <renderer.exe> --expected-sha256 50c79da2221b625d27c7f394e9787fd5895352fed45c5c7d52aff6c992579d67 --work <carpeta> --output <carpeta>
```

Inspector: definir `CG_P3_PRECISION_DIR` y ejecutar
`inspector.exe nonlinear_wires::tests::export_native_tension_precision --exact --ignored --nocapture --test-threads=1`.
Usar `RUST_MIN_STACK=33554432`; dependencias Python numpy/scipy/soundfile/mido.

## Próximo paso

P3 permanece abierto: identificar conversión de contacto a estados SI y fuerza
de puente, probar una trayectoria de tensión acumulada representable y elegir
una integración con fase/energía y coste aceptables. Después conectar el
intercambio bidireccional. Mantener los pendientes P2 y la comparación audible
con ataque, sostenido y velocidad; no cubrir esta incertidumbre con otro refit
de ganancias.
