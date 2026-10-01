# Contacto, pérdidas y radiación: pruebas posteriores al 14B

## Resultado

Hay dos problemas distintos: la excitación produce un exceso de armónicos altos
en parte del registro y la fundamental de C4 pierde demasiado cuerpo después del
ataque por la carga del puente. Cambiar solamente la excitación no corrige esa
pérdida. Las pruebas ya permiten separar esas causas dentro del modelo.

Se implementó una ley experimental de contacto disipativo para todos los
martillos. La audición está en `C:/SHARED/Refit-contacto-tabla/`.
**Todavía no es una sustitución aprobada del 14B:** mejora principalmente los
agudos y presenta regresiones en graves. No se instaló un plugin nuevo ni se
cambiaron los programas de producción.

## Qué se midió

- 17 perfiles, incluidos controles de una variable, sobre nueve notas originales
  entre C3 y F#7, a velocidades 60 y 124. Cada combinación empieza con un
  instrumento nuevo, 200 ms de calentamiento, 44,1 kHz y bloques de 256.
- Trazas del contacto, energía del martillo/cuerda, amplitudes modales, carga del
  puente, tabla, ambos observadores y contribuciones auxiliares. Las trazas de
  audio cubren los primeros 300 ms; las salidas completas tienen 12 s sostenidos
  y 4 s después de note-off.
- La variante de contacto se comprobó después en **120 combinaciones: las 30
  notas originales, a velocidades 28, 60, 100 y 124**. Los 102 casos adicionales
  no participaron en la elección entre tres valores de disipación; pertenecen a
  la misma biblioteca, no a otro piano independiente.
- C4/F4/G4/C5 se comprobaron también a 60/84/124 contra el sampler congelado.
  F4 y G4 utilizan F#4 transpuesta −1/+1 semitono; C4 y C5 son muestras exactas.

Se conservaron micrófono a 2,64 m, damping de tabla 4,6%, unísono, pérdidas,
ruidos y reconstrucción de reiteraciones del 14B. Las comparaciones directas de
grabaciones usan siempre **32,843365 dB**, y las del sampler **26,401177 dB**.
No se recalibró la ganancia de la variante ni se normalizó cada nota. No se
añadió EQ ni compresión a los audios.

El error espectral utiliza ventanas de 512/2048/8192 muestras y una máscara que
incluye tanto energía original como energía excesiva del modelo. El resumen
inicial promedia etapas de 0 a 1,5 s. No es una puntuación perceptual ni un
porcentaje de fidelidad. Las etapas posteriores y el cuerpo se guardan aparte.

## Experimentos y conclusiones

### 1. Contacto truncado y continuación de los parciales

El 14B integra el contacto hasta 8 kHz cuando hay al menos cuatro modos. C7
queda completamente en la receta analítica. Esa receta prolonga el espectro
desde un único parcial en la frontera. Integrar todos los parciales audibles
con `SIM_MIN_MODES=1` y `SIM_TOP_HZ=19000` evita esa frontera para las notas
medias/agudas, pero **por sí solo no arregla el exceso agudo**. En graves sigue
existiendo el límite de 144 modos del solver.

### 2. Pérdidas del puente

Para C4 a velocidad 124, el descenso de H1 entre 50–200 ms y 0,5–1,5 s es
15,71 dB mayor que en la grabación. Quitar únicamente el drenaje del puente
cambia ese descenso 16,06 dB: lo localiza principalmente en esa carga dentro
del modelo. No equivale a medir la movilidad real del Steinway.

Desactivar todo el drenaje empeora 14 de los 18 casos del resumen espectral.
C5 y F#6 necesitan pérdidas distintas; alargarlas todas sería incorrecto. La
variante de fieltro mantiene prácticamente el mismo problema de C4, visible en
`validacion-y-perdidas.png`.

### 3. Radiación de la tabla

Duplicar y cuadruplicar la cuadratura espacial cambia muy poco los resultados;
la resolución espacial no explica el desequilibrio en esta condición de micro.

El observador anterior ajusta dos coeficientes a la frecuencia propia de cada
modo. Una tabla forzada responde además a otras frecuencias. Comparando sus
coeficientes digitales con una integral de Rayleigh evaluada a cada frecuencia
de excitación, la transferencia de la fundamental cambia aproximadamente
+11,88 dB en C4, +7,98 dB en C5 y −12,73 dB en C6. Son diferencias de la misma
tabla sintética, no diferencias identificadas del piano real.

Se implementó un observador temporal con aceleración y retardos por superficie.
Los retardos relativos son causales; se retira por igual el tiempo común
altura/velocidad del sonido. Usa diferencia temporal de velocidad e
interpolación lineal del retardo, por lo que aún tiene error de discretización.
La presión no devuelve carga acústica a la tabla. El prototipo tiene soporte
máximo de 256 muestras y sólo se evaluó aquí a 44,1 kHz/2,64 m; exige revisar
soporte y rendimiento antes de admitir otras geometrías o tasas en producción.

La variante mejora partes de C4/C5, pero C6 queda unos 14,4 dB por debajo del
original en 50–200 ms, frente a −0,47 dB del 14B. **Se descartó como candidato
completo.** Sus coeficientes de tabla tendrían que identificarse de nuevo.

### 4. Fieltro: ley y rigidez

Se probaron por separado la eliminación de histéresis y la reducción de rigidez
a la mitad, además de su combinación. Sus efectos dependen del registro y no
son equivalentes: por ejemplo, bajar sólo la rigidez mejora bandas altas de
C4/C5 pero deja un exceso grande en C7; quitar sólo la histéresis reduce parte
de ese exceso en C7. No se atribuye todo el problema a un único coeficiente.

La prueba nueva usa, para compresión positiva:

`F = K · compresión^p · exp(clamp(α · velocidad_de_compresión, −4, 4))`

La fuerza permanece positiva y tiende suavemente a cero al separarse. Con la
energía elástica `K · compresión^(p+1)/(p+1)`, el término
`(F − F_elástica) · velocidad_de_compresión` es no negativo: disipa energía.
El exponente, masas y escala de las cuerdas conservan su variación por registro.
La variante seleccionada para escuchar emplea α=0,2 s/m y K_A0=19.600.000,
frente a 39.200.000 del 14B, con contacto integrado en el registro agudo.

Es una **ley constitutiva alternativa y efectiva**, no una medición del fieltro
del Steinway. No contiene la relajación con memoria de Stulov. Conserva además
la normalización modal por golpe y la curva MIDI del motor; la energía mecánica
todavía no se convierte en presión absoluta sin esa normalización. No se
presenta como una solución física completa ni como un refit final.

## Validación de la variante de contacto

| Registro | Casos | Error inicial 14B | Error variante | Casos que mejoran >0,25 dB | Casos que empeoran >0,25 dB |
|---|---:|---:|---:|---:|---:|
| Graves, debajo de C3 | 36 | 10,23 | 10,55 | 4 | 19 |
| Medios, C3 a debajo de F#5 | 40 | 8,69 | 8,46 | 23 | 9 |
| Agudos, F#5 en adelante | 44 | 17,78 | 16,03 | 39 | 2 |
| Total | 120 | 12,49 | 11,86 | 66 | 30 |

Los 24 restantes cambian menos de 0,25 dB. En los 102 casos adicionales, el
error medio baja de 12,45 a 11,87 dB, con 53 mejoras y 28 regresiones por encima
de ese umbral. La mejora media no permite ocultar las regresiones en graves.
Las 12 sondas C4/F4/G4/C5 reducen el error del espectro de 50–200 ms contra el
sampler; esto no garantiza que su percepción completa haya mejorado.

111 pruebas nativas aprobaron, incluyendo balance de energía de un choque y
fase/derivada del observador temporal. El choque de prueba conserva energía
cinética + elástica + disipada con residual inferior a 0,5%. En las 18 sondas
del contacto nuevo, la mayor energía final de martillo+cuerda es 94,12% de la
entrada. Es un control de esos casos; no sustituye una prueba general del
integrador multimodal. Cuatro controles con las opciones nuevas desactivadas
son idénticos muestra por muestra al 14B. No se comprobó el rendimiento ASIO
en vivo de la variante ni se la incorporó a WASM.

## Audios

- `Fieltro-experimental-medios.wav`: **C4, F4, G4 y C5**, cada una a **60, 84
  y 124**. Tres segundos sostenidos y 0,4 s de silencio; duración 40,8 s.
- `14B-anterior-medios.wav` y `Steinway-sampler-medios.wav`: mismo orden,
  misma ganancia común por condición y mismo trim de seguridad para los tres.
- `Fieltro-experimental.wav`, `14B-anterior.wav`, `Steinway-original.wav`:
  C4/C5/C6/F#6/C7 a 60/124, cuatro segundos sostenidos por nota. El original
  utiliza las grabaciones directamente, sin transposición.
- `Radiacion-prueba-descartada.wav`: diagnóstico de la radiación temporal;
  no es la variante de contacto elegida para escuchar.

Todos son estéreo PCM24 a 44,1 kHz, finitos, con pico máximo 0,85. Hay un fade
final de 15 ms para recortar cada fragmento. No hay note-off en los fragmentos
sostenidos: estos WAV cortos no sirven para evaluar la liberación de la tecla.

## Trabajo que sigue

La ley de contacto aporta una dirección que mejora agudos, pero falta un ajuste
integral por registro que conserve graves y dinámica, y un ajuste conjunto de
movilidad/carga y observación de la tabla. Los residuos modales deben cambiar
tanto la pérdida mecánica como la presión que generan; ajustar un EQ externo
no corregiría la desaparición de H1 de C4. La radiación temporal constituye un
modelo de referencia para ese ajuste, aunque la tabla actual requiere refit y
el costo del observador exige optimización antes de producción.

Antes de seleccionar otro plugin completo corresponde ampliar a las 16 capas,
controles de pedal/liberación y ambos MIDI, conservando estos casos que
empeoran. El material actual queda como experimento reproducible para escucha
y ajuste; no como reemplazo automático del modelo instalado.

## Fuentes y créditos

La separación entre base modal, cuerdas, dinámica de tabla y radiación está
descrita en [Physically-based sound synthesis software for Computer-Aided-Design
of piano soundboards](https://acta-acustica.edpsciences.org/articles/aacus/full_html/2022/01/aacus210096/aacus210096.html),
Acta Acustica, 2022. Su integral de Rayleigh sirve como referencia para el
observador, bajo la aproximación de una tabla en un bafle infinito.

La histéresis y la dependencia de la velocidad del fieltro se describen en la
[investigación y dispositivo de medida de Stulov](https://www.ioc.ee/res/piano.html).
Eliminar esa memoria en una prueba no demuestra que esté ausente en un martillo
real. Las limitaciones del acoplamiento y propagación requieren también el
marco de [Chabassier et al., simulación temporal del piano](https://archive.numdam.org/articles/10.1051/m2an/2013136/).

Grabaciones: [The bitKlavier Grand](https://bitklavier.com/the-bitklavier-grand/),
Steinway D, Taplin Auditorium, Princeton, enero de 2021; licencia
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Grabación: Andrés
Villalta; piano: Matt Wang; edición/reducción de ruido: Jeff Gordon, Katie Chou,
Christien Ayers, Matt Wang y Dan Trueman; producción: Dan Trueman; apoyo del
Departamento de Música y Center for Digital Humanities de Princeton.
Adaptaciones: selección de capas, reproducción SFZ, transposición de F4/G4,
ganancia común, recorte/fade y concatenación. Conservar créditos/licencia al
compartir esos audios. No implica respaldo de los autores a RackForge.
