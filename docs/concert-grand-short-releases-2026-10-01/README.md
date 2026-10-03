# Concert Grand: notas cortas de La Campanella

Base aceptada y comprometida: `1bb8436`, proximidad y absorción del aire.
Esta variante de repetición queda **sin commit y pendiente de audición**.
El usuario confirmó después que el click sigue igual y lo localizó en el
quinto grupo de `Notas-cortas`, C#6 (MIDI 85), cerca de 8 s. Reutilizar la
cuerda no resolvió ese síntoma; no debe presentarse como su solución.
El `.rfplugin` de `C:/SHARED` conserva su versión y contenido publicados.
El cambio anterior del host desktop queda fuera de este trabajo.

## Pasaje y diagnóstico

El usuario señaló La Campanella a **01:50**. Se conserva el MIDI completo desde
el cuadro cero, incluyendo pedales, voces y sala anteriores al recorte de
**01:46–01:55**. En este recorte, 01:50 queda a los cuatro segundos.

Entre 109.5 y 111.5 s aparecen teclas mantenidas 16–103 ms, predominantemente
23–48 ms, sin sustain. Se repiten F#5, B5, C#6 y F#6 en aproximadamente
77–119 ms. MIDI 83 es B5 con la convención C4=60.

Se hicieron ablaciones independientes del ruido de liberación, pedal,
excitación estructural de contacto, duplex, umbral de descarte, rebotes del
apagador y duración de llegada de la repetición. Los archivos de diagnóstico
están en `C:/SHARED/Click-notas-cortas/diagnostico` y `diagnostico2`.

En 108–113 s, retirar el ruido de liberación no reduce el RMS sobre 3 kHz
(0.002157 actual, 0.002180 sin ese ruido); retirar el contacto tampoco
(0.002158). La traza por etapas alrededor de 110.104 s sitúa el ruido
directo del mecanismo muy por debajo de la contribución de cuerdas/tabla.
Estas pruebas **no demuestran inaudibilidad** del mecanismo; no justifican
atribuirle todo el click ni recortar globalmente el brillo del piano.
La audición de la ablación sin rebotes se solicitó por separado; la variante
final conserva los cinco rebotes y los niveles de ruido de la base.

## Diferencia concreta del modelo

La selección de una voz existente exigía `held || sustained`. Cuando un
apagador ya había terminado de asentarse, una repetición tomaba otra voz y
dejaba apagarse la anterior. La misma cuerda quedaba representada por dos
bancos de osciladores. Esto ocurre en las repeticiones cortas sin pedal de
este pasaje; alargar `MERGE_RAMP_S` sólo cambia las voces que sí se fusionan.

La reconstrucción finita con fase conservada ahora puede reutilizar también
una voz liberada. `Voice::damp` lleva un registro de los factores de pérdida
añadidos a los polos verticales y horizontales. Al levantar ese apagador:

- se cancela cualquier contacto de rebote todavía pendiente;
- se retiran únicamente esas pérdidas de los **polos** de rotación;
- se conservan las amplitudes y fases actuales, incluida la energía perdida;
- el golpe nuevo llega por la reconstrucción finita ya existente.

El registro se reinicia al tomar una voz fresca. Los polos permanecen dentro
del círculo unidad. No se cambia el formato de reporte entre instancias ni
el estado público del plugin. El bypass de reconstrucción antigua conserva
su selección anterior para voces liberadas.

Esto corrige la duplicación de la cuerda. Sigue siendo una reconstrucción del
contacto precalculado, no una colisión no lineal inicializada desde la cuerda
en movimiento. **No se afirma que resuelva todo el click subjetivo**: el
ataque de una tecla nueva queda idéntico y requiere otra investigación si el
problema también se escucha allí.

## Comparación y verificación

En `C:/SHARED/Click-notas-cortas`:

- `Campanella-A-actual.wav`: base del commit.
- `Campanella-B-misma-cuerda.wav`: variante, mismo recorte y ganancia.
- `Campanella-A-B.wav`: A, medio segundo de silencio, B.
- `Notas-cortas-A.wav` / `Notas-cortas-B.wav`: C4, B4, F#5, B5, C#6, F#6,
  C#7, tres golpes de velocidad 70, teclas mantenidas 28 ms cada 90 ms.
- `mediciones.json`: hashes de ejecutables, MIDI, raws y diferencias espectrales.

Sin normalización por pico ni EQ de exportación; ganancia común 0 dB (la
calibración de salida ya está en el motor). El ataque fresco de C4 hasta su
primer note-off es idéntico muestra por muestra. El pico del pasaje cambia
de 0.243146 a 0.242988; la energía por bandas cambia entre -0.196 y +0.067 dB.
Son diferencias de render, no una puntuación de audibilidad del click.

- **124 pruebas nativas aprobadas**, con pila nativa de 8 MiB.
- Dos pruebas nuevas: levantar el apagador no recupera energía ni modifica
  el estado instantáneo; cinco repeticiones cortas mantienen una sola voz,
  con polos estables, en seis notas del pasaje.
- Huella sin liberaciones: `3c15008f4f349d6f`, sin cambio.
- Huella con pedales/liberaciones: `76ef3169e4a9237c`; actualizada desde
  `f5f7192f2b5ae4c5` porque ahora cambia la selección de voces ya apagadas.
  Ambas pruebas de huellas aprobadas por separado.
- WASM compilado; instancia inicial y equivalencia de una/cuatro instancias
  aprobadas. Se añade una prueba entre instancias de B5 con tecla mantenida
  29 ms y liberada 61 ms, para atravesar el apagador asentado antes de repetir.

Reproducir (numpy, scipy, soundfile):

```powershell
python tools/audit-concert-grand-short-releases.py --baseline-binary local/concert-grand-short-click/baseline-renderer.exe --candidate-binary local/concert-grand-short-click/candidate-renderer.exe --baseline-audio local/concert-grand-air/audition/Campanella-B/performance.f32 --events local/concert-grand-air/audition/Campanella-B/events.txt --work local/concert-grand-short-click --output C:/SHARED/Click-notas-cortas
```

Los binarios y renders quedan fuera de Git. Los prerolls temporales de la
primera ablación se eliminaron después de exportar sus WAV e informe debido
a falta de espacio en C; pueden regenerarse con el ejecutable base. También
se retiraron dos PDB de compilación de pruebas, regenerables por Cargo.
