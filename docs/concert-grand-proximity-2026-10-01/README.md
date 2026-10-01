# Concert Grand: recuperación de la proximidad

Auditoría del código local a partir de `15099ec` (RF-Concert Grand 0.172.0).
La ruta enviada en el informe externo corresponde a otro checkout; se verificó
el proyecto de `C:/Users/kalex/OneDrive/Documents/rackforge`.
La proximidad y el tratamiento posterior del aire fueron aceptados por el usuario
para commit el 1 de octubre. El `.rfplugin` publicado todavía no fue reemplazado.

## Verificación de las afirmaciones

| Afirmación | Resultado en este checkout |
| --- | --- |
| La ganancia sigue `PROXIMITY_STRENGTH*b*MIC_REFERENCE_M/direct_path`. | Confirmada en `tune_room`, por cápsula. `PROXIMITY_STRENGTH=0.35`, referencia 2 m, `b=0.6` y distancia 2.64 m en Concert. |
| `proximity_coeff` queda en cero y el filtro es inaudible. | Confirmada antes de corregirlo: sólo se inicializaba en `Default`; no había asignación posterior. Con estados inicialmente nulos, la recurrencia no podía aportar nada. |
| El comentario de Concert promete calidez de proximidad. | Confirmado: describe un patrón hacia figura de ocho para esa calidez, aunque el filtro estaba inactivo. Con la corrección corresponde a un efecto real, moderado y medido. |
| El comentario de `PROXIMITY_STRENGTH` se pegó a `MIC_SPACING_M`. | Confirmado y corregido, incluyendo las descripciones del registro de knobs. Había además comentarios del viejo par AB pegados a `SYMPATHY_RATE` y `Voice::last_out`; retirados. |
| El par temporal por voz ya no existe. | Confirmado: `e05b806` retiró el par que se sumaba en mono y creaba un peine. El estéreo actual viene del observador modal de tabla y de las dos cápsulas de sala. Width no crea un par AB de 0.9 m. |
| Tres imágenes van a cada canal según la documentación. | La documentación estaba desfasada: ambos canales leen las seis imágenes con caminos y respuestas polares propios. |
| La referencia dice 2.5 m pero el código usa 2 m. | Confirmado y corregido. La referencia es un factor de distancia antes de patrón y preamplificador, no una garantía de ganancia final unitaria. |
| La documentación dice que la ambientación se mezcla a cero. | Confirmado como texto histórico obsoleto. Las rutas están activas; `the_staging_leaves_a_tail_that_dies` exige cola audible y decaimiento posterior. |
| Las seis líneas FDN se recortan y sus ganancias usan las longitudes sin recortar. | El segundo punto está confirmado; el primero requiere matiz: con el máximo compilado de 45000 m³ no se recorta ninguna a 44.1 kHz, una a 48 kHz y cinco a 96 kHz. Los retardos de reflexiones tempranas/tapa tienen el mismo límite y pueden alcanzarlo por separado. |

## Historial: fue una regresión

`git log -S proximity_coeff -- plugins/concert-grand/src/lib.rs` identifica:

- `4ceb1de`: introdujo y asignó `1-exp(-2*pi*120/fs)` en la cadena de captación.
- `8cb8562`: convirtió la captación escalar en dos cápsulas y eliminó esa
  asignación al reorganizar `tune_room`. Conservó el estado, la ganancia y la
  recurrencia de filtrado. Su descripción no anuncia desactivar proximidad.

No hay evidencia de una decisión de voicing para apagarlo deliberadamente.

## Corrección propuesta

Se calcula el coeficiente al preparar/retunar la sala, fuera del bucle de audio.
El filtro por cápsula implementa:

```text
low[n] = low[n-1] + a*(direct[n] - low[n-1])
out[n] = direct[n] + g*low[n]
a = 1-exp(-2*pi*120/fs)
g = PROXIMITY_STRENGTH*b*MIC_REFERENCE_M/direct_path
H(z) = 1 + g*a/(1-(1-a)*z^-1)
```

Es una aproximación de captación: polo de paso bajo cerca de 120 Hz, ganancia
DC `1+g` y agudos aproximadamente unitarios. El polo no es el punto medio de
un shelf independiente de la ganancia. No se identifica un micrófono real ni
se deriva una esquina móvil de `c/(2*pi*r)`.

Se filtra la mezcla de camino próximo que ya llegaba a esta etapa: incluye
tabla y otras rutas directas/tempranas. La cola difusa FDN se suma después.
El estado sigue al input también en omni/ganancia cero para que un cambio de
patrón no reanude una memoria congelada. Reset y prepare borran los estados.
No se modificaron la física de cuerdas/fieltro, las ganancias del FDN ni los
coeficientes del refit para compensar el sonido de este filtro.

## Mediciones y escucha

Captación Concert de fábrica, antes del preamplificador y techo de salida:

| Frecuencia | Antes | Después a 44.1 kHz |
| --- | ---: | ---: |
| 20 Hz | 0 dB | +1.238697 dB |
| 120 Hz | 0 dB | +0.685240 dB |
| 1 kHz | 0 dB | +0.032234 dB |
| 6 kHz | 0 dB | +0.012179 dB |

La respuesta se mide inyectando senos en la función usada por el bucle real,
tras asentamiento, y comparando su energía con una evaluación independiente
de la transferencia discreta a 44.1, 48 y 96 kHz; tolerancia 0.015 dB.
No son medidas de las bandas de una interpretación musical.

En las piezas completas, la energía de banda 20–120 Hz sube **0.672 dB en
Nocturne** y **0.682 dB en Campanella**. En 120–250 Hz sube aproximadamente
0.405 dB; en las bandas superiores cambia menos de 0.06 dB. El nivel RMS total
sube sólo 0.079/0.072 dB, respectivamente. Las reflexiones y la cola que no
atraviesan el shelf reducen el efecto relativo frente a la medición aislada.
Acercar un patrón de gradiente aumenta el refuerzo; el cambio no debe
interpretarse como una solución al cuerpo faltante en todo el registro.

Audios en `C:/SHARED/Proximidad-120Hz`:

- `Notas-A-B.wav`: A y luego B en cada nota/velocidad; F#2, C3, C4, F4,
  C5 y C6, velocidades 60 y 110. Sin ajuste de nivel entre variantes.
- `Notas-A.wav`, `Notas-B.wav`: secuencias separadas.
- `Nocturne-A-actual.wav`, `Nocturne-B-proximidad.wav`: piezas completas.
- `Campanella-A-actual.wav`, `Campanella-B-proximidad.wav`: piezas completas.

A usa el renderer congelado que produjo la entrega 0.172.0; B usa el código
corregido. Los MIDI, eventos, velocidades y pedales conservan sus hashes;
estéreo a 44.1 kHz, PCM 24 bits, bloques de 256, asentamiento silencioso de
200 ms y cola de 10 s. No se normaliza ni se añade EQ/compresión al exportar.
El control completo con patrón omni (`b=0`) da **diferencia máxima cero** entre
renderers, incluso con todas las notas de la secuencia.

## Pruebas y límites

- 118 pruebas nativas aprobadas, 75 diagnósticos ignorados.
- Tres pruebas nuevas: transferencia medida, patrón/distancia/ganancia/reset,
  y llegada del filtro al audio completo.
- Dos huellas actualizadas deliberadamente para el candidato audible:
  `bf489fe94cfceda2` y `fc689ced9f1f47aa`; ambas verificadas.
- Compilación WASM y pruebas de arranque y equivalencia entre una instancia
  y cuatro trabajadores aprobadas, contra el WASM candidato compilado.
- `.rfplugin` de 0.172.0 en SHARED conserva su SHA256
  `ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

`fdn-storage.json` calcula las duraciones pedidas y almacenadas con geometría
2.4:1.6:1, volumen/3.84, camino medio `4V/S`, velocidad 343 m/s y los seis
spreads del código. Su cociente almacenado/pedido es la desviación del RT
implícito de una línea por su ganancia de banda media, **no una medición del
RT60 global de la matriz con todos sus filtros**. A 96 kHz y 45000 m³ ese
cociente cae a 0.460 en la línea más larga; en Concert 311.3 m³/44.1 kHz no
hay recorte FDN. Es un defecto separado, documentado, sin cambiarlo en esta
audición para que A/B sólo evalúe proximidad.

Reproducción de la audición:

```powershell
python tools/audit-concert-grand-proximity.py --baseline-binary local/concert-grand-restrike-contact/production-renderer.exe --candidate-binary local/concert-grand-proximity/candidate-renderer.exe --work local/concert-grand-proximity/audition --output C:/SHARED/Proximidad-120Hz --baseline-midi-dir C:/SHARED
```

Los binarios, grabaciones y WAV quedan fuera de Git. Este informe y el ledger
`PIANO_MODEL.md` describen la propuesta aún no publicada.
