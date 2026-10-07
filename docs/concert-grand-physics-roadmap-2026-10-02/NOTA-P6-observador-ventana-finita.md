# P6 — Campo Padé de una ventana finita sin historia de todas las secciones

Fecha: 2026-10-07. Prototipo separado del ensayo de resolución en curso.
No modifica los módulos autenticados de contacto, carga, presión ni certificados.

## Problema de memoria

Una historia nativa de 3 ms a 3072 kHz tiene 9216 pasos. Con 650 taps y
606 coordenadas, almacenar entradas reales de todas las secciones requiere
29.041.459.200 bytes (~27,0 GiB). El certificado angular actual puede
reproducirlas por bloques, pero hacerlo de nuevo por cada bloque angular
tiene un coste que debe medirse.

El observador propuesto conserva sólo historia del puerto y estados de
ambos extremos. Calcula las entradas centradas de las secciones en bloques
de frecuencia; no reinicia, reduce ni modifica la dinámica acústica.

## Identidad finita

Una sección Padé integrada por midpoint tiene matrices discretas constantes:

`x[k+1] = M x[k] + B u[k]`, `y[k] = C x[k] + D u[k]`.

Definir `U = Σ(k=0…N−1) z^k u[k]` y análogamente `X`, con
`z = exp(−i 2π j / L)` para FFT de tamaño L ≥ N. La suma desplazada da

`(I − z M) X = x[0] − z^N x[N] + z B U`.

Por tanto,

`Y = (D + z C (I − z M)^−1 B) U + C (I − z M)^−1 (x[0] − z^N x[N])`.

Se aplica sección a sección, conservando cada estado vivo. Incluso cuando
el tamaño FFT es N, no se puede eliminar `x[0]−x[N]`. Cuando hay padding,
tampoco se puede sustituir `z^N` por 1. Omitir ambos extremos equivale a una
hipótesis periódica incorrecta para estos contactos.

Para factor angular real H, el campo discreto de ventana es
`F[j] = Σ(tap, modo) H[tap, canal, modo] U_tap[j, modo]`.
Parseval da su flujo integrado `dt/(2 L) Σ_j peso[j] |F[j]|²`, con pesos
1 en DC/Nyquist y 2 en el interior de una rFFT. Es el mismo observable
positivo del certificado temporal; no una estimación como resto del trabajo.
Al integrar el método habrá que conservar también su almacenamiento independiente.

## Implementación y controles actuales

`tools/concert_grand_pade_window_spectrum.py` implementa sólo la reconstrucción
de las entradas centradas, con bloques explícitos y todos los modos. Las
matrices M/B/C/D proceden de las ecuaciones existentes; la prueba principal
usa el replay escalar independiente de Padé, no la misma fórmula.

Tres pruebas pasan en 0,337 s:

- Entradas de todas las secciones y flujo por Parseval frente a replay
  temporal a 32/1536/3072 kHz; memoria inicial no nula, 37 pasos, tamaños
  FFT 37/64/96 y bloques 1/7/completos. Diferencias relativas <2e−10.
- Tamaño completo de **650 taps**, tres modos sintéticos, contacto a
  1536/3072 kHz: ventanas de 128 pasos, FFT 192, bloques de ocho frecuencias,
  primero memoria fría y después continuidad viva. Diferencias <2e−10.
- Rechazo de tamaños/ventanas/bloques/espectros inconsistentes. El control
  que omite memorias tiene error >1%, como debe ocurrir.

## Límites y siguiente paso

Es un prototipo de observación, no un certificado del banco completo.
Los 650 taps se prueban con tres modos; todavía no se ha medido su tiempo
y memoria con 606 modos y los factores angulares completos. No se adopta
por defecto ni se promete aceleración. Las tolerancias son comprobaciones
numéricas, no cotas rigurosas de aritmética de intervalos.

Antes de certificar un recorrido físico: comprobar el replay del puerto
contra sus extremos grabados, enlazar el factor autenticado y el almacenamiento
independiente, comprobar flujo contra el observador anterior y medir coste.
El helper por sí solo comprueba dimensiones/valores, no certifica que una
memoria final dada proceda de ese puerto. No inferir esa relación del balance
mecánico. Mantener presupuesto para espectro del puerto, bloque espectral,
factor angular y estados; no materializar toda la historia de secciones.
