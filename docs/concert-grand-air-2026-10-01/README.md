# Concert Grand: absorción del aire por recorrido

Implementación del 1 de octubre de 2026 sobre el candidato de proximidad corregida,
aceptada por el usuario para commit. No se reemplazó
`C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin`.
El cambio previo de `apps/rackforge-desktop/src/main.rs` queda fuera de este trabajo.

## Física y referencia

La presión pierde amplitud según `exp(-alpha(f)*r)`, con `alpha` en Nepers/m.
Se implementaron los términos clásicos y de relajación de oxígeno/nitrógeno
de [ISO 9613-1](https://cdn.standards.iteh.ai/samples/17426/3a2d69b767024b74805b83b063a91445/ISO-9613-1-1993.pdf).
La propagación por aire depende de frecuencia y distancia; su tratamiento
en la respuesta de una sala se describe también en
[Hamilton, 2021](https://arxiv.org/abs/2107.11871). Este código usa una
aproximación FIR propia, no el solucionador modal completo de ese artículo.

Condiciones de referencia asumidas: **20 °C, humedad relativa 50%, presión
101.325 kPa**. No fueron identificadas de las muestras Steinway. La función
atmosférica admite temperatura, humedad y presión, pero esta audición fija
esas condiciones y no añade controles ni slots de estado público.

Valores calculados de absorción, sin pérdida geométrica ni paredes:

| Frecuencia | dB/m | Pérdida a 2.64 m |
| --- | ---: | ---: |
| 1 kHz | 0.004665 | 0.012315 dB |
| 4 kHz | 0.029666 | 0.078317 dB |
| 8 kHz | 0.105291 | 0.277968 dB |

La absorción del aire no cambia la energía mecánica de cuerdas/tabla ni sus
decaimientos. Una nota sostenida emite sonido nuevo que atraviesa siempre
el mismo camino directo; en la cola de sala se acumulan recorridos.

## Implementación y rutas

`plugins/concert-grand/src/air.rs` construye un cepstrum de fase mínima desde
la curva ISO en una rejilla de 128 puntos. Se calcula una vez por retune y se
comparte entre los caminos. Cada camino obtiene un FIR causal de 16 taps
hasta 32 m efectivos, o 32 taps para caminos mayores, con DC unitario.
El amount avanzado existente escala la absorción respecto a su valor
compilado de 0.0022; ese número deja de interpretarse como la absorción
física exacta a 4 kHz. Amounts grandes se dividen en etapas de hasta 100 m
efectivos para evitar subflujo y una aproximación corta mal condicionada.

El bucle de audio sólo hace convoluciones finitas; no tiene asignaciones,
FFT ni llamadas trascendentales. Una ruta silenciosa termina su cola FIR,
limpia su historia y omite operaciones sobre ceros. Reset/prepare limpia
todos los historiales, incluyendo las etapas auxiliares de amounts altos.
Una reconfiguración conserva la historia útil, y una etapa que se vuelve
a activar empieza con historia nula.

- **Directo:** un filtro por cápsula, usando la distancia geométrica al centro
  de la tabla. Tabla extendida, cuerdas auxiliares y mecanismos comparten esa
  aproximación. No se aplica absorción a cada punto de la integral Rayleigh.
- **Primeras reflexiones:** doce filtros, uno por cada imagen/cápsula, con
  el recorrido reflejado completo. La señal que alimenta sus retardos aún
  no pasó por el filtro de distancia directa.
- **Tapa:** ocho filtros, con el recorrido de cada imagen. La coloración de
  tapa existente sigue separada; estas reflexiones no se filtran de nuevo
  como si fueran sonido directo.
- **Sala:** seis filtros en feedback, con metros `samples_almacenados/fs*c`
  por vuelta. El RT de pared excluye el viejo término de aire, evitando
  contarlo dos veces. El gain de feedback usa el retardo almacenado, y la
  coloración de pared usa la duración media almacenada.

El límite de 4095 muestras sigue existiendo. La corrección mantiene el
objetivo de RT de pared en banda media cuando se recorta un retardo, pero
no recupera la distribución de modos que una sala grande requeriría. La
coloración de pared alta/baja sigue siendo compartida y aproximada.
Los retardos tempranos/tapa mantienen sus límites históricos.

## Medición del filtro

La prueba excita la función de filtrado real con un impulso, mide su
transferencia y la compara con la curva ISO sobre 20 Hz–16 kHz. A 44.1,
48 y 96 kHz:

- Error máximo a 2.64 m: **0.007 dB**.
- Error máximo hasta 32 m: **0.096 dB**.
- Error máximo a 100 m: **0.466 dB**.
- No hay refuerzo sobre 0 dB en la banda verificada, salvo tolerancia
  numérica de magnitud 1e-5.

Se verifica también la conversión de unidades contra tres valores ISO
independientes, la dirección del cambio con humedad, limpieza y bypass, y
que el extremo de amount 16×/100 m a 96 kHz permanezca finito.
La rejilla, truncación FIR y fase mínima son aproximaciones: no se afirma
exactitud para todas las condiciones meteorológicas, recorridos arbitrarios
o la dispersión de fase física completa.

## Audición A/B

Archivos en **`C:/SHARED/Aire-fisico`**. Las dos variantes tienen el mismo
ajuste de proximidad; A conserva la propagación/sala anteriores, B añade
la absorción ISO y la corrección de duración almacenada del FDN.

- `Notas-A-B.wav`: A y luego B para F#2, C3, C4, F4, C5 y C6, velocidad 90.
- `Nocturne-A-sin-absorcion-fisica.wav` y `Nocturne-B-aire-fisico.wav`.
- `Campanella-A-sin-absorcion-fisica.wav` y `Campanella-B-aire-fisico.wav`.

Las piezas mantienen los eventos MIDI, velocidades y mensajes de pedal,
44.1 kHz estéreo PCM24, bloques de 256, 200 ms de asentamiento silencioso
y 10 s de cola. No se normaliza ni añade EQ/compresión al exportar.
Medición de energía de banda con ventanas Hann de 1 s en ambos canales:

| Banda | Nocturne B−A | Campanella B−A |
| --- | ---: | ---: |
| 20–120 Hz | +0.0007 dB | +0.0008 dB |
| 120–250 Hz | −0.0035 dB | −0.0019 dB |
| 250–1000 Hz | +0.0060 dB | +0.0037 dB |
| 1–4 kHz | −0.0244 dB | −0.0400 dB |
| 4–8 kHz | −0.1657 dB | −0.1364 dB |
| 8–16 kHz | −0.6434 dB | −0.6017 dB |

Estas medidas incluyen la sala y las interferencias entre caminos; no son
la absorción de una sola onda. Al retirar el término de aire antiguo y
separar paredes, algunas bandas pueden subir ligeramente aunque cada filtro
de propagación sea pasivo. No se modificó el refit para compensar ese cambio.

El bypass nativo `AIR_PROPAGATION=0` reproduce **muestra por muestra** el
renderer anterior en la secuencia completa de seis notas, conservando tanto
el comportamiento antiguo de sala como los caminos directos. Diferencia
máxima: **0**. Los hashes de binarios, MIDI, eventos, audio y medidas están
en `mediciones.json`.

## Verificación

- **122 pruebas nativas aprobadas**, 75 diagnósticos ignorados.
- Huellas actualizadas deliberadamente para aire + proximidad:
  `3c15008f4f349d6f` y `f5f7192f2b5ae4c5`; ambas aprobadas.
- WASM compilado: arranque a 44.1 kHz/256 y equivalencia de una instancia
  contra cuatro trabajadores aprobados. No se cambió la reserva de pila
  del componente (16 MiB).
- La prueba de presupuesto que construye el procesador pasa con la pila
  nativa de 8 MiB configurada por el repositorio. El ejecutable de pruebas
  lanzado fuera de Cargo requiere `RUST_MIN_STACK`; sin él, usa 2 MiB y
  puede desbordar al construir este instrumento por valor.
- Ganancias de pared de una línea sin recorte y otra recortada a 96 kHz
  dan el mismo RT objetivo en banda media.
- `.rfplugin` publicado conserva SHA256
  `ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

La prueba WASM no garantiza cualquier polifonía/dispositivo. Se añade trabajo
por camino activo; el gobernador de calidad y los límites previos del modelo
siguen vigentes. La documentación principal se actualiza en `PIANO_MODEL.md`.

Reproducir audición (dependencias Python: numpy, soundfile):

```powershell
python tools/audit-concert-grand-air.py --baseline-binary local/concert-grand-air/baseline-renderer.exe --candidate-binary local/concert-grand-air/candidate-renderer.exe --work local/concert-grand-air/audition --output C:/SHARED/Aire-fisico --proximity-audit C:/SHARED/Proximidad-120Hz
```

Los WAV y binarios permanecen fuera de Git. El bypass es de investigación;
no se añade un control de clima ni se cambia el layout de sesiones/programas.
