# P6 — Contacto, captura y sostenido por bandas nativas

Fecha: 2026-10-09. Continuación del
[control de frase completa](RESULTADOS-P6-frase-integrada.md), cuyo error
global es pequeño pero falla el criterio relativo del 1 % en bandas altas.
Se mantienen las ecuaciones y los coeficientes del prototipo de tres teclas.

## Pregunta y alcance

Separar tres errores numéricos antes de ajustar el timbre:

1. Dinámica del contacto: midpoint 768 / 1536 / 3072 kHz y portadoras
   128 / 256 kHz, desde el mismo estado frío de C4 o C5, velocidad MIDI 84.
2. Captura: presión calculada con todos los extremos nativos frente a su
   representación Hermite a 128 kHz, sobre la misma trayectoria física.
3. Sostenido: coast 16 / 32 / 64 / 128 kHz durante 8 ms, desde exactamente
   el mismo estado de C4 al terminar el contacto fino de 2 ms.

Todas las teclas tienen apagadores abiertos. El micrófono es el mismo
operador congelado a 128 kHz para cada método; aquí no se identifica ni
certifica ese operador. Las ventanas del contacto duran sólo 2 ms. La
historia anterior al coast procede de midpoint 3072 kHz y no se declara
globalmente convergida. Los cuatro arrays vivos se conservan al cambiar
de solver. No se generan WAVs, ni se modifica el plugin instalado.

## Medida y controles

- Fuente: aceleración física del interpolante C1 de posición/velocidad
  de la tabla, integrada analíticamente por segmentos. Cada trayectoria
  conserva todos sus extremos nativos.
- Ventanas rectangular y Hann: multiplican la aceleración real, sin
  derivar una posición previamente recortada. No cambian la dinámica.
- Presión compleja en los dos micrófonos, sin alinear fase ni ganancia.
  Norma L2 por bandas 60–800 / 800–2500 / 2500–6000 / 6000–12000 Hz.
- Grilla fina de 957 frecuencias, separación máxima 12,5 Hz; control
  anidado de 479 frecuencias, máximo 25 Hz, mismos límites de bandas.
- Umbrales originales: error relativo de banda 1 %, discrepancia de norma
  entre grillas 0,1 %, cambio del error relativo entre grillas 0,0001
  absoluto (0,01 puntos porcentuales), oráculo Hermite independiente
  1e-10 relativo en 6 y 12 kHz. Un fallo se conserva como fallo.
- El oráculo usa los tiempos reales de cada segmento con otra evaluación
  de momentos; las pruebas contrastan además con cuadratura Gauss.

La presión de una ventana corta no equivale al espectro del WAV completo:
el recorte mezcla frecuencias y omite la historia fuera de la ventana.
Los controles permiten localizar sensibilidad numérica local; no atribuir
por sí solos toda la diferencia de la frase ni la distancia al Steinway.

Fuentes: `tools/measure-concert-grand-native-bands.py` y
`tools/concert_grand_band_diagnostics.py`. Cuatro pruebas pasan: integral
independiente de aceleración, captura del mismo polinomio cúbico y norma
compleja sin corrección de ganancia/fase, además de una oscilación conocida
de 6 kHz: su reconstrucción Hermite a 16 kHz crea una imagen a 10 kHz que
la interpolación a 128 kHz conserva. Esto prueba una limitación del
interpolante; no mide su magnitud en el piano. Se conserva una sola R en disco;
el layout contiguo es compartido sólo en RAM.

La primera ejecución falló al serializar un booleano de NumPy antes de
guardar el primer caso; se corrigió su conversión a `bool` y se repitió
el ensayo. No se perdió una comparación terminada ni se cambió la física.

## Hipótesis de dispersión

Para un oscilador lineal aislado de frecuencia angular `omega`, midpoint
avanza la fase con `omega_h = 2*atan(omega*h/2)/h`. El error de amplitud
compleja después de `T` es `2*abs(sin((omega-omega_h)*T/2))`. Para 12 kHz
y T = 2 ms resulta 12,087 % a 768 kHz, 3,027 % a 1536 kHz y 0,757 % a
3072 kHz. Las portadoras exactas evitan ese error en la parte cuadrática.

Estos números son una estimación analítica de un oscilador, no mediciones
del piano ni del WAV. El contacto no lineal, la carga acústica, el recorte
de la ventana y su mezcla de frecuencias impiden trasladarlos directamente
a una banda de presión. La comparación medida debe comprobar si la
referencia anterior también contribuye al fallo; no se presupone que
todo el error provenga de midpoint.

## Resultados

### Contacto C4 / velocidad 84

Errores relativos porcentuales, ventana Hann, respecto al contacto midpoint
3072 kHz. La comparación entre referencias también es local.

| Método | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz |
| --- | ---: | ---: | ---: | ---: |
| Midpoint 768 kHz | 0,127870 | 0,098962 | 0,115549 | **2,003638** |
| Midpoint 1536 kHz | 0,025575 | 0,019797 | 0,023121 | 0,401192 |
| Portadoras 128 kHz | 0,050404 | 0,038940 | 0,038532 | 0,296193 |
| Portadoras 256 kHz | 0,006585 | 0,007236 | 0,010304 | 0,149303 |

En la ventana rectangular, las cuatro bandas pasan: máximo 0,040050 %
para midpoint 768 kHz y 0,013868 % para portadoras 128 kHz. La ventana
Hann conserva el fallo de la referencia antigua en la banda superior.
Su energía de referencia se reparte 12,0312 / 54,0734 / 33,7676 / 0,127795 %
por banda; difiere del reparto del WAV de dos segundos.

Portadoras 128 kHz frente a la referencia antigua, ambas capturadas a
128 kHz, cambia 2,153773 % en la banda superior Hann. Frente a la
referencia fina cambia sólo 0,296193 %. Esto identifica una contribución
de la referencia anterior en este recorte, sin demostrar que explique
todo el error de banda de la frase completa.

La captura a 128 kHz de la trayectoria fina cambia como máximo 0,003439 %
en las bandas Hann de C4. Los errores mecánico/onda al final de 2 ms para
portadoras 128 kHz son 0,011185 / 0,095932 %. Se mantienen los estados vivos.

### Contacto C5 / velocidad 84

Mismo criterio y ventana Hann, con su propia referencia midpoint 3072 kHz.

| Método | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz |
| --- | ---: | ---: | ---: | ---: |
| Midpoint 768 kHz | 0,001415 | 0,006985 | 0,051024 | **1,169697** |
| Midpoint 1536 kHz | 0,000283 | 0,001397 | 0,010206 | 0,233990 |
| Portadoras 128 kHz | 0,002551 | 0,008810 | 0,038731 | 0,252336 |
| Portadoras 256 kHz | 0,000576 | 0,002014 | 0,009070 | 0,097303 |

La ventana rectangular pasa para todos esos contactos: máximo 0,029293 %
para midpoint 768 kHz y 0,021563 % para portadoras 128 kHz. La captura
128 kHz de la referencia fina cambia como máximo 0,001962 % en Hann.
Portadoras 128 frente a la referencia antigua capturada cambia 1,262566 %
en la banda superior Hann. Los errores mecánico/onda frente a la fina son
0,007862 / 0,048403 % al final. La contribución numérica de la referencia
anterior también aparece aquí; no se sustituye una referencia por el
Steinway ni se cambia el modelo físico.

### Primer control de sostenido: grilla insuficiente

Ensayo inicial completo: **1213,884 s (20,23 min)**, 14 casos y 27
comparaciones. Los controles espectrales de los diez contactos pasan.
El oráculo Hermite pasa también en los cuatro coasts, pero fallan sus
controles de grilla: hasta 0,235664 % en norma, límite 0,1 %; cambio del
error superior de coast 16/128 de 0,000230 absoluto, límite 0,0001.
Se preservan `observer_controls_passed=false` y
`comparison_grid_controls_passed=false`, junto a todos los valores.

Por ello los errores de coast no se usan para certificar una tasa. Los
valores preliminares Hann 16 / 32 / 64 frente a 128 kHz en 6–12 kHz son
63,852638 / 4,682347 / 1,677234 %. La interpolación de una trayectoria
gruesa a 128 kHz reproduce su propio Hermite casi exactamente: eso no
recupera el movimiento perdido dentro de sus pasos ni certifica su
presión física. La comparación rectangular es mucho menos sensible al
error superior; se conservan ambas ventanas y sus energías.

Evidencia inicial en
`medidas-P6-bandas-nativas-2026-10-09/bandas-nativas.json` y su único
archivo pequeño `espectros-presion.npz`.

### Refinamiento del sostenido y atribución

Terminado en **648,715 s (10,81 min)**, incluido repetir el contacto para
reconstruir su extremo. Referencia hasta 512 kHz, grilla fina de 1913
frecuencias, máximo 6,25 Hz frente a 12,5 Hz. Pasan todos los controles
originales en esta grilla: discrepancia máxima de norma 0,058283 %
(<0,1 %), cambio máximo del error relativo 0,000057566 absoluto
(<0,0001), oráculo Hermite máximo 8,235e-12 (<1e-10). No se relajan
umbrales. Se conservan la primera medición fallida y la nueva evidencia.

Se compara además la trayectoria más fina submuestreada a cada tasa:
esa comparación aísla la reconstrucción Hermite. La dinámica de cada
coast se contrasta con esa misma captura fina, para separar los dos
efectos sin ajustar fase, ganancia ni física. Sus errores no se suman
como escalares, ya que son diferencias de vectores complejos.

El contacto reconstruido reproduce exactamente los espectros rectangular
y Hann de la primera ejecución (error relativo 0). Su estado completo
se guarda una sola vez, 1.976.085 bytes, SHA256
`d4ccfd58dbdbbca11c8183e314879dd742e41f93c7a734dba7c5210a45a539c0`,
en `local/concert-grand-physics/P6-bandas-nativas-2026-10-09/Estado-C4-84-2ms-3072k.npz`.
No contiene otra tabla ni otra R. El informe conserva su contexto, hashes
de fuentes, tabla, R y base acústica para autenticar una reutilización.

#### Dinámica y presión del coast

Errores porcentuales Hann frente a la trayectoria nativa de 512 kHz;
misma historia y estado inicial. Seis trayectorias, 15 comparaciones.

| Coast | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz | Estado mecánico | Onda |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 kHz | 0,065710 | 0,600337 | **4,011701** | **63,936865** | **1,265874** | **7,054417** |
| 32 kHz | 0,016908 | 0,144375 | 0,954024 | **4,851579** | 0,054116 | **1,786624** |
| 64 kHz | 0,004160 | 0,035621 | 0,222356 | **2,001397** | 0,010041 | 0,442225 |
| 128 kHz | 0,000990 | 0,008477 | 0,052499 | 0,453207 | 0,002343 | 0,105293 |
| 256 kHz | 0,000198 | 0,001695 | 0,010470 | 0,094256 | 0,000468 | 0,021057 |

La relajación cambia 0,978596 / 0,243656 / 0,059210 / 0,014088 /
0,002807 % respectivamente. El estado mecánico usa norma energética
del núcleo, sin cabezas. El contraste de referencia 256/512 pasa las
bandas localmente, pero no prueba toda la historia contra el continuo.
128 kHz pasa el 1 % local de presión/estados en este tramo; 64 kHz
todavía falla la banda superior. En rectangular, 16 kHz falla también
2,5–6 y 6–12 kHz (2,022723 / 2,105568 %); las demás tasas pasan esa ventana.

La referencia Hann reparte su energía 77,983148 / 22,014236 /
0,00261377 / 0,00000182526 % por bandas. La banda superior tiene muy
poca energía: el error superior del 63,936865 % a 16 kHz equivale a
**0,008638 % de la norma total**. No es una predicción de un cambio
audible de esa magnitud ni identifica la falta de cuerpo frente al Steinway.

#### Capturar el mismo movimiento no es repetir la dinámica

Errores porcentuales en 6–12 kHz Hann. La primera columna de error
contrasta el Hermite submuestreado de la trayectoria 512 con su propia
representación nativa. La segunda compara la dinámica de la tasa indicada
con la trayectoria fina submuestreada a esa misma tasa: su denominador
es esa representación gruesa, no la presión nativa de 512 kHz.

| Tasa | Sólo reconstrucción, misma trayectoria fina | Dinámica, misma tasa de captura |
| --- | ---: | ---: |
| 16 kHz | 44,041886 | 51,517622 |
| 32 kHz | 0,836944 | 4,981476 |
| 64 kHz | 0,056053 | 2,007767 |
| 128 kHz | 0,003552 | 0,453884 |
| 256 kHz | 0,000210 | 0,094296 |

La reconstrucción a 16 kHz es un problema real del cálculo de fuente,
pero corregirla por sí sola no elimina la diferencia dinámica. A 32/64
kHz la reconstrucción de la trayectoria fina es mucho más precisa que
la propia dinámica a esa tasa. No atribuir automáticamente esa dinámica
al aire, al AVF no lineal o a una fase concreta del splitting: hace falta
aislar esos operadores con controles que preserven el mismo estado.

#### Coste y verificación

Los 8 ms de coast tardan 6,364 / 11,487 / 22,123 / 40,853 / 79,152 /
141,047 s a 16 / 32 / 64 / 128 / 256 / 512 kHz. Pasar directamente de
16 a 128 cuesta **6,42×** en este prefijo; no se traslada ese factor
a la frase completa con apagadores. No se vuelve a renderizar la misma
frase ni se adopta una tasa de fábrica en esta etapa.

Verificados independientemente: espectros finitos y dimensiones, las
normas de las 27 comparaciones originales y las 15 refinadas, energías
de referencia y controles de grilla, hashes de todas las fuentes
registradas, tabla, R, estado y datos espectrales. Los espectros se guardan
una sola vez por ensayo: 1.488.410 y 1.306.432 bytes. La segunda evidencia
es `coast-refinado.json` / `coast-refinado.npz` en la misma carpeta del repo.
No queda un trabajador activo. El total de ambos ensayos terminados es
31,04 min; excluye el primer intento fallido de escritura.

## Decisión y siguiente paso

- Conservar contacto a 128 kHz como candidato: C4/C5/84 pasan por bandas
  contra referencias más finas. La referencia de contacto anterior
  también aporta error en la banda superior Hann.
- Conservar el coast de 16 kHz sólo como control de coste/escucha previo.
  No aprobarlo por bandas ni estados. El coast 128 kHz pasa únicamente
  este tramo de 8 ms, no el sostenido, apagador o frase completos.
- Siguiente hipótesis: localizar el error dinámico del coast mediante
  un control cuadrático con el mismo estado y carga, separando el residuo
  no lineal de la composición mecánica–aire. Después ensayar el operador
  responsable con portadoras y una fuente continua más precisa, contra
  las referencias medidas. Toda ablación será sólo diagnóstico.
- Medir coste y trabajo/pérdidas junto a presión/estados; resolver sólo
  la reconstrucción de fuente no basta. No subir la tasa de toda la frase
  para declarar resuelto el coste, ni cambiar fieltro para compensar
  un error numérico.
- Completar controles de duración/release y ampliar con F4 para la escucha
  física siguiente. Siguen pendientes la precisión acústica de rango 192
  (9,2021 % frente al límite 5 %), el balance de energía/radiación completo,
  identificación real y el banco completo. Los gates globales siguen falsos.

Este resultado localiza errores del prototipo offline. No demuestra
mejora de timbre frente al Steinway ni actualiza el plugin 0.172.0.
