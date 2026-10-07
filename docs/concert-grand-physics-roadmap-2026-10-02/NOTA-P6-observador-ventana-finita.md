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
Los controles sintéticos anteriores prueban 650 taps con tres modos.
El ensayo real añadido debajo mide dos bloques angulares con una base
de observación de rango 324 sobre la dinámica de 606 coordenadas; aún
no sustituye el certificado completo ni mide un contacto largo. No se adopta
por defecto ni se promete aceleración. Las tolerancias de comparación son
comprobaciones numéricas, no cotas rigurosas de aritmética de intervalos.

Antes de certificar un recorrido físico: comprobar el replay del puerto
contra sus extremos grabados, enlazar el factor autenticado y el almacenamiento
independiente, comprobar flujo contra el observador anterior y medir coste.
El helper por sí solo comprueba dimensiones/valores, no certifica que una
memoria final dada proceda de ese puerto. No inferir esa relación del balance
mecánico. Mantener presupuesto para espectro del puerto, bloque espectral,
factor angular y estados; no materializar toda la historia de secciones.

## 2026-10-07 — Sondas en el coast refinado real

[Método, resultados y límite fallido](RESULTADOS-P6-coast-refinado.md).
`tools/concert_grand_window_field_probe.py` enlaza las memorias inicial/final
y puerto del recorrido físico de 96/192 modos de cuerda, tabla/aire completos.
El certificado establecido sigue recorriendo todos los 384 bloques angulares
y contabilizando almacenamiento y flujo independientes con residuo acotado.
Las sondas son adicionales: bloques 0/192, tres tasas, dos FFT por ventana,
12 comparaciones. La vista no modifica proveedor ni R físicos; pruebas
sintéticas verifican equivalencia exacta del certificado con y sin vista.

Replay completo del certificado: 0; replay proyectado de sonda máximo
5,11995e−15. Campo complejo: 2,25109e−13; flujo por Parseval: 1,47126e−13,
frente a 1e−10. Memorias y padding se conservan. La base de observación
es 324/606, no una reducción de la dinámica; el certificado completo acota
su residuo. El resultado de las sondas no extiende ese certificado a todo
el método espectral ni al contacto de 3 ms.

Kernel espectral local: 0,276–0,874 s; producto temporal con inputs guardados:
0,0257–0,0793 s; replay/preparación: 0,304–1,690 s. Para cada pareja
bloque/ventana el kernel espectral cuesta 10,26–13,80 veces el producto
temporal. Generación/proyección comunes excluidas; el replay temporal se
comparte entre bloques. El certificado con sondas dura 1123,168 s y el
ensayo completo 2231,768 s, pico 4,853 GiB. No demuestra aceleración de
ventanas largas ni resuelve todavía la historia de 27 GiB del contacto.

El coast pasa **15/16**: la radiación 128→256 kHz cambia 0,274024% con
cotas, por encima del límite 0,2%; los demás controles pasan. Las sondas
correctas tampoco justifican aprobar ese paso temporal. Sigue comprobar
256→512 kHz con el mismo endpoint y límites, conservando este fallo.

## Otra vía pendiente para contactos repetidos

Este observador no acelera el contacto que está ejecutándose. La dinámica
todavía contrae R completa por cada paso. Para un banco de una cuerda,
`MovingBridge.physical/fields/pull_gradient` muestra que el acoplamiento
vertical con la tabla pasa por el escalar `b = phi.T B` y una reacción en
dirección phi. La tabla y el aire son lineales; la cuerda y el fieltro no.

Se puede estudiar una respuesta impulsional discreta del subsistema lineal
para reutilizarla entre contactos de la misma geometría/tasa, conservando
la colisión no lineal. Antes de implementar esa eliminación hay que derivar
el puerto con la **inercia compartida**: el levantamiento aporta masa a la
tabla y términos cruzados con la aceleración de cuerda. La reacción no es
sólo tensión ni fuerza del martillo. Además, la ventana debe ser causal,
con su término instantáneo implícito y memoria inicial, sin periodizarla.

Sería un método nuevo, aún sin implementar ni medir. Deberá reproducir
trayectorias físicas, memoria acústica, fuerza, trabajo medido del puerto
y presión del integrador completo antes de reemplazarlo. Si hay ahorro,
será para múltiples contactos que comparten operador, no automáticamente
para una primera nota ni para geometrías o tasas distintas.
