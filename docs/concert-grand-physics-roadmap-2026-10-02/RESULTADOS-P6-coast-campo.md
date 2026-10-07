# P6 — Balance angular del coast y presión de la trayectoria viva

Fecha: 2026-10-07. Se continúa el
[control de coste/operador de micrófonos](RESULTADOS-P6-propagacion-coste.md).
El auditor y la captura quedan congelados en **e6c3a93**.
Se conservan fieltro, cuerda, tabla, modos, pérdidas y carga acústica.

## Hipótesis y alcance

El control previo reproduce el contacto y compara kernels de cálculo;
no mide por separado almacenamiento/radiación del coast completo ni la
presión de su trayectoria. Esta etapa cierra esas dos observaciones antes
de escoger una tasa para una duración audible.

- Una cuerda C4 supuesta, incidente a 5 m/s, 606 coordenadas físicas de
  tabla y todos los estados acústicos vivos. Misma R única autenticada.
- Se reconstruyen los 3 ms del contacto medido a 768 kHz. Se compara su
  extremo con el banco previo: energía mecánica, relajación, norma de onda
  y compresión. El martillo debe estar separado.
- Cada coast de otros **0,5 ms** parte del mismo extremo y conserva la
  memoria completa. Tasas 64/128/256 kHz; se captura cada extremo nativo,
  posición/velocidad físicas de tabla y velocidad centrada del puerto.
- El factor angular completo se regenera por bloques. La base Q sirve
  únicamente para observar campo, con residuo y cotas; no da fuerzas ni
  reduce la trayectoria. Sus inputs se guardan una vez en RAM.
- Las dos cápsulas usan las mismas posiciones supuestas del banco armónico,
  cuadratura de orden 4 y Green retardado. No se calibra nivel o EQ.

La energía de onda se obtiene del almacenamiento positivo del factor;
la radiación, del flujo angular integrado. **No se infiere como resto del
trabajo mecánico**. Se usan los extremos adversos de sus intervalos para
comprobar los balances total y del aire, con límite **1e−9** relativo a la
energía total inicial. Se mantiene precisión de observación **1e−4** con
piso 1e−14 J y el refinamiento de trabajo **0,2%**, con el mismo piso.

El refinamiento físico de tabla conserva 1% y razón < 0,35. Para presión
y captura se usa 0,1%, como el control temporal del piloto anterior; el
kernel conserva 0,02% contra Green continuo. No se ajustan tolerancias
después de medir.

## Captura y presión

Se compara la transformada compleja de la aceleración interpolada por
Hermite C1 en 60–6000 Hz, sondas cada 60 Hz. La integral exponencial se
evalúa analíticamente; no contiene impulsos delta por cortar la ventana.
Se repite usando uno de cada dos extremos nativos para medir sensibilidad
de captura, manteniendo la misma trayectoria de referencia y operador.

Para la comparación temporal todas las curvas se interpolan en la misma
grilla de **256 kHz** y usan el mismo kernel y tamaño FFT. La presión se
restringe a DC–6 kHz, la banda física comprobada, antes de comparar. Es una
medida de contribución de una **ventana finita de aceleración**, desde
3 hasta 3,5 ms del golpe, con su retardo de propagación y padding.
No simula soltar una tecla ni la cola completa de una nota sostenida.
No se genera un WAV para escuchar esa ventana tan corta.

## Controles dirigidos

Trece pruebas pasan en **0,223 s**: captura de coast con memoria viva,
replay exacto de los puertos y balance frente al factor angular independiente;
inyección de flujo incorrecto detectada por el balance; rechazo de contacto
y duración inválida; cuatro pruebas de observador, dos de intervalos y cinco
de presión/captura. La captura no altera el estado inicial recibido.

La prueba nueva de flujo no sustituye la medida de la geometría completa.
Las cotas analíticas de proyección incluyen salvaguardas numéricas; no
constituyen una prueba formal de redondeo mediante aritmética de intervalos.

## Resultado

[Medición del campo y presión](medidas-P6-coast-campo/coast-0p5ms.json).
Los cuatro diagnósticos del extremo inicial reproducen **exactamente** el
banco de coste. El observador elige **351 de 606** direcciones, cola relativa
9,679745e−13; fuerzas y trayectorias permanecen completas. Replay de los tres
extremos **0**, diferencia de norma del factor regenerado respecto de R[0]
**0**. Los inputs proyectados ocupan 408.844.800 bytes sólo en RAM.

| Control independiente | Máximo medido | Límite |
| --- | ---: | ---: |
| Balance total con extremos adversos | 4,583048e−11 | 1e−9 |
| Balance del aire con extremos adversos | 4,555940e−11 | 1e−9 |
| Cota relativa de observación | 2,326217e−7 | 1e−4 |
| Refinamiento de trabajo fino, seis términos | 0,0496604% | 0,2% |
| Error de kernel, 101 sondas DC–6 kHz | 0,000716863% | 0,02% |

El mayor cambio de trabajo es el flujo de onda del par 128/256 kHz.
El fieltro no aporta fuerza/calor en este tramo; calor material, relajación
y potencias de onda permanecen no negativos.

| Tasa | Almacenamiento final de onda, J | Flujo independiente, J |
| --- | ---: | ---: |
| 64 kHz | 4,563769138e−6 | 9,599884770e−8 |
| 128 kHz | 4,562991296e−6 | 9,618926871e−8 |
| 256 kHz | 4,562796565e−6 | 9,623701568e−8 |

Los límites inferiores del flujo son no nulos. En el caso fino son
**9,623699330e−8 J**; la radiación no se deduce del balance. Los valores
centrales del balance total cierran < 2,711e−13 relativo; las cotas de la
tabla anterior son más conservadoras.

## Presión y fallo localizado de grilla

| Tasa frente a 256 kHz | Presión compleja, 60–6000 Hz | Presión temporal, DC–6 kHz |
| --- | ---: | ---: |
| 64 kHz | 0,00604996% | 0,00604346% |
| 128 kHz | 0,00121814% | 0,00121680% |

El par fino pasa 0,1% en ambos dominios. Al conservar la trayectoria y
usar uno de cada dos extremos, la presión cambia 0,000279435% a 64 kHz,
0,0000177136% a 128 y **0,00000111379%** a 256. La sensibilidad de captura
pasa en el caso fino. Razón de errores de presión compleja: **0,201348**,
compatible con el control de orden dos desde referencia fina finita.

Pasan **12 de 13 controles**, pero se conserva un fallo:
`second_order_board=false`. Las curvas de tabla interpoladas en toda la
grilla de 256 kHz cambian 1,561649e−5/5,806854e−6 frente a la fina. Su razón
**0,371841** supera el **0,35** fijado, aunque sus errores absolutos sean
pequeños. No se ensancha el límite ni se aprueba ese observable.

El control anterior de coste usaba extremos físicos **nativos comunes a
64 kHz**, sin interpolación, y medía razón 0,321249. Se prepara en
**8558e69** un control separado de esa definición, reconstruyendo el mismo
golpe y los tres coast. Debe reproducir tanto los diagnósticos de dinámica
como la presión y el observable interpolado que falló, antes de vincular
sus resultados al certificado del campo. No vuelve a integrar los ángulos
ni reemplaza la medida original; conserva su fallo y el mismo umbral.

[Control de tiempos nativos comunes](medidas-P6-coast-campo/coast-grilla-comun.json):
reproduce **exactamente**, diferencia relativa máxima **0**, los cuatro
diagnósticos iniciales, ocho diagnósticos por coast, las dos diferencias
de tabla interpolada y las cuatro medidas de presión compleja/temporal.
Se recuperan los 33 extremos comunes a 64 kHz directamente, sin spline:
errores frente a 256 kHz **0,00209233%/0,000672160%**, razón
**0,321249494**, bajo el 0,35 original. Pasan sus cuatro controles y los
**16 controles vinculados** de esta duración. El cambio de observable
se registra explícitamente; el fallo interpolado continúa en el padre.

No se atribuye por completo ese fallo a una causa física: las grillas y
pesos de la norma son diferentes. Este control aprueba la convergencia en
los tiempos físicos comunes usados antes; no certifica orden uniforme de
toda la extensión spline. La presión y captura tienen sus propias medidas
independientes, que sí pasan los límites fijados.

La [primera tentativa de alineación](medidas-P6-coast-campo/coast-grilla-comun-incompleto.json)
queda incompleta: el JSON final rechazó un booleano NumPy y no conservó
los diagnósticos finales. Su tiempo es el del último checkpoint, no el
total de la tentativa. No se utiliza para aprobar convergencia.
**d9cc07c** convierte los indicadores a booleanos Python y guarda los
diagnósticos antes de ensamblarlos; la repetición completa tarda
**285,662 s** (4,761 min), pico **2,847 GiB**. No vuelve a medir el campo
angular ni cambia la dinámica para superar el control.

Tiempo completo **1419,592 s** (23,660 min); certificado **1107,991 s**
(18,467 min), pico propio **4,308 GiB**. El certificado sigue dominando el
coste incluso en esta duración: antes de escalar la nota hay que mejorar
su evaluación sin alterar las fuerzas ni saltar los controles.
La puerta general permanece falsa; no se cambia el paquete ni el timbre.

## Siguiente etapa

Extender la duración conservando el control angular y la presión/captura
refinadas, y comprobar el coste del certificado antes de escalarlo.
Su operación de observación `H @ Q` es lineal: estudiar mover la proyección
a las formas de celda antes de construir los histogramas, conservando la
medida del factor completo y sus cotas. Esa posible aceleración aún no se
implementó ni midió; necesita comparación de campo/energía y coste.
La implementación posterior y su ensayo se registran por separado en
[proyección de celdas](RESULTADOS-P6-proyeccion-celdas.md); este documento
conserva el coste y los resultados de la ruta anterior.
Este banco conserva sólo **tres modos transversales por dirección y seis
axiales** de la cuerda supuesta: son los del control de contacto, no la
resolución amplia del piloto audible anterior. Antes de otra escucha hay
que refinar también la resolución de cuerda y su espectro, con el mismo
puente/carga; aprobar el observador de 606 no recupera parciales ausentes.
Luego segunda pulsación natural, apagador/release y cobertura
de notas/intensidades. Los parámetros geométricos siguen siendo supuestos:
estos controles no identifican un piano real ni aprueban timbre o plugin.

Datos e historias permanecen en RAM durante el ensayo; los informes se
guardan sólo en el repo. No se duplica R ni se crean archivos en SHARED.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1, BLAS de un hilo y sin bytecode.
Usar un destino nuevo para preservar informes:
Estos auditores verifican SHA de fuentes congelados; reproducir el campo
con **e6c3a93** y la alineación con **d9cc07c**. El observador modificado
posteriormente requiere el auditor de proyección enlazado arriba, que
autentica la referencia histórica y registra el cambio explícito.

```powershell
python tools/audit-concert-grand-full-coast-field.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --cost-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-propagacion-coste/pares-completos.json --microphone-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-propagacion-coste/microfonos-completos.json --rates 64000 128000 256000 --duration .0005 --max-array-gib 12 --report NUEVO_CAMPO.json
python tools/audit-concert-grand-coast-capture-grid.py --field-report NUEVO_CAMPO.json --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVA_ALINEACION.json
```
