# P6 — Primera nota física sostenida: C4/124

Fecha de trabajo: 2026-10-05. Rama `codex/concert-grand-restrike`, base
`f124550`. El usuario pidió priorizar una primera nota audible después de
los controles de contactos de 59 modos. Se adelanta esa escucha diagnóstica;
la comparación estructural 59→90 queda pendiente, no se da por aprobada.
No se modifica ni se empaqueta el motor de fábrica.

## Qué se conserva

- C4, velocidad MIDI 124; mismo alambre/fieltro y misma geometría/pérdidas
  hipotéticas P2 del banco de contactos refinado. No identificación Steinway.
- 72 modos transversales en cada dirección y 144 axiales; una cuerda
  vertical excitada, sin inventar el unísono de tres cuerdas.
- 59 modos físicos de tabla hasta 1193,507913 Hz; superficie 192×144,
  factor acústico 128 kHz, 38232 estados acústicos.
- Potencial no lineal de cuerda y fieltro, integración AVF y midpoint de aire;
  el martillo que ya se separó sigue su trayectoria. Separación no convierte
  toda la cuerda en un sistema lineal.
- Posición, velocidad, estados de onda y relajación al final del contacto
  frío de 4 ms se conservan exactamente al iniciar la continuación.

Se reutiliza el archivo padre de coeficientes de 90 modos; no se genera
una copia del factor. Datos únicos en
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-primera-nota`. Informes y fuentes
tienen su única copia en este repo.

## Aceleración de las mismas ecuaciones

`concert_grand_fast_contact.py` mantiene la implementación escalar anterior
como referencia. Evalúa la cascada allpass por `lfilter`, contrae residuos
en BLAS y evita calcular Hessianas al comprobar cada residuo. El tangente
constante sólo precondiciona el solve; se conserva el residuo no lineal
completo y su tolerancia. Hay retorno al tangente completo si no converge.
Una evaluación de gradiente aceptada por la búsqueda se reutiliza.

El tangente AVF respecto al extremo es H/2 para un potencial cuadrático;
por ello su contribución al precondicionador es dt²·H/4. El primer intento
usaba dt²·H/2: conservaba las ecuaciones al converger, pero necesitaba
demasiadas iteraciones. Se conservan sus mediciones en `perfil.json` y
`ataque-acelerado.json`. Su continuación se detuvo a unos 24 ms sin guardar
trayectoria ni audio. No se presenta ese intento como resultado sostenido.

La versión corregida se mide en `perfil-tangente-consistente.json`:

| Control vivo, 256 pasos a 768 kHz | Resultado | Límite |
|---|---:|---:|
| Diferencia máxima de estados, norma relativa | 9,108e−12 | 2e−8 |
| Diferencia de trabajo, norma relativa | 2,157e−15 | 2e−8 |
| Referencia / acelerado, dinámica | 10,391 / 0,812 s | Diagnóstico |
| Aceleración de dinámica en ese prefijo | 12,80× | Diagnóstico |

Es un prefijo vivo de 0,333 ms, no una garantía de coste durante todo el
sostenido. Las tres pruebas de `test_concert_fast_contact.py` comprueban
gradiente AVF, estados/trabajos contra referencia con memoria activa,
balance independiente de energía y desacoplamiento, a tres tasas.
La verificación del ataque completo de la versión corregida se registra
en `ataque-tangente-consistente.json`: diferencias relativas de B/V
5,083e−14 / 3,023e−13, estado final máximo 1,779e−11 y balance independiente
de energía 1,405e−14. Pasan los tres controles, sin retorno al tangente
completo; integra los 4 ms en 71,188 s con observación angular incluida.

## Micrófonos y audio

Dos cápsulas en (0,55; 0,5; 0,66) y (0,85; 0,5; 0,66) m; misma fuente
cargada y misma superficie constante que P6. Rayleigh en semiespacio
superior con baffle rígido. No sala, patrón de micrófono, EQ ni nueva pérdida.

La integral de cada punto de cuadratura se aproxima por un retardo
fraccionario de cuatro taps. La aceleración se transforma mediante los
momentos analíticos de la misma interpolación Hermite C1 del banco anterior.
No se diferencia una nota recortada ni se agrega un impulso al comienzo.
La conversión Fourier Pa·s → muestras Pa incluye la tasa de captura.

| Control de salida | Resultado | Límite |
|---|---:|---:|
| Retardo fraccionario vs Green continuo | 0,011469% | 0,02% |
| Cuadratura 4 vs 8 dentro de las celdas | 5,385e−14 relativo | 1e−5 |
| Superficie 192×144 vs 256×192 | 0,106445% | 1% |

Se usan siete sondas entre DC y 6 kHz. `microfonos.json` conserva todos los
inputs y hashes; el kernel único se guarda en `Microfonos-128k.npz`.
Las cuatro pruebas de `test_concert_cell_audio.py` verifican transformadas
contra el observador analítico previo, distancia/fase, escala de presión,
ausencia de delta en velocidad constante y rechazo de grillas inválidas.

## Continuación y escucha

Completado: continuación a 768 kHz hasta 250 ms, con captura B/V a 128 kHz.
La integración y los tres controles de almacenamiento tardan unos 577 s;
guardar/comprimir la trayectoria agrega un pequeño coste. No hay nuevos
contactos del martillo ni retornos al tangente completo. El almacenamiento
total cae de 0,046906871 J al comenzar la continuación a 0,040382775 J al
terminar; calor material 0,004869418 J y radiación inferida 0,001654678 J.
Los tres valores de radiación inferida son positivos y crecientes.

La presión de captura 64 vs 128 kHz difiere 2,359e−7 relativo
(0,00002359%) en 60–6000 Hz, bajo el límite 0,1%. Se exporta un WAV estéreo
a 48 kHz con ganancia común **0,25 full scale/Pa**, sin ajuste por canal.
Pico físico 2,021643 Pa; pico del WAV 0,505326, sin clipping. Contiene
100 ms de silencio inicial, la fuente y su propagación, y 300 ms finales;
duración del archivo 0,653479 s. Un fade terminal de 8 ms evita el corte de
edición; no representa física del apagador ni una nota soltada.

Primera escucha única:
`C:/SHARED/Fisica-pendiente-2026-10-05/P6-primera-nota/C4-124-Piloto-fisico-250ms.wav`.
Informe: `medidas-P6-primera-nota/escucha-250ms.json`. Pasan los tres controles
del ataque acelerado, dos de referencia viva, tres de micrófonos y el de
captura. Pasan las siete pruebas nuevas. Esto establece una primera escucha
diagnóstica de una cuerda, no una mejora audible elegida ni un piano completo.

El sostenido integra las mismas ecuaciones. Su radiación angular completa
es demasiado costosa para evaluarla en todos los pasos de este primer
render: se registran almacenamiento positivo, calor material y trabajo.
La radiación inferida por diferencia de energía se identifica como tal;
no se confunde con un balance cerrado por una segunda integral independiente.
El ataque completo y los tests pequeños sí integran la radiación angular.

## Reproducción

Python 3.12.14 / NumPy 2.3.5 / SciPy 1.18.1; un hilo BLAS y sin caché de
bytecode. Se usa el Python del runtime de Codex con `PYTHONPATH` del venv
de refit conservado. Los ejecutables están en `tools/`:

1. `render-concert-grand-physical-note.py --phase profile --probe-steps 256`
   y `--phase onset-check`: comparten los inputs siguientes y usan informes
   nuevos dentro de `docs/.../medidas-P6-primera-nota/`.
2. `render-concert-grand-physical-note.py --phase coast --duration 0.25
   --rate 768000`: mismos inputs; `--output` es el NPZ único de trayectoria
   y `--report` es `sostenido-250ms.json` en el repo.
3. `observe-concert-grand-physical-note.py --phase prepare`: usa `--asset`,
   `--contact-report`, `--kernel` y `--report`; grilla/cantidad de modos se
   toman de la procedencia, sin suponer el banco anterior de 35 modos.
4. `observe-concert-grand-physical-note.py --phase listen`: además requiere
   `--trajectory`, `--trajectory-report`, `--kernel-report`, `--audio` y
   un `--report` nuevo. `--gain` es una única conversión Pa→full scale común
   a las dos cápsulas; en este ensayo `--gain 0.25`. No cambia las trayectorias
   ni el kernel.

Inputs físicos compartidos:

| Argumento | Ruta |
|---|---|
| `--input` | `C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input` |
| `--asset` | `C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz` |
| `--coefficients` | `C:/SHARED/Fisica-pendiente-2026-10-05/P6-tabla-1500-refinada/Wave128000-Cells192x144-Angle96x96-Modes90.npz` |
| `--contact-report` | `docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-contacto-1200-refinado.json` |
| `--contact-data` | `C:/SHARED/Fisica-pendiente-2026-10-05/P6-contacto-1200-refinado` |

Los SHA256 unen el banco validado, coeficientes, trayectoria sostenida,
kernel y audio. Para repetir un ensayo se eligen destinos nuevos; no se
reemplaza la trayectoria ni el kernel medidos. Los dos archivos de tests
nuevos se ejecutan con `unittest discover -s tools/tests -p <nombre>`.

## Qué sigue abierto

Esta primera escucha no valida fidelidad ni convergencia del piano completo.
Faltan refinamiento temporal del sostenido largo, comparación 59→90,
geometría/pérdidas identificadas, dos caras/borde, unísono, P4, graves,
notas/intensidades, acción/pedal, reducción de coste e integración del plugin.
La próxima decisión debe usar la nota audible como diagnóstico, conservar
estos límites y atender el coste antes de renderizar MIDI.
