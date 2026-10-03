# Cuerda, carga del puente y transición al sonido sostenido

## Resultado

No se aprueba una nueva calibración. Se completó el diagnóstico con el motor
nativo completo y las referencias Steinway: la duración de C4 no se corrige
alargando globalmente la pérdida interna de las cuerdas. Su fundamental pierde
demasiado nivel durante la transición al sonido sostenido. Al final, su pendiente
es muy próxima a la referencia, pero ya llega con unos 22 dB de déficit.

Las pruebas que eliminan o reducen el drenaje del puente son controles de
diagnóstico: desacoplan la carga mecánica de la tabla audible y **no son un piano
para instalar**. Mejorar C4 con ese desacoplamiento empeora el conjunto.

Los cuatro cambios consistentes de pérdida/movilidad de la tabla tampoco pasan
las guardas. Su masa afecta tanto a la admisión de energía como a la presión
radiada: al reducir la carga también cae el cuerpo audible.

## Separación de pérdidas

Se amplió únicamente la instrumentación `cfg(test)` de `BRIDGE_LOSS_TRACE`.
Se registran la tasa de pérdida intrínseca y la pérdida adicional del segundo
miembro del unísono, conservando las seis columnas previas.

`intended_rate_s` es una tasa **por cuerda**. La tasa colectiva coherente es
`mechanical_norm * intended_rate_s`; la columna histórica `effective_rate_s`
también divide por la norma y no debe interpretarse como tasa colectiva.
Para la carga compleja, esa columna histórica no es una medición independiente
del radio complejo: usa sólo el coeficiente real. Este informe usa la tasa
de conductancia prevista para separar los canales.

C4, fundamental 261,606 Hz, fábrica Concert Grand, 44,1 kHz:

| Canal | Tasa de amplitud, 1/s |
| --- | ---: |
| Intrínseca | 0,1436 |
| Puente por cuerda | 1,3799 |
| Puente colectivo, norma 3,0144 | 4,1596 |
| Pérdida extra del segundo miembro | 2,0000 |

La pérdida colectiva del puente es unas 29 veces la intrínseca. Estas tasas no
se suman todas como una única exponencial: la pérdida adicional no afecta por
igual a todos los miembros y la proyección del puente mezcla configuraciones.

En C3, C4 y C5, el puente colectivo del fundamental da respectivamente 0,814,
4,160 y 0,850 por segundo. La diferencia no demuestra un error aritmético: una
admitancia modal puede tener picos. Sí muestra por qué un cambio uniforme no
puede resolver el problema observado en C4 sin afectar otros registros.

## Ablaciones con el motor completo

Se usaron 22 pares de ajuste (C2..C7 cada seis semitonos, velocidades 60 y 110)
y 20 pares de alturas intermedias reservadas. Son reservas internas del corpus
conocido, no mediciones nuevas ni una evaluación ciega. Todos los renders usan
el mismo receptor, rutas auxiliares, sala, ganancia de fábrica y exportación a
0 dB. No hay normalización por nota.

| C4/60: caída de H1, 50–200 a 500–1500 ms | dB |
| --- | ---: |
| Actual | −23,602 |
| Pérdida interna menor, `SLOW_STAGE_RATIO=6` | −22,817 |
| Mitad del drenaje del puente | −16,868 |
| Sin drenaje del puente, diagnóstico | −7,012 |
| Sin pérdida extra del unísono | −29,528 |
| Steinway | −6,102 |

Quitar la pérdida extra del unísono no alarga necesariamente la **presión
proyectada**: puede quitar la mezcla entre configuraciones que mantenía el
aftersound. Una menor disipación total no garantiza más nivel en el micrófono.

Las cuatro alternativas físicas comparten las masas/coeficientes entre carga y
radiación: pérdida de tabla 7%, con/sin movilidad media 0,00035 y con/sin pérdida
baja 0,08. Ninguna mejora a la vez el error de todo el teclado y el ataque.
Son comprobaciones dirigidas de la hipótesis de ensanchar los picos del puente,
no una nueva búsqueda global ni prueba de que toda calibración física sea inútil.

## Curvas locales: dónde se pierde C4

Se añadió `measure-concert-grand-partial-decays.py`. Se midieron H1..H3 en
**105 pares**, en presión nativa, presión de referencia y fuerza capturada.
La energía incluye ambas cápsulas. La demodulación Hann usa como mínimo ocho
periodos fundamentales y unos 40 ms de soporte; no es un filtro del plugin.
Cada pendiente conserva su residuo de ajuste y una bandera de soporte basada
sólo en la referencia. No se fuerza una única exponencial a toda la nota.

| C4/60, H1 | Actual, dB/s | Steinway, dB/s | Déficit de nivel actual, dB |
| --- | ---: | ---: | ---: |
| 150–400 ms | −43,226 | −9,621 | 9,042 |
| 450–800 ms | −19,816 | −6,590 | 18,525 |
| 900–1400 ms | −7,296 | −7,207 | 21,666 |

C4/110 repite el patrón: pendientes nativas −43,215 / −19,823 / −7,295 dB/s;
referencia −12,445 / −7,020 / −7,502. El déficit tardío es 20,993 dB.
Esto orienta el siguiente ajuste hacia el balance energético y la transición
entre configuraciones rápida/lenta, conservando el ataque. Un multiplicador
global de T60 conserva gran parte del déficit y altera los registros que ya
tienen otro comportamiento.

Las pendientes son de envolventes acústicas, no constantes materiales de la
cuerda identificadas de forma única. En C6/124 la referencia tiene residuos
de 3,697 dB en la primera ventana: sus modulaciones impiden tratar esa pendiente
como una sola pérdida exponencial precisa. Las fases y condiciones de grabación
reales no se recuperan con este análisis.

## Control de memoria del puente

También se exportaron las masas positivas, pérdidas y proyecciones de la tabla
para un control de envolvente pasivo. Un parcial colectivo intercambia energía
con el banco modal. La eliminación estacionaria del banco recupera exactamente
la conductancia y reactancia nativas en la frecuencia portadora:

`a' = -r*a - i*sum(g*b)`; `b' = -(sigma + i*delta)*b - i*g*a`.

La derivada de `|a|² + sum(|b|²)` es `-2*r*|a|² - 2*sum(sigma*|b|²)`.
El acoplamiento almacena y devuelve energía; no puede crearla. Se comprobaron
esta identidad, todos los polos estables, energía no creciente, residuo del
eigensolve y equivalencia estacionaria con la carga exportada.

Es una aproximación de envolvente rotante, **no un solver completo de cuerdas
y tabla**. Excluye martillo, pérdidas diferentes entre cuerdas, configuraciones
horizontales, micrófonos y rutas auxiliares. Tampoco convierte las frecuencias
propias de una tabla real en datos medidos. Se analiza la envolvente de cuerda,
no se proclama una mejora de presión ni se exporta como nuevo piano.

Sobre los primeros tres parciales de 21 alturas, la mayor variación absoluta de
caída es 1,612 dB. En H1 de C4 cambia −0,873 dB, y en C6 +0,084 dB. Con esta
tabla y este control, añadir memoria no recupera los aproximadamente 17,5 dB
de diferencia de caída acústica de C4. Por tanto no se justifica implementar
una devolución de energía completa suponiendo que resolverá por sí sola el
déficit. No se excluyen otras geometrías o un modelo completo distinto.

La relevancia del acoplamiento y las polarizaciones para el aftersound está
descrita por [Weinreich](https://www.speech.kth.se/music/5_lectures/weinreic/weinreic.html).
La separación entre admitancia mecánica pasiva y observación acústica puede
verse en el trabajo de [Bank](https://home.mit.bme.hu/~bank/publist/dafx10adm/index.html).
Los controles presentes son nuestros ensayos reducidos, no reproducciones
completas de esos trabajos.

## Audios en C:\SHARED

Carpeta: `C:\SHARED\Decaimiento-puente-2026-10-02`.

- `C4-Actual-intensidades.wav`, `C4-Prueba-sin-carga-intensidades.wav`,
  `C4-Steinway-intensidades.wav`.
- Los mismos tres archivos para C6, para escuchar el coste de la ablación
  fuera de C4.
- Velocidades 28, 60, 92, 124; comienzan a 0, 3, 6, 9 s. Cada nota se mantiene
  2 s, sigue 0,5 s de release y 0,5 s de silencio.
- Ganancia común, sin EQ, raíces exactas sin transposición. El control actual
  de cuatro segundos coincide bit a bit con las capturas nativas anteriores.
- `report.json`, `tabla-consistente/report.json`, `memoria/memory-report.json`,
  `curvas/partial-decays.json` y `audition.json` contienen los detalles.

No hay commit ni paquete nuevo. Las huellas de audio de fábrica y con pedal
se conservan: `232b085ed4248398` y `4371ab4adc1542ab`.
El paquete 0.172.0 en SHARED conserva SHA256
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

## Reproducción

Renderer de ablaciones e instrumentación:
`a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831`.
Renderer que exporta el banco modal (mismo sonido):
`38b2299c41d53051d7c75f52d15b6ea650ac8c114ca352777f216ba6aeb06f0a`.

```powershell
$env:RUST_MIN_STACK='33554432'
$env:CG_GRID='36:60,39:60,42:60,45:60,48:60,51:60,54:60,57:60,60:60,63:60,66:60,69:60,72:60,75:60,78:60,81:60,84:60,87:60,90:60,93:60,96:60'
$env:CG_RATE='44100'
$env:CG_PRESET='concert'
$env:CG_WARMUP_MS='200'
$env:CG_STAGE_DIR="$PWD/local/concert-grand-decay/loading"
$env:CG_RENDER_DIR=$env:CG_STAGE_DIR
& local/concert-grand-decay/build/renderer.exe tests::render_reference_wavs --exact --ignored --nocapture --test-threads=1
$env:CG_LOADING_DIR="$PWD/local/concert-grand-decay/modes"
& local/concert-grand-decay/build/memory-renderer.exe bench::export_bridge_modal_loading --exact --ignored --nocapture --test-threads=1
python tools/audit-concert-grand-decay-loading.py --binary local/concert-grand-decay/build/renderer.exe --expected-sha256 a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831 --reference local/concert-grand-identified-body --work local/concert-grand-decay/diagnostic --output C:/SHARED/Decaimiento-puente-2026-10-02 --loading local/concert-grand-decay/loading --audition
python tools/audit-concert-grand-decay-loading.py --binary local/concert-grand-decay/build/renderer.exe --expected-sha256 a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831 --reference local/concert-grand-identified-body --work local/concert-grand-decay/physical --output C:/SHARED/Decaimiento-puente-2026-10-02/tabla-consistente --profiles docs/concert-grand-decay-loading-2026-10-02/physical-profiles.json
python tools/audit-concert-grand-bridge-memory.py --modes local/concert-grand-decay/modes --loading local/concert-grand-decay/loading --output C:/SHARED/Decaimiento-puente-2026-10-02/memoria --modes-renderer local/concert-grand-decay/build/memory-renderer.exe --expected-sha256 38b2299c41d53051d7c75f52d15b6ea650ac8c114ca352777f216ba6aeb06f0a
python tools/measure-concert-grand-partial-decays.py --input local/concert-grand-identified-body --output C:/SHARED/Decaimiento-puente-2026-10-02/curvas
python -m unittest discover -s tools/tests -p 'test_concert_*.py' -v
```

Las pruebas nuevas comparan el intercambio con una exponencial matricial
independiente, verifican admitancia estacionaria, recuperan pendientes conocidas
y comprueban que un parcial vecino o una cancelación mono no impongan la caída.
11 pruebas Python aprobadas. Los cambios Rust de esta etapa sólo pertenecen
a diagnósticos nativos `cfg(test)`; no se modificó el DSP de producción.
