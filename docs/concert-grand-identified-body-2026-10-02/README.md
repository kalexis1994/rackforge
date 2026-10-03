# Identificación acústica desde la excitación del puente

## Qué se hizo

Se congelaron cuerdas, contacto, afinación, pérdidas, carga de puente y
programa Concert Grand. La nueva captura registra los 16 puntos de excitación
justo antes de proyectarlos sobre la tabla, después de la alimentación del
bed. Sus unidades son las calibradas del motor; no son una medición de fuerza
en newtons. Capturar no cambia el audio: se comprueba igualdad exacta, además
de las huellas sostenida y con pedales.

La salida experimental reemplaza la cadena acústica posterior por convolución
de esas señales con filtros causales. No agrega de nuevo tabla, rim o sala
del modelo anterior. El input conserva sus pérdidas y feedback anteriores:
esto identifica una observación acústica empírica, no una nueva admisión
mecánica ni un sistema con cierre energético recalculado.

Se ajustan magnitudes a partir de tres ventanas: 0–120, 120–500 y 500–1500 ms.
El espectro se representa con 64 nodos logarítmicos de 35 Hz a 12 kHz. Se
enmascaran datos débiles de fuente y referencia para evitar invertir nulos o
ruido; no se inventan parciales donde no hay evidencia. Cada registro tiene
el mismo peso total y no hay ganancias por nota.

Se prueban una respuesta global y tres regiones triangulares del puente,
con tres intensidades de suavizado. La regularización suaviza frecuencia y
espacio, y conserva una referencia de magnitud del cuerpo actual en lugares
poco observados. Las ganancias se limitan a ±24 dB alrededor de esa referencia.
La interpolación del ajuste se hace en dB; la convolución efectiva mezcla
presiones con pesos lineales. La evaluación usa esa convolución completa.

La fase se reconstruye como **fase mínima**, no se mide ni se identifica.
No se ajustan explícitamente polos, pérdidas modales o dinámica del martillo.
Este ensayo es una primera identificación de magnitud; no agota los posibles
modelos de cuerpo ni prueba que una respuesta acústica más completa no sirva.

## Separación de datos

- Ajuste: 11 raíces C2–C7 cada seis semitonos, velocidades 60/110: 22 pares.
- Selección: cuatro folds por altura; ambas intensidades de una nota quedan
  juntas, fuera del ajuste de ese fold. Se decide usando audio convolucionado,
  no sólo la curva de ganancia ajustada.
- Alturas reservadas: diez raíces intermedias, velocidades 60/110: 20 pares.
- Intensidades reservadas: 28/92/124 sobre 21 raíces: 63 pares.

El seleccionado es `regions-3-smooth-3`. La selección y el hash del filtro se
registran antes de evaluar las reservas. Son divisiones internas de este
experimento: el corpus ya se utilizó en trabajos anteriores. No se presentan
como una prueba independiente o ciega.

Referencia: bitKlavier Steinway D Stage/Lid ORTF, sampler histórico con sus
ganancias y release. Todas las alturas son raíces grabadas, sin transposición.
La capa `velocity//8+1` no es una medición de velocidad física del martillo.
Se guardan hashes de WAVs originales, mapeo, SFZ, DLL y capturas. El caché
de referencia verifica esos datos antes de reutilizar renders.

## Resultado

| Score v2, menor es mejor | Actual | Cuerpo identificado |
| --- | ---: | ---: |
| Ajuste, media de 22 pares | 5,589 | 5,868 |
| Alturas reservadas, media de 20 pares | 5,437 | 5,686 |
| Alturas reservadas, peor par | 7,024 | 11,270 |
| Intensidades reservadas, media de 63 pares | 5,410 | 5,773 |
| Intensidades reservadas, peor par | 9,066 | 12,063 |

No pasa las guardas para reemplazar el modelo actual. En intensidades
reservadas, el medio mejora (5,852 → 4,909), mientras graves y agudos empeoran.
La regresión máxima por par es 5,627 puntos. La métrica es una combinación
de errores en dB, no un porcentaje de calidad perceptiva.

**Mejora de balance armónico en C6/124**, una intensidad reservada; no equivale
a una mejora del sonido ni del ataque:

| Ataque, 20–100 ms | Actual | Identificado | Steinway |
| --- | ---: | ---: | ---: |
| H1, dB | −20,430 | −19,197 | −17,066 |
| H2, dB | −48,389 | −37,727 | −32,902 |
| H2/H1, dB | −27,960 | −18,530 | −15,837 |
| Error del cuerpo del impacto | 9,933 | 2,553 | — |

A diferencia del ensayo anterior de radiación, el acercamiento de H2 no se
obtiene debilitando H1: ésta también se acerca en esa ventana. Pero la escucha
rechazó el candidato: tiene menos ataque y se percibe más lejos del Steinway.
El ensayo queda **descartado**, incluso para C6. No se instaló.

El [control de reconstrucción posterior](../concert-grand-reconstruction-2026-10-02/README.md)
conserva una respuesta causal conocida y captura además la excitación auxiliar
que faltaba en este ensayo. Con las compuertas MIDI nativas reproduce el
ataque actual en todas las divisiones internas. El primer refit relativo
conserva el ataque, pero la selección por validación cruzada mantiene corrección
cero por regresiones en agudos. Este candidato original continúa descartado.

## Corrección tras la escucha: el crecimiento del golpe

La medida denominada «cuerpo del impacto» promediaba armónicos entre 20 y
100 ms. El ajuste usaba espectros entre 0 y 120 ms y escogía fase mínima.
Ninguno de esos pasos preserva por sí solo la evolución de energía del inicio.
La medida de bandas de los primeros 20 ms tampoco distingue cómo se reparte
la energía dentro de esa ventana. Era incorrecto presentar el acercamiento
armónico como una mejora del ataque.

Se auditaron las 24 notas de escucha y los 105 pares congelados del ensayo,
sin ajustar coeficientes ni modificar los audios. En C6/124:

| Energía de ambas cápsulas, alineada al inicio | Actual | Identificado | Steinway |
| --- | ---: | ---: | ---: |
| 0–5 ms, dB | −21,594 | −17,653 | −19,544 |
| 5–20 ms, dB | −18,036 | −16,735 | −15,513 |
| Crecimiento entre esas ventanas, dB | **3,558** | **0,918** | **4,031** |
| Tiempo del máximo RMS móvil de 5 ms, primeros 200 ms | 8,87 ms | 4,63 ms | 12,61 ms |

El candidato no pierde simplemente volumen: empieza con más energía que el
actual, pero su crecimiento inicial se aplana. El error del crecimiento
respecto al Steinway aumenta de 0,473 a 3,114 dB en C6/124. En F#6/124 el
crecimiento pasa de 2,318 a 0,668 dB, frente a 9,522 dB de la referencia.
Esto ofrece un correlato temporal de lo que se escuchó; no identifica por sí
solo toda la causa perceptiva ni prueba que la fase mínima sea el único origen.

| Error medio del crecimiento 0–5 → 5–20 ms, alturas resolubles | Actual | Identificado |
| --- | ---: | ---: |
| Ajuste | 4,032 dB | 6,506 dB |
| Alturas reservadas | 3,398 dB | 5,135 dB |
| Intensidades reservadas | 4,091 dB | 6,193 dB |
| Secuencia de escucha | 3,914 dB | 5,901 dB |

El promedio de forma de envolvente a 200 ms sí se acerca ligeramente; tampoco
sirve por sí solo para aprobar este ataque. Ahora se mantienen comprobaciones
separadas de forma temporal y crecimiento inicial. La energía se suma entre
cápsulas sin colapsar a mono. El suavizado de la envolvente usa al menos dos
períodos del fundamental; la medida fija de 5 ms sólo participa en la guarda
cuando resuelve dos ciclos. En graves se informa como diagnóstico, sin fingir
que esa ventana separa su envolvente de su fase.

Las guardas exigen no empeorar el error medio de forma ni de crecimiento;
permiten 0,5 dB en el percentil 95 de forma. Son criterios de ingeniería,
**no umbrales perceptivos validados**. Se conserva además la evaluación de
niveles y armónicos. El rechazo por escucha prevalece aunque pasen las guardas.
No se añade ruido de martillo ni un refuerzo por nota para cumplirlas.

El script de identificación, algoritmo 3, selecciona entre configuraciones
que pasan las guardas temporales en validación cruzada; si no pasa ninguna,
exporta sólo un diagnóstico. Las reservas deben volver a pasarlas. Los WAVs,
coeficientes y selección del ensayo aquí descrito siguen siendo **algoritmo 2**,
sin recalcular ni reinterpretar su selección histórica.

La comprobación nueva distingue dos señales invertidas en el tiempo, con
magnitudes FFT idénticas pero ataques distintos. Otra prueba comprueba que
las medidas relativas son invariantes a ganancia y que el error absoluto
sigue detectando una diferencia de 20 dB. Ambas pasan; también sigue pasando
la prueba del highpass analítico. Se verificó la ruta de validación sobre el
C6 congelado: conserva su score armónico anterior y expone el fallo temporal.

## Diagnóstico temporal

Se comparó la excitación capturada con la presión grabada, con alineación
de inicio independiente. Son señales diferentes: una resonancia acústica
puede cambiar la envolvente. Estos números no miden directamente las pérdidas
de las cuerdas reales ni identifican por sí solos el componente incorrecto.

| Cambio de H1 entre 50–200 y 500–1500 ms | Excitación del motor | Steinway |
| --- | ---: | ---: |
| C4/60 | −22,01 dB | −6,10 dB |
| C4/110 | −22,01 dB | −6,99 dB |
| C6/124 | −23,94 dB | −23,02 dB |

En C4/110 la ganancia requerida fuente→micrófono cambia unos 15 dB entre
esas ventanas. En C6/124 cambia menos de 1 dB. Un ajuste de color suave y fijo
no corrige ambos problemas temporales. La siguiente investigación debe separar
pérdidas intrínsecas, carga colectiva del unísono y memoria modal del cuerpo,
usando C4 como diagnóstico y el resto del teclado para verificar generalización.
La hipótesis de pérdidas/acoplamiento excesivos es una inferencia que requiere
ensayos controlados; la fuerza real del Steinway no fue medida.

## Verificación y límites numéricos

- 132 pruebas nativas aprobadas; 76 ensayos ignorados en esa ejecución.
- Captura exacta, huella sostenida y huella con pedales: tres ensayos aislados
  aprobados. Huellas `232b085ed4248398` y `4371ab4adc1542ab`.
- Test Python de respuesta impulsiva contra un highpass analítico conocido:
  error relativo menor que 10⁻¹⁰ y DC menor que 10⁻¹⁰.
- FIRs finitos de 32768 muestras, FFT 65536, acumulación offline en f64.
  No hay polos ajustados inestables. No es una implementación para tiempo real.
- Se corrigió una cola circular artificial de la primera reconstrucción:
  el cero DC se implementa mediante un highpass estable conocido de 20 Hz,
  compensado en la magnitud de ajuste, antes de truncar el FIR.
- Energía descartada: como máximo −106,56 dB. DC residual máximo 5,95×10⁻⁶.
- Pico máximo en las reservas: 0,512. Exportación a ganancia unidad, sin
  normalización por nota, saturador añadido ni cambio de pitch.

Los scripts conservan muestras FLOAT si alguna exportación excediera unidad;
este ensayo no requiere esa excepción en los conjuntos reservados. Fase
mínima y suavizado no sustituyen una identificación completa de fase y polos.

## Archivos

Carpeta `C:/SHARED/Cuerpo-identificado-2026-10-02`:

- `Cuerpo-identificado-intensidades.wav`: sólo el nuevo cuerpo empírico.
- `Actual-intensidades.wav`, `Steinway-intensidades.wav`: comparaciones separadas.
- `C6-fuerte-*.wav`: notas individuales del caso principal.
- `identified-body.npz`: coeficientes, posiciones de excitación y frecuencia.
- `report.json`, `selection.json`, `source-diagnosis.json`, `audition.json`:
  resultados, selección, procedencia y tiempos.
- `attack-diagnosis.json` y `attack-diagnosis.csv`: auditoría temporal posterior
  al rechazo por escucha, con hashes de los 72 WAVs individuales.

Secuencias de 72 s: F#2/C3/C4/C5/C6/F#6; cada una a 28/60/92/124. Cada evento
ocupa 3 s: 2 s tecla, 0,5 s release y 0,5 s silencio. C6/124 empieza en 57 s.
El release del candidato usa la excitación física amortiguada, convolucionada;
no se reutiliza como cola la muestra original.

Renderer SHA256:
`f1ee7a153219a9ae3b0e35863f75957234f1111363b1a51c91512dd73d37beff`.
Filtro SHA256:
`215cfaa633c722bc300bed874f9688a6e4e489668bf33cd5136901f1b7d7a12d`.
Datos en `local/concert-grand-identified-body`. No se instaló, empaquetó ni
hizo commit de este experimento. El `.rfplugin` anterior permanece idéntico.

Auditoría del experimento congelado (no vuelve a ajustar filtros):

```powershell
python tools/audit-concert-grand-identified-attack.py --work local/concert-grand-identified-body --output C:/SHARED/Cuerpo-identificado-2026-10-02
python tools/audit-concert-grand-bridge-envelope.py --input local/concert-grand-identified-body --output C:/SHARED/Cuerpo-identificado-2026-10-02
python -m unittest discover -s tools/tests -p test_concert_body_identification.py
python -m unittest discover -s tools/tests -p test_concert_attack_metrics.py
```

## Fundamento

[Bank y Sujbert, DAFx 2002](https://home.mit.bme.hu/~bank/publist/dafx02.pdf)
describen síntesis conmutada y separación estimada de contacto y cuerpo mediante
filtrado inverso de tonos grabados. Este prototipo adopta una identificación
de magnitud regularizada; no implementa ni valida todos los detalles de su método.
