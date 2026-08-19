# Concert Grand — cola de investigación e implementación

Papers pendientes de incorporar al modelo, en orden de implementación. Cada
item se marca al aterrizar en `plugins/concert-grand`, con la sección de
`PIANO_MODEL.md` que lo documenta.

## 1. Histéresis del fieltro (Stulov) — [x] (v0.19.0)

**Fuente:** A. Stulov, "Hysteretic model of the grand piano hammer felt",
*JASA* 97 (1995); PDFs en https://homes.ioc.ee/stulov/klaver2.pdf y
https://www.ioc.ee/~stulov/smac03.pdf.

**Qué dice:** todo martillo medido tiene característica fuerza-compresión
histerética: el fieltro es un material con memoria,
`F(t) = F0·[ξ^p − (ε/τ0)·∫ ξ^p(t')·e^-((t-t')/τ0) dt']`,
con dos parámetros hereditarios (ε, τ0) además de (F0, p). La carga es más
rígida que la descarga; el contacto disipa energía.

**Qué cambia en el modelo:** `simulate_strike()` usa hoy un resorte sin
pérdidas `F = K·ξ^2.5` — demasiado elástico, pulso simétrico irreal. La
histéresis acorta y asimetriza el pulso y oscurece la descarga. Es un filtro
exponencial corriente sobre ξ^p dentro del integrador: costo despreciable.

## 2. Validación de fases del bajo (Galembo–Askenfelt–Cuddy) — [x] (v0.19.1) — resultado: fases ordenadas medidas e impuestas, efecto audible casi nulo según el usuario; las fases no eran el cuello de botella

**Fuentes:**
- "Effects of relative phases on pitch and timbre in the piano bass range"
  (las fases relativas entre parciales son audibles en el registro grave).
- "Perceptual significance of inharmonicity and spectral envelope in the
  piano bass range" (el ancho de banda espectral pesa más que la
  inharmonicidad en el timbre grave).

**Qué cambia:** el sim del martillo (v0.18.0) ya produce fases correlacionadas;
estos papers dan el protocolo para validarlas contra las muestras YDP
(medir fases relativas de los primeros ~10 parciales del bajo en el ataque,
comparar real vs modelo con `tools/analyze-piano-sf2.py` extendido). También:
priorizar ancho de banda del bajo sobre ajustes finos de B.

## 3. Parámetros medidos de un grand real (Chabassier–Chaigne–Joly) — [~] parcial (v0.20.0): exponente del fieltro por registro (1.7→3.4, su rango medido 1.5–3.5) desde la Part 1 (M2AN 2014, PDF guardado); faltan las tablas numéricas por nota (masa, kH) del paper JASA/Part 2

**Fuente:** J. Chabassier, A. Chaigne, P. Joly, "Modeling and simulation of
a grand piano", *JASA* 134 (2013) — el modelo numérico completo de un
Steinway D, con tablas de parámetros por nota: masas de martillo, rigidez y
exponente del fieltro, datos de cuerda (tensión, masa, longitud, diámetros).

**Qué cambia:** `simulate_strike()` usa escalados inventados
(masa 0.06+0.85·pos^1.3, rigidez desde el tiempo de contacto objetivo).
Reemplazarlos por los valores medidos del paper, nota por nota
(interpolados), y derivar los tiempos de contacto en vez de imponerlos.

## 4. Pérdidas de cuerda de dos parámetros (Bensa et al.) — [ ]

**Fuente:** J. Bensa, S. Bilbao, R. Kronland-Martinet, J. O. Smith, "The
simulation of piano string vibration", *JASA* 114 (2003).

**Qué cambia:** nuestra curva T60(f, string_scale) es un fit global; el
modelo b1/b3 (fricción de aire + pérdida viscoelástica) da forma medida por
cuerda con dos parámetros publicados.

## 6. Amortiguación por radiación acústica — [x] (v0.38.0)

**Fuentes:** K. Ege, X. Boutillon, "Vibrational and acoustical
characteristics of the piano soundboard" (arXiv:1212.3068) y "Global and
local synthetic descriptions of the piano soundboard" (arXiv:1210.5109);
B. Trévisan et al., "Linear string-soundboard coupling in pianos", ICA 2010;
J. Bensa et al., *JASA* 114 (2003) para las pérdidas propias de la cuerda.

**El síntoma:** el usuario oye los graves "planos, casi como cuerdas de
guitarra", con los medios-agudos sin filtrar.

**La medición (0.08-0.25 s contra 1.6-2.2 s, A0 del YDP):**

| banda | modelo | real |
|---|---|---|
| 60-120 Hz | +11.2 dB | +3.7 dB |
| 1-2 kHz | **+5.7** | -7.5 |
| 2-4 kHz | **+3.7** | -22.4 |
| 4-8 kHz | -7.7 | -42.3 |

Los medios-agudos del modelo *crecían* durante dos segundos donde los del
piano real se derrumban 22-42 dB. Una nota grave real se OSCURECE mientras
suena; la nuestra se aclaraba.

**Lo que quedó descartado antes de llegar al mecanismo, y por qué importa:**

* *Pérdidas internas de la cuerda* (b1/b3 de Bensa, item 4). σ = b1 + b2·κ²,
  pero para una cuerda rígida κ² crece como ω, no ω², así que esa ley da
  T60 ∝ 1/f — más suave que el 1/f^1.25 que ya teníamos. No es el hueco.
* *Admitancia del puente.* Medida en un vertical, es ~4·10⁻⁴ s/kg y
  prácticamente plana: "decrece levemente entre 500 y 2000 Hz, seguida de un
  leve aumento entre 2 y 4 kHz". No filtra. Además, el amortiguamiento por
  el puente da la MISMA tasa para todas las parciales, porque cada una hace
  el mismo número de viajes de ida y vuelta por segundo.
* *El ambiente.* Verificado por medición, no por argumento: con `air` y
  `sympathy` en cero la banda de 2-4 kHz de A0 cambia 0.2 dB. Eran las
  cuerdas.

**El mecanismo:** eficiencia de radiación de la tabla armónica. Una placa
radia bien solo cuando su longitud de onda de flexión supera la del aire;
por debajo de esa coincidencia las regiones vecinas se mueven en antifase y
sus campos cercanos se cancelan — la tabla empuja aire de costado en vez de
comprimirlo, y la parcial se queda con su energía. Ege y Boutillon ubican la
transición cerca de 1.1 kHz. Medido: eficiencia muy baja bajo 80 Hz,
razonable entre 100 Hz y 1 kHz, muy alta sobre 1.4 kHz.

Es UN mecanismo, no dos, y ahí está la gracia: la misma ineficiencia que
hace callado al fundamental del bajo es la que lo hace durar, y la misma
eficiencia que hace fuertes a las parciales altas es la que las mata.

**Implementado en v0.38.0** como un canal de pérdida en paralelo con las de
la cuerda (las tasas se suman), con la rodilla ajustada a 2.5 kHz sobre las
tasas medidas del A0. Al hacerlo se quitó el rolloff empírico de agudos que
había en `t60_seconds`: hacía a mano el mismo trabajo y contarlo dos veces
dejaba 4-8 kHz 18-34 dB por debajo del real. `treble_life` sobrevive como
control pero ahora gobierna cuánto entrega la tabla, que es de donde el
efecto sale de verdad.

**Resultado medido:** el error de caída del A0 en 4-8 kHz pasa de +34.5 dB a
+3.6, y en 2-4 kHz de +26.1 a +17.8. Sobre las notas graves el error
absoluto medio baja de 7.13 a 6.45 dB.

**Lo que quedó peor, y hay que atender:** F#1 y C2 en 4-8 kHz pasan a -18 y
-30 dB. Esas notas ya tenían déficit ahí antes (-10.7 en C2) y la radiación
lo profundizó. El déficit de tono en agudos ya está anotado en
PIANO_MODEL.md; esto lo vuelve más urgente, no lo causa.

## 5. Tabla armónica medida en medios-agudos (Ege, Boutillon) — [ ]

**Fuente:** K. Ege, X. Boutillon, "Synthetic description of the piano
soundboard mechanical mobility" / tesis de Ege (2011).

**Qué cambia:** el board_response sintético (3 senos en log-f) y los 18 modos
del cuerpo podrían seguir la densidad modal y movilidad medidas, incluida la
transición a comportamiento de placa nervada (~1.1 kHz).

## 7. Analisis de brecha contra Pianoteq — [ ]

**Metodo, y sus limites.** Nada de esto sale de decompilar su motor: eso lo
prohibe su licencia y ademas contaminaria nuestra implementacion. Lo que hay
aca son dos fuentes publicas: el manifiesto NKS que el producto instala para
navegacion por hardware (`Pianoteq 9/nks/*.nksf`, MessagePack dentro de RIFF)
y el manual de usuario publicado. Ambos dicen QUE mecanismos exponen, no COMO
los implementan. La lista sirve como checklist de fisica, no como receta.

### Medir su SALIDA, que es lo que se puede y ademas sirve mas

`tools/compare-reference-render.py` rinde notas sueltas por el exportador de
linea de comandos que su propio manual documenta y mide el audio que sale, con
las mismas herramientas con que medimos el YDP y nuestros renders. No lee ni
desensambla nada. Eso da la referencia externa que faltaba: un modelo que
convence, medido igual que nosotros.

**El resultado, y no fue el esperado.** La densidad no nos separa: en 2-4 kHz
del A0 la referencia da 27 picos, nosotros 36 y el instrumento 82. No estan
persiguiendo el conteo. Lo que si nos separa, en las tres notas y solo a
nosotros, es el RELIEVE de los picos sobre el piso: ellos y el piano real
entre 27 y 35 dB, nosotros en 23-24. Ver PIANO_MODEL.md.

### La inferencia principal: su tabla armonica es una PERDIDA, no un filtro

Pianoteq expone la tabla con tres controles -- **impedancia mecanica**
("cuanto mayor la impedancia, mas largo se vuelve el sonido"), **frecuencia de
corte de la impedancia** y **pendiente de la impedancia** ("mayor factor da
una caida mas rapida de los armonicos altos"). Los tres describen DURACION,
ninguno describe color.

La nuestra hace lo contrario: `board_response` es una curva de aspereza
sintetica que multiplica AMPLITUDES, y la misma curva la muestrea cada nota.
Eso es exactamente el formante fijo medido (7.9 dB rms contra 4.7 del real,
ver PIANO_MODEL.md), y es la explicacion de por que el usuario oye "nasal" en
los graves y no en los agudos.

Si la tabla se expresa como una impedancia que fija cuanta energia entrega
cada parcial -- es decir, como amortiguacion dependiente de la frecuencia --
deja de teñir y pasa a gobernar la duracion, que es lo que hace una tabla de
verdad. Esto tambien absorbe la amortiguacion por radiacion del item 6 en un
solo mecanismo en vez de dos.

### Mecanismos que exponen y nosotros no modelamos en absoluto

Ordenados por relevancia a lo que el usuario oye hoy:

1. **Radiacion como modelo 3D con microfonos y posicion de tapa.** Ellos
   colocan microfonos alrededor del instrumento, con tipo, direccion,
   compensacion de proximidad y velocidad del sonido. Nosotros tenemos taps de
   retardo FIJOS para la tapa, que son un peine fijo en frecuencia -- otra
   fuente de color nasal, y la sospechosa que queda tras arreglar la tabla.
2. **Duplex scale.** Los tramos de cuerda detras del puente y delante del
   agrafe, sin apagador, siempre sonando. No tenemos nada.
3. **Last damper.** Las notas por encima de cierta tecla NO tienen apagador y
   suenan siempre, sin pedal. Nosotros amortiguamos todo.
4. **Blooming inertia.** Tenemos transferencia de energia a los armonicos
   altos pero no la VELOCIDAD a la que ocurre. Relevante: nuestros 2-4 kHz del
   bajo suben durante dos segundos donde el real cae.
5. **Ruidos de accion separados en tres** -- liberacion de tecla, apagador, y
   el "whoosh" del pedal cuando todos los apagadores suben Y cuando bajan --
   mas *Stickiness*. Nosotros tenemos un solo golpe.
6. **Damper position, damping duration y mute.** El apagador como mecanismo
   con posicion y eficiencia, no como un corte.
7. **Strike point humanization** y **Condition** ("de recien afinado a
   completamente gastado", con semilla aleatoria). Irregularidad nota a nota
   como parametro de primera clase.
8. **Unison balance** (la cuerda del medio del trio) y **spectrum profile**
   (intensidad individual de los primeros ocho armonicos, editable por nota).
9. **Mallet bounce**, y pedales que cambian la terminacion o interponen
   material: buff stop, celeste, rattle, pinch harmonic.

### Lo que ya tenemos y coincide

Inarmonicidad y estiramiento derivados, ancho de unison, dureza del fieltro
por registro, punto de golpe, resonancia simpatica, blooming, caida del
apagador al soltar, y una sala. La brecha no esta en la lista de ingredientes
basicos: esta en como se expresa la tabla armonica y en todo lo que rodea a la
cuerda.

## 8. Modulacion de tension (Kirchhoff-Carrier) — [x] (v0.46.0)

**El planteo que se descarto primero.** La propuesta inicial fue un hibrido:
diferencias finitas para la octava o dos mas graves, modal de ahi para arriba.
El usuario lo rechazo como parche y tenia razon. Ningun piano cambia de fisica
en la nota 45, y las notas a cada lado del corte sonarian distintas por una
razon que no existe en el instrumento.

**Y el diagnostico de fondo estaba mal.** La sintesis modal no es una
aproximacion de la cuerda: para una cuerda rigida LINEAL es la solucion
exacta. No estamos mas lejos de la realidad por usar modos. La linea real no
es grave contra agudo, es **lineal contra no lineal**: los modos dejan de ser
independientes cuando la amplitud es grande, y eso pasa en el bajo y en los
golpes fuertes, que es donde el modelo falla.

**El mecanismo.** Ley de Kirchhoff: estirar la cuerda sube su tension, y la
tension fija la frecuencia de todos los modos, `T = T0 + (EA/2L)*int (dy/dx)^2`.
En coordenadas modales esa integral es `sum (n*pi/L)^2 q_n^2`, sin terminos
cruzados. Y hay una coincidencia afortunada en como este modelo guarda las
cosas: sus amplitudes ya son FUERZA EN EL PUENTE, que lleva el factor n, asi
que `(n q_n)^2` es el cuadrado del propio componente. Toda la no linealidad es
una suma escalar.

De ahi salen dos cosas que el modelo escribia a mano:

* la parte lenta es el **glide**, que era una rampa dibujada de 28 pasos con
  un tamaño puesto a dedo. Ahora se produce.
* la parte que oscila al doble de cada modo modula a todos los demas y pone
  bandas laterales en `f_i +/- 2f_j`: las **parciales fantasma** de Conklin,
  que el modelo coloca de antemano en frecuencias que calcula. Aca se generan
  solo mientras la cuerda de verdad se mueve tanto.

**Un error encontrado midiendo.** La primera version acumulaba: el empuje
edita la matriz del oscilador y es permanente, asi que pedir el desplazamiento
completo en cada paso lo sumaba una y otra vez, y la nota subia 46 cents por
segundo mientras siguiera sonando. Se corrigio aplicando solo la DIFERENCIA
contra lo ya aplicado. Sin instrumentar la escala del estiramiento el error
era invisible: el score empeoraba y nada decia por que.

**Coste**: se corre cada 32 muestras, no por muestra. La parte oscilante que
importa esta al doble de los modos que llevan la energia -- decenas a pocos
cientos de Hz en el bajo -- y 32 muestras alcanzan 690 Hz. El fuel va de 39%
a 42% en el peor caso.

**Resultado medido**, en relieve de los picos sobre el piso en 2-4 kHz, contra
las dos referencias:

| nota | YDP | referencia | antes | ahora |
|---|---|---|---|---|
| A0 | 30.3 | 26.8 | 26.2 | **27.2** |
| F#1 | 29.6 | 33.4 | 24.6 | **27.0** |
| C2 | 34.0 | 34.6 | 25.9 | 26.3 |

El A0 paso a la referencia. F#1 gano 2.4 dB. El score del fitter queda igual
(19.61 -> 19.63), que para un cambio que reemplaza un guion por una ley es el
resultado que se queria: no compra nada prestado.

**Lo que sigue pendiente**: las fantasmas siguen colocandose a mano ademas de
generarse. Si la ley las produce a nivel suficiente, el mecanismo escrito
sobra y hay que quitarlo; medir antes de sacarlo.

## 9. La base: el pulso esta DIBUJADO, no generado — [~] parcial (v0.50.0)

**El planteo es del usuario**, tras una sesion entera en que cada arreglo
mejoraba un numero y no cambiaba lo que oia: "el problema esta en las
matematicas de base que estamos usando para generar el pulso; si nada lo
mejora es porque la base no es la ideal". Tenia razon, y se puede contar.

**Que es hoy nuestro pulso.** La amplitud de cada parcial sale de un producto
de factores analiticos -- peine de punto de golpe x ventana de contacto x
fieltro x color de tabla. Eso es una CURVA, no una fuerza. Existe una
simulacion de verdad, `simulate_strike`, que integra el fieltro de Stulov
contra los modos de la cuerda y produce amplitudes Y FASES modales reales.
Pero su resultado se renormaliza al pico de la receta y despues se mezcla con
`if candidate > amplitudes[n]`: sobrevive solo donde le gana a la curva.

**La medicion que lo cierra.** `how_much_of_the_note_the_strike_actually_sets`
cuenta cuantos parciales puede alcanzar la simulacion:

| nota | parciales | alcanzables antes |
|---|---|---|
| A0 | 144 | 48 = **33%** |
| C2 | 102 | 48 = **47%** |
| C3 | 69 | 48 = 69% |
| C4 | 45 | 45 = **100%** |
| C5 | 31 | 31 = **100%** |

`SIM_MODES` estaba en 48. En el bajo, DOS TERCIOS de la nota eran la curva
dibujada; en el agudo, el golpe la generaba entera. Y esa linea es exactamente
la linea que el usuario viene describiendo hace cuarenta versiones: los
agudos suenan a piano, los graves a electrico. No es calibracion, es
arquitectura.

**Hecho en v0.50.0:** `SIM_MODES` 48 -> 144, asi que el golpe alcanza el 100%
de toda nota. El fuel no se movio (42% en el peor caso): solo cuesta en el
note-on y esta acotado por el presupuesto de golpes por bloque.

**Y no alcanzo, lo cual confirma la otra mitad.** El score quedo igual (22.64
-> 22.70). La simulacion ahora llega a todos los modos y su resultado se
sigue tirando: renormalizado al pico de la receta y descartado donde no le
gana. Falta lo de fondo.

**Lo que queda, y es la reescritura de la base:**

1. Quitar la renormalizacion `peak / sim_peak`, para que el golpe lleve su
   propio nivel. Hoy la masa del martillo se cancela justo ahi.
2. Quitar el maximo y dejar que la simulacion FIJE el espectro, con el color
   de tabla y radiacion aplicado encima (eso si es legitimo: es el camino de
   radiacion, no la excitacion).
3. Degradar la receta a piso, o borrarla.
4. Refit completo -- mueve todos los anchors. Validar en niveles absolutos,
   factor de cresta, y las medidas que el costo no contiene (densidad,
   relieve, razon de decaimiento grave/agudo).

Un intento parcial de (2) sin (1) ni (4) se probo y se revirtio: crossfade a
1.0 / 0.6 / 0.3 dio 20.81 / 20.06 / 20.07 contra 19.95, pero la prueba era
injusta porque la tabla de calibracion habia sido ajustada con el maximo
puesto.

## 10. Vibracion longitudinal de la cuerda — [~] cableada, sin calibrar

**Fuente:** B. Bank, L. Sujbert, "Generation of longitudinal vibrations in
piano strings: From physics to sound synthesis", *JASA* 117 (2005).

**Por que importa.** El usuario insistio en que faltaba algo estructural en el
bajo, no valores. La cuerda tiene una segunda onda -- la de compresion, que
viaja unas treinta veces mas rapido y no depende de la tension -- y el paper
es explicito sobre su peso: la vibracion longitudinal "contribuye enormemente
al caracter distintivo de las notas graves" y es "responsable del caracter
metalico de las notas bajas".

**Lo que teniamos:** dos componentes en 17*f0, colocadas una sola vez en el
note-on, con nivel fijo y un cuarto de segundo de cola. El paper contradice
las tres cosas:

* "el movimiento longitudinal se excita CONTINUAMENTE por la vibracion
  transversal a lo largo de la cuerda, y no solo durante el contacto del
  martillo". El nuestro disparaba una vez y decaia.
* su amplitud es "una funcion no lineal de la amplitud de la transversal...
  mas rapida que una cuadratica simple". El nuestro tenia nivel fijo.
* no es un formante sino "un espectro cuasi-armonico con picos tipo formante
  en las frecuencias modales longitudinales".

**La receta de sintesis del paper**, que es implementable: un banco de
resonadores de segundo orden en esas frecuencias modales, excitado por la
tension que produce el movimiento transversal, sumando un modo par y uno
impar. Y notan que "la eficiencia se puede aumentar si no se computan las
componentes de la excitacion donde la ganancia del banco es pequeña".

**Estado: cableado, sin calibrar.** Cuatro resonadores por voz en
k*c_L/(2L), con la velocidad longitudinal tomada de su medicion (~1.15 kHz
para C2, la mitad de lo que daria acero desnudo, porque el entorchado de
cobre agrega masa sin rigidez). La excitacion se calcula por muestra desde
las parciales transversales que llevan la energia.

Lo que falta es el nivel, y esta mal por ordenes de magnitud: el audio cambia
pero la energia cerca de 1150 Hz no se mueve 0.1 dB entre mezcla 0 y 60. El
coeficiente de entrada del resonador es (1-r)*2*sin(w) ~ 5.7e-5, y el
contenido de la excitacion A esa frecuencia es una fraccion pequeña de una
señal que ya es chica tras el escalado de headroom. Encontrar donde se pierde
es lo que sigue; subir la constante a ciegas no.

**Error a no repetir:** la primera version alimentaba el banco con el
estiramiento calculado a tasa de control, cada 32 muestras. Eso es una
envolvente: un resonador a 1150 Hz necesita excitacion A 1150 Hz. Los modos
longitudinales se excitan a las frecuencias suma y diferencia de las
transversales, que son de audio.

## Referencia permanente

- Muestras de calibración: YDP Grand en
  `rackforge-plugin-rf-dls/artifacts/rf-soundfonts-0.2.0.rfplugin`
  (assets/ydp-grand-piano.sf2); script `tools/analyze-piano-sf2.py`.
- Reglas aprendidas: sonido seco primero (staging a cero hasta nuevo aviso);
  nada de redes difusas (leen como reverb); el sustain agudo debe venir de
  resonancias afinadas discretas; el HF sostenido del bajo es del la cuerda
  (amortiguamiento por cuerda, no por frecuencia).
