# Seguimiento de física pendiente — Concert Grand

Actualizado: 2026-10-04. El usuario autoriza investigar y probar los mecanismos
pendientes en orden, documentando resultados. Las muestras disponibles permiten
comparar el resultado audible; no identifican de forma única cada mecanismo real.

## Base y reglas de comparación

- Fábrica y paquete actuales: 0.172.0. El paquete en `C:\SHARED` conserva
  SHA256 `ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
- El control A sin drenaje conserva un sostenido que gustó al usuario. Es una
  ablación resistiva, **no una nueva calibración física aceptada o instalada**.
- B/piloto I fue rechazada por efecto parecido a flanger. No usarla como base.
- Una hipótesis por etapa, con control congelado, mismos eventos y ganancia.
  No aplicar otra vez los 25,120884 dB ya incorporados en el renderer actual.
- Mantener un control con carga física. Los controles sin drenaje sólo sirven
  para atribuir efectos. No normalizar individualmente las notas para ocultar
  regresiones ni aceptar automáticamente una mejora del score.
- Prototipos nativos fuera de la calibración de fábrica. Antes de trasladar
  algo al plugin: estabilidad, uso de pedales/reiteraciones, coste WASM y escucha.
- Conservar resultados rechazados y sus causas. No repetir búsquedas sin una
  hipótesis distinta. El avance acumulado quedó en el commit 346b6c8 autorizado
  el 2026-10-03; no se selecciona ni empaqueta automáticamente un piloto.

## Orden y estado

| ID | Mecanismo | Representación actual / falta | Estado |
| --- | --- | --- | --- |
| P1a | Admitancia direccional del puente | La horizontal usa la misma familia de modos con un factor fijo; falta una segunda proyección mecánica independiente. | Piloto medido: pasivo, sin mejora consistente; escucha pendiente |
| P1b | Dos polarizaciones por cuerda | Fábrica: tres verticales y una horizontal agregada. Prototipo: una horizontal independiente por cuerda, con proyección equivalente. Falta identificar propiedades direccionales reales. | Piloto medido: equivalencia verificada, sin mejora consistente |
| P2 | Tabla ortótropa con costillas y puente | Piloto: placa ortótropa, nueve costillas discretas, puente recto/curvo; modos físicos bajo 800 Hz con complemento alto heredado. Geometría y acústica aún sin identificar. | Resolución y curva medidas: mejora desigual, sin selección |
| P3 | Intercambio no lineal transversal/longitudinal | Núcleo SI con contacto, resolución Euler, golpes sobre cuerda en movimiento y soporte vertical recíproco verificados. Observación SI probada en una ventana de C4. | Piloto medido; fuente audible completa e integración pendientes |
| P4 | Dispersión Timoshenko | Escalera rígida simplificada; faltan cizallamiento e inercia de rotación. | Pendiente |
| P5 | Contacto integrado con el puente | Fábrica: sin devolución de movimiento durante el contacto. Piloto SI: soporte vertical con inercia compartida y contacto recíproco de una cuerda. Faltan unísono y demás direcciones. | Parcial: mecánica vertical medida; integración pendiente |
| P6 | Radiación y carga acústica del aire | Matriz P2 y contacto C4 medidos. Carga racional pasiva reduce estados 61× y elimina el eco tardío, pero falla la fuerza transitoria y la banda alta. Faltan kernel geométrico temporal, completado alto, contactos largos/banco completo, separación de pérdidas y ambas caras/borde. | Referencia medida; reducción compacta aún rechazada; integración audible pendiente |

El orden es una prioridad de investigación, no una afirmación de causalidad.
P1 se relaciona con el sostenido/unísono percibido; P2 puede afectar el cuerpo
y la irregularidad del teclado; P3 la evolución con intensidad; P4 principalmente
la dispersión de parciales altos. Cada efecto debe verificarse al implementarlo.

## P1: diseño del ensayo

### P1a: dirección antes de aumentar estados

Conservar inicialmente la horizontal agregada permite aislar el cambio de
admitancia sin cambiar a la vez afinación, reparto del martillo y pérdidas.
El resultado **no completa P1b** ni representa seis estados físicos independientes.

Se prueba una hipótesis de puente rígido elevado sobre una superficie flexible:
la fuerza lateral en el punto elevado ejerce un momento sobre la superficie.
Para un modo de tabla, su desplazamiento local y su pendiente dan dos
proyecciones distintas. Cada contribución de admitancia se construye como
producto exterior de esas proyecciones y una masa positiva compartida; la
parte disipativa permanece semidefinida positiva. La radiación debe recibir
la misma combinación de fuerza y momento que usa la carga.

La geometría de puente, orientación lateral y formas modales son supuestas.
Una altura de ensayo no es una altura medida del Steinway. Se conserva la
proyección horizontal heredada y se añade una contribución de palanca para
medir sensibilidad. No se presenta como identificación completa del puente.

Controles necesarios:

1. Interruptor apagado: huellas de fábrica preservadas.
2. Palanca nula en el cálculo: equivalencia con la carga escalar heredada.
3. Admitancia contra una exponencial matricial independiente y balance de energía.
4. Misma proyección de momento en carga y excitación de tabla.
5. C4/C5/C6/F#6 a velocidades medias y altas, sin detune adicional; ataque,
   cuerpo y movimiento de color durante el sostenido. Escucha con referencia.
6. Después de un resultado útil: extender registros, velocidades y pedales.

### P1b: estados de cada cuerda

Separar las horizontales conservando energía y fuerza en el límite equivalente.
Los parámetros deben referirse a cada cuerda completa, no a detune independiente
por parcial. Comparar separación de estados sola y admitancia direccional sola
antes de combinarlas. Comprobar qué configuraciones oscuras se excitan y cómo
radian. No asumir que agregar estados mejora el sonido por sí mismo.

## Ensayos previos que condicionan el plan

- [B/piloto I](../concert-grand-unison-flanger-2026-10-02/README.md): separación
  de afinación más pérdidas secundarias menores produce movimiento de balance
  entre parciales; rechazado por oído. El conteo de mínimos tardíos no bastaba.
- [Carga y memoria](../concert-grand-decay-loading-2026-10-02/README.md): quitar
  drenaje recupera sostenido en C4 pero es diagnóstico. La memoria pasiva con
  la misma tabla no recuperó el déficit grande; no repetirla como solución única.
- [Unísono sutil](../concert-grand-subtle-unison-2026-10-02/README.md): el 2×2
  espacial probado era fuerza común y momento por separación de pines; **no**
  dos direcciones de vibración. No confundir ese ensayo con P1a/P1b.
- [Tabla y radiación](../concert-grand-joint-radiation-2026-10-01/README.md):
  ajustes conjuntos de la base modal sintética no generalizaron. Cambiar su
  geometría física en P2 es una hipótesis distinta, no otra búsqueda del mismo banco.
- [Cuerpo identificado](../concert-grand-identified-body-2026-10-02/README.md):
  el control anterior suavizó el ataque y fue rechazado. Conservar el ataque
  como criterio audible, además del sostenido y score por armónicos.

## Fuentes primarias

- [Bensa y colaboradores, ICMC 1999, Resynthesis of Piano Strings Vibrations](https://quod.lib.umich.edu/cgi/p/pod/dod-idx/resynthesis-of-piano-strings-vibrations-based-on-physical.pdf?c=icmc%3Bidno%3Dbbp2372.1999.335%3Bformat%3Dpdf): acoplamiento entre polarizaciones.
- [Bank, admitancia matricial pasiva](https://home.mit.bme.hu/~bank/publist/dafx10adm/index.html): construcción pasiva multidimensional. Su ejemplo es guitarra; aquí se usa el principio mecánico, no se afirma haber reproducido ese instrumento.
- [Ege y Boutillon, descripciones global/local de tabla](https://arxiv.org/abs/1210.5109): transición a comportamiento localizado entre costillas.
- [Chabassier, Chaigne y Joly, piano completo](https://perso.ensta-paris.fr/~touze/PDF/Batwoman/chabassier-jasa.pdf): cuerdas no lineales Timoshenko, tabla ortótropa y acoplamientos.

## Registro de etapas

### 2026-10-02 — Inicio P1a

Auditoría del código y definición del control direccional. No se ha aprobado
un sonido nuevo.

### 2026-10-02 — P1a medido

[Resultados y reproducción](RESULTADOS-P1a.md). Se probaron 10/25/40 mm de
palanca supuesta en 16 casos y cuatro sostenidos largos. Pasividad, equivalencia
de palanca nula, proyección de momento y oráculo matricial independientes pasan.
El control apagado coincide exactamente con los 16 audios de fábrica congelados.

La variante de 10 mm cambia muy poco el error medio y empeora el peor caso.
25/40 mm empeoran el error medio. El ataque cambia poco y no se recupera
consistentemente el cuerpo. Aumentar la palanca introduce más evolución de
balance entre parciales agudos. **Sin mejora demostrada; ninguna seleccionada.**
Esto no descarta la admitancia direccional real: el ensayo conserva tabla
sintética y horizontal agregada, con geometría supuesta. Tampoco verifica P1b.

Audios y copia del plan: `C:\SHARED\Fisica-pendiente-2026-10-02`.
Los comparadores alternan base / palanca 25 mm / Steinway a velocidades
28/60/92/124. La escucha del usuario permanece pendiente. No se instaló ni
empaquetó el prototipo.

### Diseño aplicado — P1b

1. Crear dos estados transversales por cuerda, con masa/energía explícitas.
2. Levantar la horizontal agregada al subespacio común de las horizontales
   individuales. Con afinación, pérdidas y excitación iguales, preservar la
   fuerza y el sonido del control; no duplicar energía al copiar estados.
3. Dar a cada cuerda una sola afinación/rigidez para toda su escalera. Mantener
   primero los valores de fábrica, sin añadir dispersión por parcial.
4. Separar ensayos de excitación diferencial y de carga direccional. Registrar
   lo que puede radiar y lo que permanece oscuro. Si sólo cambia el número de
   estados y el sonido es idéntico, registrarlo como equivalencia, no como mejora.
5. Repetir intensidades y sostenidos C4/C5/C6/F#6; observar energía, ataque,
   cuerpo y movimiento de color. Sólo ampliar o recalibrar tras un resultado útil.

### 2026-10-02 — P1b medido

[Resultados y reproducción](RESULTADOS-P1b.md). Prototipo nativo con una
horizontal por cuerda y ensayos separados de reparto de martillo, afinación
mínima y palanca. Se conservó la energía horizontal inicial del modelo y su
proyección conjugada, no se copió la energía tres veces.

El control equivalente tiene error RMS relativo máximo de −70,756 dB en
16 casos; el control de una cuerda es idéntico, el de dos cuerdas difiere
−72,079 dB y el de reiteraciones/pedal/medio pedal −74,050 dB. La fábrica
apagada conserva 16 audios exactos y sus huellas. Cinco pruebas nuevas pasan.

El reparto de martillo cambia el score medio de 6,83418 a 6,83538; no recupera
consistentemente cuerpo ni ataque. Añadir afinación mínima produce evolución
de color principalmente por la afinación; la palanca conserva la regresión de
P1a. **Sin mejora demostrada; ninguna seleccionada.** Audios y metadatos en
`C:\SHARED\Fisica-pendiente-2026-10-02\P1b-polarizaciones`.

Con proyecciones idénticas y frecuencias horizontales coincidentes, las
diferencias entre horizontales permanecen oscuras: tener más estados no basta
para hacerlas audibles. Los estados ya permiten ensayar una tabla y geometría
distintas en P2. El piloto no identifica masas, admitancia lateral ni contacto
tridimensional real; tampoco sustituye el intercambio no lineal pendiente en P3.

### Diseño aplicado — P2

Construir primero una placa ortótropa con costillas discretas en coordenadas
modales. Verificar masa y rigidez positivas, convergencia de frecuencias,
formas y proyecciones, y reciprocidad entre carga y excitación. Registrar
dimensiones, material, apoyos y orientación supuestos. No afirmar geometría
identificada del Steinway a partir de WAVs. Conservar una placa sin costillas
como control analítico antes de conectarla al renderer.

### 2026-10-02 — P2 estructural y acústico medidos

[Resultados y reproducción](RESULTADOS-P2.md). Placa rectangular ortótropa,
costillas discretas y puente como vigas. Masa/rigidez positivas, comparación
analítica, reciprocidad y pasividad verificadas. Convergencia por enriquecimiento
Ritz del banco con costillas: cambio máximo de frecuencias 0,3415% para 256 modos
y de movilidad 0,51% entre 250 y 1000 Hz en los puntos probados. Estos controles
no identifican geometría del Steinway ni resuelven la carga acústica.

Se conectaron tres variantes bajo 800 Hz, conservando los 121 modos altos de
fábrica, con proyecciones compartidas de carga, excitación y superficie.
Dieciséis ataques y cuatro sostenidos por variante; controles apagados exactos.
Además, cuatro ataques fuertes por variante con el observador acústico causal.
La mejora de C4 con placa y puente en el observador habitual no persiste en ese
control causal. La variante con costillas recupera parte del fundamental
sostenido de C4, pero empeora C5 y el cuerpo del ataque. **Sin mejora global
demostrada; ninguna seleccionada.** No se cambió el paquete instalado.

Audios en `C:\SHARED\Fisica-pendiente-2026-10-02\P2-tabla-audio` y
`P2-tabla-causal`. Reportes originales, hashes y resumen conservados.

### Decisiones de seguimiento dentro de P2

1. Convergencia del banco bajo verificada en el ensayo siguiente; pendiente
   el complemento alto y la identificación del observador acústico completo.
2. Ensayar geometría global de puente y apoyos con controles separados.
   El puente recto centrado conserva simetrías y deja modos sin excitación;
   el puente real curvo puede cambiar esas proyecciones. Es una hipótesis
   pendiente, no la causa ya demostrada del déficit de C5.
3. Separar errores de carga mecánica de errores de proyección al micrófono.
   No elegir una tabla sólo porque aumenta graves o baja el score medio.
4. Antes de promover: extender registros, pedales/reiteraciones, estados durante
   cambios de programa, headroom interno y coste en tiempo real/WASM.
5. Mantener P3/P4/P5 en el seguimiento. P3 debe auditar los términos cruzados
   anotados debajo; no confundir esa corrección pendiente con el resultado P2.

### 2026-10-02 — P2 resolución y curva global medidas

[Resultados y reproducción](RESULTADOS-P2-geometria.md). Refinar la cuadratura
baja cambia poco la presión; 16 puntos de puente producen hasta ≈0,806 dB
de error en los casos armónicos probados, insuficiente para explicar el
déficit tardío de C5. No se retocó la fábrica por este resultado.

El puente recto centrado deja 16/35 modos bajos casi sin excitación. Se probó
una curva global supuesta de 10 cm con masa, rigidez y formas correspondientes,
y un control que mueve sólo la trayectoria. Con observador causal en todas
las variantes, la curva consistente mejora frente al recto, pero C4/C5 siguen
peor que la base causal; el promedio 7,75351 no supera la fábrica habitual
(6,83418). **Sin selección.** Tres nuevas verificaciones nativas pasan; los
controles fuertes anteriores son exactos. Audios en SHARED.

La separación de rutas identifica en C5 una carga de puente 5,361 veces la base,
además de una transferencia de presión 4,265 dB menor por fuerza fija. No es
una medición del Steinway. Pendiente P2: masa/geometría y rango de validez de
la aproximación de carga débil a frecuencia congelada, especialmente cerca
de picos. No reducir pérdidas arbitrariamente para cubrir esos casos.

Siguiente etapa del orden: **P3**, auditoría de energía y normalización por
cuerda. Los pendientes de P2 permanecen registrados; no se declara tabla
completa o identificada antes de seguir con otros mecanismos.

### Incertidumbre adicional a auditar en P3

El comentario de `Voice::tension_step` describe suma de pendientes cuadradas
por cuerda, pero el cálculo actual eleva al cuadrado una suma de estados.
Eso contiene términos cruzados. Auditar su interpretación, normalización y
balance de energía dentro de P3; no modificarlo como arreglo aislado sin control.

### Diseño del siguiente ensayo — P3

1. Explicitar unidades y energía de estados de cuerda, masa modal, tensión,
   fuerza de puente y coordenadas de tabla. La norma modal heredada de P1b
   no basta para afirmar conservación en julios.
2. Derivar por cuerda el incremento de longitud: suma de energías de pendiente,
   separando direcciones/polarizaciones. Comprobar qué términos cruzados son
   físicos dentro de una cuerda y cuáles aparecen sólo al sumar cuerdas distintas.
3. Auditar la tensión común actual y el paso de actualización cada 32 muestras;
   identificar el efecto del suavizado antes de sustituirlo.
4. Ensayo conservativo aislado sin pérdidas: balance energético con referencia
   independiente y convergencia temporal. Después, pérdidas positivas y
   intercambio transversal/longitudinal bidireccional.
5. Controles congelados de ataque, sostenido y evolución con intensidad en varios
   registros. No mezclar a la vez una nueva afinación, geometría de tabla y contacto.
   El cambio de energía debe explicar una variación audible antes de hacer refit.

### 2026-10-02 — P3 energía, geometría y precisión medidas

[Resultados y reproducción](RESULTADOS-P3.md). Se construyó un núcleo aislado
con dos direcciones transversales y movimiento axial, derivado de un potencial
compartido en julios. Conserva energía y contabiliza las pérdidas positivas.
La convergencia contra un integrador independiente muestra que conservar energía
no basta: a 44,1 kHz el caso resonante conserva mal la fase; a 32 veces esa tasa
el error final de estado ponderado por energía baja a 0,6884% en 12 ms.
Ese núcleo todavía no está integrado ni identificado con el Steinway.

Los controles nativos de estiramiento y fuente longitudinal por cuerda no
mejoran las diez notas/intensidades medidas. El inspector encuentra tensión
solicitada sin cambio representable de polos en C5/C6/F#6 fuertes durante los
primeros 120 ms. C4 tiene cambios sólo en algunos parciales, hasta 0,0004343
cents; F#2 llega a 0,01527 cents. Es una limitación medida del cálculo actual,
no prueba de que la no linealidad física real sea despreciable.

**Ninguna variante seleccionada.** Fábrica y paquete preservados. Cuatro pruebas
algebraicas, balance/convergencia físicos y ambas huellas nativas pasan.
Reportes, CSV y audios en `C:\SHARED\Fisica-pendiente-2026-10-02\P3-*`.

Siguiente trabajo dentro de P3:

1. Derivar y medir la conversión del contacto SI a desplazamiento/velocidad por
   cuerda y fuerza de puente. Hoy hay normalización empírica entre esas etapas.
2. Mantener una frecuencia de referencia explícita y un desplazamiento acumulado
   representable; evitar perder cada incremento pequeño al redondear el polo.
   Validar con una trayectoria prescrita antes de activar la no linealidad.
3. Elegir integración con energía y fase verificadas a coste viable; no trasladar
   directamente el ensayo implícito a 32× al callback.
4. Conectar intercambio axial/transversal bidireccional y carga conjugada con
   controles de potencia, contacto, reiteraciones y pedal. Retener los pendientes
   de P2 sobre carga débil cerca de resonancias.
5. Repetir comparación por registro/intensidad; sólo después considerar refit.

Los crudos intermedios propios de P2 se archivaron de forma reversible en
`local/concert-grand-ritz/P2-native-raw-cache.zip`, con hashes individuales
verificados en `P2-native-raw-cache.json`. Los WAV, reportes, fuentes y binarios
continúan en sus ubicaciones; no regenerar todo P2 sin necesidad.

### 2026-10-02 — P3 contacto y referencia acumulada medidos

[Resultados y reproducción](RESULTADOS-P3-contacto.md). El nuevo control conserva
una referencia de frecuencia y radio en doble precisión; no descarta cada
incremento por redondeo del polo. Dos verificaciones nuevas pasan, incluyendo
volver exactamente a la base, respetar la pérdida del apagador y no reiniciar
polos retirados. Los coeficientes finales siguen cuantizados a `f32`.

Veinte controles apagados son exactos frente a los audios P3 anteriores. La
referencia acumulada cambia muy poco el sonido: score medio 6,51316 frente
a 6,51306 de fábrica; la geometría por cuerda con referencia llega a 6,51312.
**Sin selección.** La corrección numérica sola no recupera cuerpo.

El contacto SI termina por separación en los diez casos, sin alcanzar el cap
de 20 ms. Su conversión a estados audibles varía con nota, parcial y velocidad:
en el fundamental de F#6, pasa de 6407 a 443 unidades nativas por metro entre
velocidades 60 y 124. El piloto dispone ahora de coordenadas de masa, energía,
incremento de longitud y fuerza de tensión en SI con controles espaciales y
de trabajo. Todavía no se utiliza para controlar la síntesis audible.

Se identificó además una subida inicial de afinación dibujada en graves que
sigue activa: F#2 fuerte arranca 3,402 cents por encima del caso suave. El
contacto deriva la masa lineal de ese `f0` ya elevado, reduciendo su masa
modal 0,3923% al cambiar velocidad. Los contadores de descenso están en cero;
no es el intercambio no lineal nuevo. Corregir la representación física usando
la escala de referencia y evitar duplicar esa subida al integrar P3.

Siguiente ensayo de integración: conservar estados físicos del contacto antes
de la normalización, separar su conversión al micrófono y contabilizar el
trabajo de inicialización no lineal. El contacto actual es lineal en las
cuerdas: añadirle energía de estiramiento o un estado axial arbitrario al
terminar puede crear un ataque artificial. Validar ese paso antes de comparar
otro candidato completo o promover la referencia de polos.

### 2026-10-03 — P3 contacto acoplado desde el inicio

[Resultados y reproducción](RESULTADOS-P3-contacto-acoplado.md). El martillo,
fieltro y movimiento transversal/axial comparten energía desde primer contacto.
Al separarse se conserva el estado sin normalización ni energía añadida.
C4/C5 a velocidades 60/124 pasan balance, pérdidas positivas y convergencia
contra DOP853: error final de estado entre 0,00274% y 0,06952% a 705600 Hz,
en 6 ms. Al refinar el paso temporal, el error cae aproximadamente cuatro veces.

La resolución modal permanece abierta: de 2 a 8 modos axiales, la energía
de cuerda cambia +1,26%/+2,26% en C4/C5 fuertes y su reparto axial cambia más.
No identificar el mecanismo real a partir de un truncamiento pequeño.
No se produjo una voz audible nueva ni se modificó el paquete. Seis pruebas
Rust, exportación SI y dos huellas de fábrica pasan. Datos y seguimiento en
`C:\SHARED\Fisica-pendiente-2026-10-03`.

Siguiente paso: resolución y dispersión del contacto, reiteración desde un
estado físico existente y transferencia SI de potencia al puente/salida,
con masa independiente de velocidad. Conservar el control y una ganancia
común antes de comparar por oído o intentar otro refit. P2/P4/P5 continúan abiertos.

### 2026-10-03 — P3 resolución y golpes sucesivos

[Resultados y reproducción](RESULTADOS-P3-resolucion-reiteracion.md). Ocho
casos C4/C5/C6/F#6 a velocidades 60/124 pasan criterios previos de convergencia
de energía, impulso, tiempo, fuerza y estado en el piloto de extremos fijos
con flexión Euler. Se necesitan 2N axiales para cubrir la fuente de N modos
transversales; dos axiales no bastaban. Últimos refinamientos: 24→48 para
siete casos, 48→72 para C4 fuerte. Esto no completa Timoshenko ni identifica el piano.

Dieciséis controles conservan la cuerda ya vibrando e incluyen pérdidas
positivas. Dos muestran separación temporal con martillo aún avanzando;
seguir hasta el regreso cambia el intercambio de energía. La mecánica de
captura real sigue pendiente. Ocho golpes rápidos cierran el presupuesto
acumulado a 3,01e−14 relativo. El exportador nativo confirma que los diez
contactos de fábrica desde reposo terminan con martillo retrocediendo.

Se verificó una distinción en la reiteración nativa: la fase de la voz se
conserva, pero `simulate_strike` calcula el nuevo contacto con q/v inicialmente
cero. Su reacción al movimiento existente no está integrada durante el golpe.
Cinco pruebas Python, seis Rust, exportación y ambas huellas pasan; fábrica
y paquete preservados. Datos en `C:\SHARED\Fisica-pendiente-2026-10-03`.

Siguiente trabajo: conservar estado SI por cuerda y acoplar el puente móvil
con reacción recíproca, masa y potencia consistentes. La reacción calculada
con extremo fijo sirve de diagnóstico; excitar la tabla con ella sin reacción
no paga su energía. Validar la ruta de observación y usar ganancia común antes
de comparar sonidos. P2/P4/P5, backcheck, pedal y coste siguen pendientes.

### 2026-10-03 — P3/P5 puente móvil recíproco

[Resultados y reproducción](RESULTADOS-P3-puente-movil.md). La cuerda, el
martillo y el soporte vertical comparten masa y potencial. Se incluye la
inercia cruzada de mover el extremo de cuerda; la fuerza y velocidad de
interfaz verifican el trabajo que pasa al soporte. Ocho pruebas pasan.

Un soporte elemental y los 35 modos físicos bajo 800 Hz de P2 cierran
energía y pérdidas. En C4 fuerte, 24→48 aún falla el criterio de energía y
tiempo; 48→72 pasa. El error de velocidad del puente baja a 0,00560% RMS
con el oscilador y 0,00824% con P2. La resolución del resto del teclado
con extremo móvil no está verificada. Balance máximo: 1,423e-13 relativo.

La ablación de soporte excitado sin reacción recibe trabajo externo, que
debe contabilizarse aparte; no es una medición del error de la fábrica.
El soporte recibe poca energía durante el contacto en este golpe. No hay
evidencia de mejora audible ni un nuevo refit. Paquete preservado.

Siguiente etapa: observación de presión desde el movimiento SI de tabla,
con superficie/formas consistentes y ganancia común. Luego comparación por
registro/intensidad. Siguen abiertos banco alto, geometría real, unísono,
backcheck/pedal, masa de graves, Timoshenko e implementación en tiempo real.
Datos y copia del seguimiento en `C:\SHARED\Fisica-pendiente-2026-10-03`.

### 2026-10-03 — Commit y observación SI de superficie

Investigación acumulada del piano y sus herramientas/documentación guardada
en **346b6c8**, por pedido del usuario. El trabajo posterior del observador
continúa como etapa independiente.

[Resultados y reproducción](RESULTADOS-P3-observacion-SI.md). Observador de
presión desde las formas SI de la tabla, con retardos punto a punto y sin
ganancias ajustadas por nota. Seis pruebas independientes pasan, incluyendo
pistón analítico, temporal retardado, simetría y potencia radiada positiva.

La captura C4/124 de 4 ms conserva exactamente los estados congelados de la
etapa anterior. La presión en 60–800 Hz cambia 0,01020% RMS complejo al
refinar 48→72 modos de cuerda. Superficie 64×48 y retardo a 192 kHz pasan
los criterios declarados. No se produjo ni seleccionó un piano audible.

Se agrega **P6** para seguir la carga del aire: en el banco bajo P2 supuesto,
su radiación representa 16,78–70,31% de la disipación modelada bajo las cuatro
excitaciones armónicas de prueba. No es una medición del Steinway ni de la
fábrica. El observador lee movimiento, pero aún no devuelve carga acústica.
El 0,046 existente puede incluir radiación; separar pérdidas antes de añadir
resistencia, e incluir la parte reactiva para conservar energía del aire.

Siguiente trabajo: ensayo reducido recíproco tabla/aire con un control
analítico de pasividad/energía, y después fuente SI de mayor duración/banda
para escuchar. Banco alto, unísono, masa de graves, Timoshenko, backcheck/pedal
y coste siguen registrados. Datos en `C:\SHARED\Fisica-pendiente-2026-10-03\P3-observacion-SI`.

### 2026-10-03 — P6 carga reactiva y radiación recíprocas

La observación SI anterior quedó en **aaa520d**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-carga-aire.md). Se implementó la
impedancia exacta de una esfera pulsante como control analítico, con fuerza
recíproca, almacenamiento inercial y canal de onda saliente. Ocho pruebas
pasan, incluyendo solución de campo, pasividad, exponencial matricial y
contacto no lineal contra DOP853 independiente.

El soporte lineal supuesto cambia su pico de velocidad de 180 a 158,2 Hz al
cargarlo con ese puerto. Una resistencia constante sólo cambia amplitud en
ese control. No se trasladan esas frecuencias ni la geometría esférica al piano.
El elemento inercial puede devolver energía sin hacer negativa la radiación.

C4/124 con soporte elemental y esfera pasa resolución 48→72: error de
velocidad de soporte 0,00597% RMS, radiación 0,000717% y trabajo 0,00118%.
El control sin aire reproduce exactamente el estado congelado. El presupuesto
total recíproco cierra a 8,280e-15 relativo; una ablación sin reacción paga
su energía con un driver externo contabilizado. Fábrica/paquete preservados.

Siguiente: impedancia **compleja modal de P2**, autointeracción de superficie
y realización causal/pasiva con términos entre modos. No reemplazar la placa
por esferas por nota, ni duplicar masa/pérdida ya incluida en datos medidos.
Conservar estado acústico global entre golpes. Después ampliar banda/duración
para escuchar. Pendientes previos siguen abiertos. Datos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-carga-aire`.

### 2026-10-03 — P6 impedancia compleja de la superficie P2

El control de carga acústica anterior quedó en **3bbb170**, por pedido del
usuario. [Resultados y reproducción](RESULTADOS-P6-impedancia-tabla.md).
Resistencia y reactancia se calculan juntas desde el kernel saliente de
Rayleigh, conservando todos los términos entre los 35 modos físicos bajos.
Se integra la autointeracción y la interacción singular entre celdas vecinas.
Siete pruebas pasan, incluido un pistón circular con solución compleja analítica.

En seis frecuencias de 60–800 Hz, la resistencia coincide con un integral
angular independiente de la misma superficie a 1,470e-15 relativo. La malla
fina aproxima la superficie Ritz continua con error máximo de 0,07351%.
192×144→256×192 cambia la resistencia hasta 0,05718% y la reactancia hasta
0,11058%. La potencia armónica cierra a 6,093e-16 relativo. No se recortan
autovalores para hacer pasar pasividad. Son controles en puntos declarados,
no una prueba temporal global ni una medición del Steinway.

Siguiente: representación causal y pasiva de la matriz con estados acústicos,
validada en una banda más densa y contra la matriz compleja, manteniendo
interacción entre modos y energía almacenada. Separar pérdidas estructurales
de radiación antes de integrar contactos largos. Después ampliar banco,
unísono y pedales para escuchar con ganancia común. La fábrica y el paquete
continúan preservados. Datos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-impedancia-tabla`.

### 2026-10-03 — P6 estados acústicos y tabla cargada en el tiempo

La matriz compleja anterior quedó en **ec03076**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-estados-acusticos.md). Realización
espectral mediante canales oscilatorios recíprocos y masa de cola positiva.
Su identidad de energía establece pasividad; seis pruebas independientes
pasan. En 187 frecuencias de 60–800 Hz, el error máximo de impedancia compleja
es 0,22332% y el de velocidad de tabla cargada 0,20272% frente al control
Rayleigh de la misma superficie. La absorción del baño es numérica y se
documenta separadamente de pérdidas físicas y ondas almacenadas.

Dos impulsos separados 8 ms conservan el aire. El ensayo de 24 ms necesita
192→384 kHz para pasar el criterio temporal de 0,1%: velocidad cambia
0,03535% y desplazamiento 0,00867%. El balance total cierra a 6,168e−13
relativo, incluyendo trabajo instantáneo de la masa de cola. Hay devolución
de energía al puerto con absorción no negativa. No se promovió audio/paquete.

Siguiente: contacto no lineal SI con esta referencia, incluyendo la cola
inercial en la masa compartida. Luego reducir coste y verificar horizontes
largos y recurrencias antes de escuchar. El banco tiene 352026 estados de
aire y no es una implementación lista para tiempo real. Partición de pérdidas,
segunda cara/borde, banco alto, unísono, Timoshenko, graves y acción/pedal
siguen abiertos. Datos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-estados-acusticos`.

### 2026-10-03 — P6 contacto no lineal y aire persistente

Los estados acústicos anteriores quedaron en **e244450**, por pedido del
usuario. [Resultados y reproducción](RESULTADOS-P6-contacto-aire.md). El
contacto SI de cuerda/martillo/puente recibe la matriz completa del aire;
la masa de cola entra en la inercia compartida y se verifica el trabajo
entre cuerda, tabla y aire. Seis pruebas pasan, incluida una ODE independiente
en coordenadas físicas y el límite desacoplado contra el contacto previo.

En C4/124, 4 ms: 384→768 kHz cambia la velocidad de puente ≈0,01112% RMS,
y 48→72 modos de cuerda ≈0,00827%. Energías, trabajo, fuerza, impulso y
duración pasan sus criterios previos. El control sin aire aproxima la
velocidad final DOP853 congelada con error 0,04511% RMS. Un segundo martillo
prescrito conserva exactamente los estados vivos; sus balances cierran,
pero su refinamiento de precisión sigue pendiente.

La carga reduce 5,66307% la energía de tabla a 4 ms respecto del control
sin aire, mientras el primer contacto permanece prácticamente igual.
Esto no demuestra resolver el ataque fino. La referencia conserva una
cuerda, tabla hasta 800 Hz y una cara en bafle; no es audio de fábrica.
Datos y fuentes en `C:\SHARED\Fisica-pendiente-2026-10-03\P6-contacto-aire`.

Siguiente: reducción pasiva contra este banco completo, precisión de
contactos reiterados y horizontes largos. Luego ampliar banda, registros,
unísono y observación SI para escucha con ganancia común. La partición de
pérdidas, P4/Timoshenko, acción y pedal continúan pendientes. No se cambia
el paquete por la validación de esta referencia.

### 2026-10-03 — P6 reducción y rechazo por ecos tardíos

El contacto con aire anterior quedó en **6ebf47e**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-reduccion-aire.md). Cuatro pruebas
independientes pasan. Pesos de residuos PSD no negativos y cola PSD conservan
pasividad y masa estática. El entrenamiento usa sólo impedancias de la
referencia física, con verificación fuera del entrenamiento.

La variante de 17119 canales (10,28× menos) pasa frecuencia, tabla de 24 ms
y dos contactos C4 de 4 ms. Pero el control acústico de 64 ms con idéntica
velocidad prescrita detecta un eco a 29,0625 ms: error de fuerza 49,39123%
RMS y pico tardío 55,30072% del pico inicial. **Rechazada.** La absorción
alta baja el error pero también falla. No se cambiaron límites para aceptarla.

Conservar la grilla/Γ y descartar sólo direcciones espaciales débiles pasa
frecuencia y memoria: 0,29763% RMS, pico tardío 0,06273%. Reduce 30,29% los
canales; no cumple el objetivo de 10× ni tiene aún todos los contactos.
Ninguna variante se selecciona. Datos, fuentes y rechazos en
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-reduccion-aire`.

Siguiente: aproximación racional matricial con polos compartidos/adaptados,
certificación de pasividad y estos mismos controles de memoria. Contrastar
también la referencia finita con el kernel geométrico causal al ampliar el
horizonte. No volver a adelgazar la grilla suponiendo que pasividad o una
curva de frecuencia buena resuelven la memoria. Fábrica y pendientes de
banda alta, unísono, P4, pérdidas, acción/pedal continúan en el plan.

### 2026-10-04 — P6 carga racional y límite transitorio

La reducción anterior quedó en **77f00db**, por pedido del usuario.
[Resultados y reproducción](RESULTADOS-P6-carga-racional.md). Un factor de
ondas de la superficie, con pares angulares invertidos y transformación de
Cayley, genera una carga recíproca pasiva con almacenamiento verificable.
Seis pruebas nuevas pasan; las 22 de esta etapa y sus referencias pasan.
No se implementa Vector Fitting ni se ajustan muestras/programas.

Una cola de masa ya PSD permite 5740 estados frente a 352026 (61,33× menos).
Z/R/X/Laplace, masa y velocidad cargada pasan los límites previos bajo 800 Hz.
El balance del factor físico cierra por debajo de 7,22e−16 a tres pasos.
La memoria de 64 ms ya no tiene el eco recurrente anterior, pero el mejor
RMS de fuerza es 5,29358% con límite 2%: **rechazada para promoción**.
Un kernel continuo con parada exacta confirma que el fallo no se resuelve
refinando el integrador. La memoria no es una nota larga acoplada.

Fuera de la banda baja, el diagnóstico geométrico detecta error complejo
20,63944% a 6 kHz en esa variante. La cola inercial tampoco tiene el límite
resistivo finito del Rayleigh físico; una corrección positiva aditiva no
siempre es posible. Esto explica limitaciones del método, sin atribuir todavía
todo el error a un único mecanismo. No basta aumentar factor_rate.

Siguiente: contrastar el transitorio de ambos modelos con el kernel causal
geométrico directo y completar la banda alta respetando masa, resistencia
límite y pasividad. Después retomar tabla/contactos reiterados, precisión
del segundo contacto, demás registros y escucha. No se modifica fábrica;
coste real, unísono, P4, pérdidas, acción/pedal y ambas caras siguen abiertos.
Datos en `C:\SHARED\Fisica-pendiente-2026-10-03\P6-carga-racional`.
