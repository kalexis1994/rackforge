# MIDI con muestras originales del bitKlavier Grand

Nocturno Op. 9 n.º 2 y La Campanella, reproducidos con las grabaciones del
Steinway D, perspectiva **Stage ORTF**. Archivos de escucha:
`C:/SHARED/SteinwayD-MIDI/`.

Estos WAV son interpretaciones MIDI realizadas por un sampler. No son nuevas
grabaciones acústicas de estas obras ni renders del modelo físico RF-Concert Grand.

## Reproducción

- Motor nativo sfizz 1.2.3, render offline, remuestreo de calidad 10 y 512 voces.
- Muestra principal más cercana: como máximo un semitono de transposición.
  Las notas grabadas conservan su pitch original. Se conservan ambos canales.
- 16 capas grabadas de intensidad, seleccionadas en intervalos uniformes de
  velocidad MIDI. Ganancia dentro de cada capa basada en el RMS de sus primeros
  400 ms, siguiendo el enfoque publicado del cargador/reproductor de bitKlavier.
  No se superponen ataques de distintas capas para interpolar el timbre.
- Eventos y tempo del MIDI conservados; comienzo en el tiempo cero del archivo.
  Se añaden 10 segundos de cola. Los MIDI originales no se modifican.
- CC64 controla el sustain mediante el umbral del sampler (64). CC66 controla
  sostenuto. CC67 reduce continuamente el volumen hasta 3 dB.
- Muestras originales de resonancia al liberar y retorno de tecla, a niveles
  documentados en `mapping.json`. Los agudos desde F#6 conservan el decaimiento
  grabado sin apagador. F6 permanece amortiguado.
- Ruido original de pedal D1/U1 únicamente al alternar extremos de recorrido
  (CC64 >=115 y <=10), evitando repetirlo con fluctuaciones intermedias.
- Sin reverberación añadida, ecualizador, compresor ni bucles artificiales.
- Exportación estéreo de 44,1 kHz, PCM de 24 bits. Las fuentes son la edición
  oficial de 44,1 kHz/16 bits; exportar a 24 bits preserva precisión de la mezcla.
- Una sola ganancia común para ambas piezas limita el pico a 0,85. No se
  normalizan notas individuales ni se comprime la dinámica de las piezas.

## Límites de esta referencia

Es un mapeo SFZ independiente, no una exportación desde la aplicación bitKlavier.
No reproduce físicamente el medio pedal, la una corda, el acoplamiento entre
cuerdas ni la recaptura de apagadores de un piano real. La resonancia de liberación
y las envolventes son aproximaciones del sampler. El nivel del ruido de pedal
no constituye una calibración acústica del micrófono.

La sala y la captación presentes en los WAV originales permanecen en el audio.
Las grabaciones de la biblioteca fueron editadas, niveladas y tratadas con
reducción de ruido por sus autores. La transposición cambia también su envolvente
temporal; se mantiene pequeña al usar la raíz más cercana.

## Créditos y licencia de las grabaciones

Biblioteca: [The bitKlavier Grand](https://bitklavier.com/the-bitklavier-grand/).
Steinway D de Princeton University, Taplin Auditorium, Fine Hall; enero de 2021,
tapa completamente abierta. Grabación: Andrés Villalta; piano: Matt Wang;
edición y reducción de ruido: Jeff Gordon, Katie Chou, Christien Ayers, Matt Wang
y Dan Trueman; producción: Dan Trueman. Con apoyo del Departamento de Música
y del Center for Digital Humanities de Princeton University.

Las grabaciones están bajo [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).
Esta adaptación realiza mapeo de notas, transposición, selección por velocidad,
envolventes, mezcla y ajuste global de nivel para reproducir los MIDI indicados.
Conservar este crédito y la licencia al compartir los audios derivados. No implica
respaldo de los autores a RackForge. La licencia de las grabaciones no acredita
derechos adicionales sobre los archivos MIDI utilizados.

## Reproducibilidad

Herramienta: `tools/render-bitklavier-steinway-midi.py`.
`mapping.json` contiene parámetros y SHA-256 de las fuentes utilizadas, motor,
SFZ y herramienta. Cada pieza tiene un JSON con hash del MIDI/WAV, eventos,
ganancia, pico y máximo de voces. `verification.json` registra los controles finales.
Se guardan copias de estos registros en esta carpeta y en la carpeta de escucha.

El SFZ contiene rutas absolutas hacia la biblioteca local; para repetir el render
en otra máquina hay que disponer de esas muestras y regenerar el mapeo.
No es necesario instalar sfizz como plugin: se usa su biblioteca nativa descargada
de la [publicación oficial 1.2.3](https://github.com/sfztools/sfizz/releases/tag/1.2.3).
En `provenance.json` se registran también los hashes del código fuente oficial
de bitKlavier consultado para el mapeo de velocidades y liberaciones.
