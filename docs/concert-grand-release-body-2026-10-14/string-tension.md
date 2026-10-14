# Tensión: sumar el estiramiento de cuerdas separadas

Base de escucha aceptada: acoplamiento común `7d7546e5`, aceptación registrada
en `412e84c5`. El usuario responde «Suena bien, sigamos». El cambio siguiente
corrige una inconsistencia adicional; no se presenta como una mejora audible
de cuerpo y sostenido. Su escucha comparada queda disponible.

## Corrección común

El comentario de `Voice::tension_step` describía una suma de pendientes al
cuadrado, pero el código calculaba, para cada parcial,

`w² (s0 + s1 + s2 + sh)²`.

Eso introducía productos cruzados entre cuerdas diferentes y entre
polarizaciones ortogonales. Dos cuerdas moviéndose en oposición podían
cancelar el estiramiento estimado, además de su proyección acústica. Ahora:

`w² (s0² + s1² + s2² + sh²)`.

Los distintos modos de una cuerda tienen integrales de pendiente ortogonales;
las distintas cuerdas tienen integrales independientes. Las dos componentes
transversales aportan cuadrados separados a la extensión geométrica. La
coordenada de bloom queda fuera porque es una construcción de ataque. La
relación cuadrática entre desplazamiento transversal y extensión se deriva
en [Woodhouse, movimiento longitudinal](https://euphonics.org/7-4-1-nonlinearity-and-longitudinal-string-motion/).
Nuestra prueba verifica la reducción modal mediante integración espacial
numérica independiente, incluso con cuerdas en oposición y bloom grande.

Se conserva la aproximación **de tensión agregada por voz**, su ganancia,
suavizado, límites y leyes previas. No se resuelve una tensión independiente
en unidades SI para cada cuerda. Tampoco se modifica la excitación del banco
longitudinal, la captación, la afinación, el puerto del puente ni las tablas
de pérdidas. No se introduce selección de notas ni ajuste de registros.
El mismo cálculo se usa en todas las voces; eso no implica que cambien todas
las muestras, porque su efecto depende del movimiento y de la ganancia previa.

## Resultado: efecto pequeño, sin mejora de cuerpo demostrada

88 teclas × velocidades 60/110: **176 pares** contra la base aceptada,
tecla sostenida dos segundos y salida de cuatro. Muestras finitas, máximo
pico B **0,5247**. En las cinco ventanas de nivel analizadas, el máximo
cambio absoluto es **0,01241 dB** (A0/v110). No hay ataques que cambien 1 dB.
Los crudos son idénticos en 111 de los 176 pares; cambian 15 a velocidad 60
y 50 a velocidad 110. Es efecto de las leyes y precisión existentes, no una
lista de notas seleccionada por esta corrección.

El error medio de caída H1/H2/H3 contra los doce casos de referencia conocidos
pasa de `[6,47490; 3,48369; 7,85906]` a
`[6,47494; 3,48357; 7,85924]` dB: esencialmente igual. En particular, C4/v60
sigue en **−14,312 dB** de caída H1 y D#4/v60 en **−23,332 dB**.
Esta corrección no resuelve sus déficits de cuerpo.

Diez notas se mantienen doce segundos y se liberan durante dos. El máximo
cambio absoluto de sus ventanas RMS es **0,04435 dB**, sin crecimiento
numérico. No se usa esa prueba como validación de energía física en julios.

El guion nativo de 14 s, tres ejecuciones alternadas por versión, tarda
**0,71670 s** de mediana en A y **0,72311 s** en B (**+0,89%**). Las series
se solapan; no permite atribuir una diferencia precisa de CPU. Incluye I/O.
No hay osciladores ni estados nuevos: la operación adicional se realiza en
el cálculo de tensión cada 32 muestras.

## Audios completos y verificación

Carpeta `C:/SHARED/Piano-Tension-separada-2026-10-14/`:

- `Campanella-B-tension-separada.wav`: **4:40**, render **27,46 s**.
- `Nocturno-B-tension-separada.wav`: **3:53**, render **26,43 s**.
- `Notas-A-acoplamiento-aceptado.wav` / `Notas-B-tension-separada.wav`:
  secuencia de notas iguales, velocidades 60 y 110.

Los A completos son los B de la comparación de acoplamiento aceptada:
`C:/SHARED/Piano-Acoplamiento-general-2026-10-14/`.
Mismos MIDI completos, velocidades, tempos y controladores, una instancia
continua, pedal final conservado, ganancia de exportación 1, sin normalizar
ni ecualizar. WAV float32 estéreo de 44,1 kHz, idénticos al crudo.
Picos de las piezas B: Campanella **0,92363**, Nocturno **0,33713**.

**82 pruebas habituales pasan**, ninguna falla, 69 especiales ignoradas.
Las dos huellas se actualizan deliberadamente por la nueva estimación de
tensión y pasan por separado: `e4abcfd02a5757c1` / `dfb7f193962235e0`.
Compila WASM con SIMD128 y pila de 8 MiB; no se prueba aquí su ejecución.

El barrido acústico se congeló antes de actualizar las dos constantes de
huellas de prueba. La verificación final demuestra que sólo cambiaron esas
constantes y su comentario en `cfg(test) mod bench`, que todo el resto del
fuente es idéntico y que un nuevo ejecutable produce el guion de 14 s
**idéntico byte por byte**. Se conservan ambos hashes de fuente.

- [Barrido, piezas, hashes y mediciones](string-tension.json).
- [Pruebas finales, compilación y equivalencia](string-tension-verification.json).
- Reproducción del barrido: `tools/audit-concert-grand-string-tension.py`.
- El verificador posterior conserva la evidencia del cambio exclusivo de
  constantes de prueba: `tools/verify-concert-grand-string-tension.py`.

La base aceptada sigue documentada por separado. Esta revisión queda en la
rama de desarrollo para escucharla, sin publicar ni instalar un paquete.
