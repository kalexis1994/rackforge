# P1b — dos polarizaciones por cuerda

Fecha 2026-10-02. Piloto medido, sin mejora demostrada ni selección automática.
Paquete/fábrica conservados. Audios en
`C:\SHARED\Fisica-pendiente-2026-10-02\P1b-polarizaciones`.

## Construcción y límites

Sólo en pruebas nativas: siete lanes, V0/V1/V2, H0/H1/H2 y bloom heredado.
Una o dos cuerdas usan sólo sus pares correspondientes. El estado horizontal
agregado se levanta como `H_i = H / sqrt(N)`, con proyección `h / sqrt(N)`.
Así se conservan la norma de energía modal heredada y la fuerza conjugada.
Esta norma no equivale a una identificación de masa ni energía en julios.

Modo 1: mismo polo/excitación en cada horizontal. Control de equivalencia.
Modo 2: reparto/fase del martillo heredado de cada vertical, normalizado a la
misma energía horizontal inicial; la horizontal usa el ángulo de frecuencia
de su propia cuerda y pérdidas intrínsecas heredadas. Sin nuevo jitter por
parcial. `POLARISATION_CENTS=0` en este piloto. La cancelación del bloom se
reconstruye con la proyección nueva; es el mecanismo sintético de ataque ya
existente, no contacto tridimensional de fieltro identificado.

La carga escalar se levanta al subespacio común horizontal. La matriz P1a se
levanta a las dos coordenadas comunes mediante una transformación isométrica;
no absorbe por sí sola las diferencias entre horizontales. Con polos iguales,
esas diferencias permanecen oscuras. La afinación pequeña es un control
separado, no una consecuencia inevitable de agregar estados.

Amortiguadores, recuperación del pedal, culling, empujes de reiteración y
rotaciones recorren los estados nuevos. La tensión no lineal conserva la
proyección horizontal común para aislar P1b; su auditoría física corresponde
a P3. No se habilitan a la vez las separaciones de pines del ensayo anterior.

## Ensayos

C4/C5/C6/F#6, velocidades 28/60/92/124, tecla 2 s y salida 4 s. Además,
sostenidos de 12 s a velocidad 124, y controles de una cuerda, dos cuerdas y
reiteraciones con sustain/medio pedal/liberación. 44,1 kHz, bloque 256,
calentamiento 200 ms. Referencia ORTF bitKlavier original, notas exactas,
misma ganancia de exportación y corpus usado antes; no validación ciega.

| Perfil | Error medio, 16 casos | Peor caso |
| --- | ---: | ---: |
| Base | 6,83418 | 9,06559 |
| Separación equivalente | 6,83416 | 9,06558 |
| Reparto de martillo por cuerda | 6,83538 | 9,06704 |
| Base con afinación 0,04 | 6,83853 | 9,05541 |
| Reparto con afinación 0,04 | 6,83876 | 9,05676 |
| Reparto con palanca 25 mm | 6,89652 | 9,25022 |

Score v2 heredado: menor es mejor, no una medida universal de fidelidad audible.
El reparto solo cambia el error del fundamental tardío a velocidad 124 en
0,0063 / 0,0028 / 0,0187 / −0,0096 dB para C4/C5/C6/F#6. La forma del ataque
media pasa de 1,86208 a 1,86201 dB de error. Cambios mínimos, sin mejora consistente.

Mediana del movimiento de color P95–P5 respecto de la base, 0,25–1,8 s,
frecuencias fijas y soporte armónico de la referencia:

| Nota | Reparto solo | Afinación sola 0,04 | Reparto + afinación | Reparto + palanca |
| --- | ---: | ---: | ---: | ---: |
| C4 | 0,0011 | 0,1264 | 0,1257 | 0,1786 |
| C5 | 0,0037 | 0,4127 | 0,4200 | 0,4481 |
| C6 | 0,0322 | 1,1661 | 1,1189 | 0,4816 |
| F#6 | 0,0479 | 0,9369 | 0,9400 | 1,4232 |

Es una descripción relativa del timbre, no un detector perceptual de flanger.
La afinación domina ese cambio en su comparación factorial; separar estados
no lo suprime por sí solo. No seleccionar por conteo de mínimos tardíos cero.

## Verificación y procedencia

- Cinco pruebas de polarización pasan: norma/fuerza/carga compleja, diferencias
  horizontales oscuras, seguimiento temporal escalar y matricial, reparto
  inicial con afinación por cuerda, culling y grip/recuperación del amortiguador.
- Seis pruebas existentes/matriciales pasan; un exportador queda ignorado.
- Prototipo apagado: 16 audios crudos idénticos a P1a/Base y huellas
  `232b085ed4248398`, `4371ab4adc1542ab` conservadas.
- Separación equivalente: error RMS relativo peor −70,756 dB en los 16 casos,
  −72,079 dB en dos cuerdas y −74,050 dB con reiteraciones y pedal; una cuerda
  idéntica. No se afirma identidad binaria de los controles de varias cuerdas.
- Todos los casos son finitos y no clipean. Aún falta coste/validación WASM,
  piezas completas y propiedades físicas direccionales identificadas.
- Renderer congelado `local/concert-grand-polarization/build/renderer.exe`,
  SHA256 `e3d116ed629f95f4bfd942b84a3af11d3222a7ee8a88095f0a8a2fd3e6e76638`.
- Herramienta `tools/audit-concert-grand-polarization.py`; metadatos completos
  en el `report.json` de SHARED; [resumen](resumen-P1b.json) en el repositorio.

```powershell
$env:PYTHONPATH='C:/Users/kalex/.codex/worktrees/concert-grand-refit/rackforge/target/refit-venv/Lib/site-packages'
$env:PYTHONDONTWRITEBYTECODE='1'
$env:OPENBLAS_NUM_THREADS='1'
$env:OMP_NUM_THREADS='1'
& 'C:/Users/kalex/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe' tools/audit-concert-grand-polarization.py --binary local/concert-grand-polarization/build/renderer.exe --expected-sha256 e3d116ed629f95f4bfd942b84a3af11d3222a7ee8a88095f0a8a2fd3e6e76638 --work local/concert-grand-polarization/renders --output C:/SHARED/Fisica-pendiente-2026-10-02/P1b-polarizaciones
```

Archivos `C4-Reparto-martillo-intensidades.wav` y equivalentes para otras notas:
sólo el prototipo, intensidades a 0/3/6/9 s. Los comparadores alternan por
intensidad base / P1b / Steinway. La escucha permanece pendiente; ninguna variante
se anuncia como mejor ni se instala.

## Fuentes y siguiente paso

[Bank, tesis de modelado de piano](https://home.mit.bme.hu/~bank/thesis/pianomod.pdf)
describe las dos polarizaciones y advierte que su acoplamiento complica la
explicación de doble decaimiento basada sólo en pérdidas distintas. Se usa
como contexto físico; la normalización concreta anterior es un control propio.
P2 pasa a la tabla ortótropa con costillas: cambiar su representación espacial,
no ampliar otra búsqueda de detune que ya fue rechazada.
