# Volver a la release y mejorar desde el sonido aceptado

Fecha: 2026-10-14. Base publicada: RackForge **v0.1.32**, Concert Grand **0.171.35**.

## Decisión y alcance

El usuario encuentra la última escucha algo mejor que la anterior, pero muy
lejos del modelo que funciona en la release. Se retoma ese instrumento como
base de desarrollo. El prototipo SI/nativo queda como investigación conservada;
no es un reemplazo seleccionado ni el punto de partida para otra calibración.

Se conservan estas semanas de trabajo y los cambios ajenos al piano. Esta
revisión congela la base publicada sin instalar, publicar ni cambiar su DSP.

**Distinción esencial:** el manifiesto de esta rama y el perfil
`plugins/concert-grand/calibration/factory-0.172.0.json` dicen 0.172.0,
pero el [tag publicado v0.1.32](https://github.com/kalexis1994/rackforge/releases/tag/v0.1.32)
incluye **0.171.35**. GitHub CLI confirma publicación el 2026-10-13 a las
08:23:18 de Buenos Aires. Su commit es
`91eb43f88495f0485e7a7ed61718280ddce23a8d`. El perfil 0.172.0 y los informes
que lo llaman «actual» pertenecen a la línea local de investigación. No se
los confunde con la versión incluida en la release ni se traslada su ganancia
25,120884 dB a la base publicada. El usuario confirma que quiere la versión
incluida en la release y elige **cuerpo y sostenido** como primera prioridad.
No se inspeccionó su instalación; la referencia reproducible es el tag publicado.

## Qué salió mal en el proceso

Se avanzó mucho en resolver y acelerar componentes físicos, pero la escucha
musical comparada con la release llegó demasiado tarde. La similitud entre
ocho y doce modos no lineales sólo mostraba que esa reducción cambiaba poco
un fragmento; no demostraba que ninguno de los dos sonara bien.

La primera mitad presentó ticks generalizados; la corrección de apagadores
mejoró el síntoma sin alcanzar el sonido aceptado. La prueba corta inicial,
con pedal levantado, no ejercitaba el fallo de levantamiento del apagador
que apareció después. Conservar eventos, energía y archivos finitos era
necesario, pero no cubría esa regresión musical.

También cambiaron simultáneamente demasiadas partes respecto de la release:
estado y excitación de las cuerdas, contacto, pérdidas, tabla, captación y
registro. La geometría y los materiales supuestos del prototipo no identifican
el piano real ni reproducen por sí solos la calibración de la release.
El último MIDI además usa tabla en un solo sentido y pérdidas diagonales;
no equivale al modelo físico recíproco completo investigado por separado.

## Aprendizajes que sí se conservan

| Aprendizaje | Evidencia y uso al volver a la release |
| --- | --- |
| Conservar energía no garantiza trayectoria correcta ni mejor sonido. | [P3](RESULTADOS-P3.md) separa balance de energía y error de estado. No usar una prueba de energía como aceptación de timbre. |
| El cuerpo audible depende de toda la cadena y de sus estados. | La [reconstrucción causal](../concert-grand-reconstruction-2026-10-02/README.md) necesita excitación auxiliar y compuertas MIDI además de los puertos del puente. Conservar esas rutas al comparar. |
| Más modos o una geometría más detallada no garantizan una mejora. | Los pilotos [P1a](RESULTADOS-P1a.md), [P1b](RESULTADOS-P1b.md) y [P2](RESULTADOS-P2.md) no demostraron una mejora consistente. No trasladarlos por completitud física. |
| La mayor parte del costo puede estar en física que no cambia esa escucha. | El [híbrido](RESULTADOS-P7-hibrido-lineal-no-lineal.md) retiene los portadores lineales y reduce sólo el potencial no lineal. Es una herramienta futura, no una mejora aprobada de la release. |
| El control debe probar teclas cortas, repetición y pedales variables. | La [corrección de ticks](RESULTADOS-P7-apagadores-ticks.md) reproduce un error oculto por el pedal del comienzo. Agregar silencio antes de un golpe no debe cambiar una nota mantenida. |
| Hay comportamientos que la release ya resolvía mejor. | El tag publicado ya pasa `the_top_octave_has_no_dampers` y los controles de medio pedal. La recuperación del apagador asentado y la reutilización de voz liberada tienen pruebas adicionales en 0.172.0; son candidatos separados, no prestaciones que deban atribuirse a 0.171.35. No copiar el límite provisional F6 del prototipo sobre el registro por programa. |
| La pasividad y la optimización siguen siendo herramientas útiles. | Se conservan solvers, controles de fase, filtros continuos, trazas y referencias congeladas. Su reutilización requiere un problema concreto del instrumento actual. |

## Preguntas para auditar la release y prioridad propuesta

Los diagnósticos históricos siguientes pertenecen principalmente a 0.172.0.
Orientan qué mirar, pero **no demuestran el mismo defecto en 0.171.35**.
Primero se mide la base publicada con su propia captación y calibración.

1. **¿Pierde cuerpo durante la transición al sostenido, especialmente alrededor de C4?**
   El [diagnóstico ya existente de carga y pérdidas](../concert-grand-decay-loading-2026-10-02/README.md)
   mide en C4/60 una caída del fundamental de 23,60 dB entre sus ventanas,
   frente a 6,10 dB en la referencia. Entre 900–1400 ms la pendiente ya es
   parecida, pero llega con unos 21,67 dB de déficit. Es un problema localizado
   y medido; esos números no describen todo el teclado ni son una escala de
   calidad percibida. Esos datos son del modelo local 0.172.0: verificar si
   el patrón existe en la release antes de tocar su transición o carga.

2. **¿Hay ataques o repeticiones cortas molestas en las notas señaladas?**
   La [investigación anterior de notas cortas](../concert-grand-short-releases-2026-10-01/README.md)
   distinguió duplicación de cuerda y click subjetivo; corregir lo primero no
   resolvía automáticamente lo segundo. Usar el pasaje y alturas originales,
   con sus estados anteriores, primero sobre 0.171.35, antes de proponer
   cambios de ruido o brillo. La reutilización de cuerdas de 0.172.0 se
   evalúa sola si el caso aparece; no se trae todo su modelo.

3. **Regularidad entre registros e intensidades.**
   Evaluar las alturas vecinas y las velocidades junto con el caso objetivo.
   Cambiar pérdidas globales, movilidad o detune ya produjo compromisos que
   empeoraron otras notas. Los resultados del corpus conocido son diagnósticos,
   no una identificación única del instrumento ni una validación ciega.

4. **Costo del instrumento completo bajo polifonía.**
   La release es la ruta tocable. Conservar sus controles de presupuesto,
   arranque WASM y equivalencia entre instancias. Medir un pasaje completo,
   incluidos sus ataques, antes de aceptar una optimización. El costo de un
   solver físico aislado no es el presupuesto de este instrumento.

El usuario selecciona el punto 1: medir y mejorar cuerpo y sostenido. No se
afirma haber corregido un punto débil de la versión publicada en esta revisión.

## Primera comparación sobre la base actual

La release queda congelada. Medir primero, sin alterar coeficientes: C3/C4/C5 a
velocidades 60 y 110, notas vecinas de C4 y controles con/sin pedal. Separar
ataque 0–100 ms, transición 150–800 ms, sostenido 900–1400 ms y liberación.
Usar la presión estéreo completa además de las trazas de fuerza/energía;
una cuerda con más energía puede radiar menos en esos micrófonos.

Si se reproduce el déficit, la primera hipótesis será el reparto y la pérdida
durante la transición del unísono, conservando la excitación y el receptor. Antes
de repetir una ablación, comprobar qué hipótesis nueva aporta: reducir o quitar
el drenaje, alargar globalmente T60, añadir memoria del mismo puente y cambiar
afinación ya tienen resultados que no aprobaron el conjunto.

Una variante modifica un mecanismo por vez y se escucha frente a la release
con los mismos eventos, programas, ganancia y estado anterior. El conjunto
corto debe incluir pedal bajo y variable, reiteración, registro medio/agudo y
una frase musical; después se amplía a La Campanella. Sólo una mejora audible
que conserve estabilidad y costo pasa a ser candidata de integración.

## Identidad de la base y comprobaciones

La versión del plugin es independiente de los tags `v0.1.x` del host. Se
extrajeron sin modificar los diez archivos de fuente/SDK/metadatos necesarios
del tag v0.1.32 a `local/concert-grand-physics/revision-release-2026-10-14/`.
Se compilaron directamente con Rust 1.98.0, edición 2024, sin sustituir fuentes
en el checkout ni dependencias del SDK de aquella versión por las actuales.

Pasan **seis controles** del snapshot publicado: dos huellas de audio,
registro sin apagadores, dos de medio pedal y reiteración sobre la misma cuerda.
Sus huellas son `e74df30116b39a5a` y `8d20f899b0509bd0`. El registro agudo
liberado conserva 0,752 de la energía del control con pedal, y C6, que sí tiene
apagador en ese ensayo, 0,056. Son resultados de esas pruebas específicas.

Las fuentes actuales de la rama 0.172.0 también compilan con Cargo y pasan
sus **dos huellas distintas**, `232b085ed4248398` y `4371ab4adc1542ab`.
Por tanto el sonido por defecto de esta rama **no es la release publicada**:
congelar sólo 0.172.0 habría establecido una referencia equivocada. Las
huellas fijan dos escenarios; no prueban todos los programas ni el paquete
instalado. La compilación de la rama fue nativa, no una nueva prueba WASM.

El checkout administrado de la release queda listo en
`C:/Users/kalex/.codex/worktrees/concert-grand-release-base/rackforge`, rama
**`codex/concert-grand-release-improvements`**, partiendo exactamente del
commit publicado. Está limpio y sus fuentes/SDK/metadatos coinciden con el
snapshot probado. Las próximas modificaciones del instrumento se hacen allí;
esta rama de investigación conserva los experimentos y esta revisión.

El [registro de procedencia y comprobaciones](revision-release-2026-10-14.json)
conserva los hashes de los diez archivos, del ejecutable de prueba y de la
fuente local comparada, los filtros ejecutados y la identidad del worktree.
