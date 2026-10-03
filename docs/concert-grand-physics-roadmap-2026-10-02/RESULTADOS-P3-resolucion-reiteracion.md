# P3 — resolución del contacto y cuerda ya en movimiento

2026-10-03. Se completa la comprobación de resolución del **piloto de extremos
fijos con flexión Euler** en ocho casos. Se verifica además la inicialización
de golpes sucesivos sobre estados físicos existentes. Esto no completa el
modelo Timoshenko, el puente o el sonido del piano; no se selecciona otra voz.

## Qué cambió en el piloto

`tools/concert_grand_contact_study.py` añade una cuadratura de cosenos exacta
para el potencial discretizado y una inicialización que conserva todos los
desplazamientos y velocidades de la cuerda. El núcleo SI y contacto de la
etapa anterior permanecen intactos; se extienden fuera de la síntesis nativa.

Para N modos transversales por dirección, el cuadrado de sus pendientes tiene
componentes hasta 2N. El ensayo anterior con dos modos axiales no representaba
toda esa fuente. Ahora se utilizan **2N axiales** y más de 2N nodos de punto
medio. La ortogonalidad de cosenos integra exactamente el potencial cuártico
y los productos de su gradiente en este espacio finito. Una DCT evalúa el
gradiente; su energía, gradiente y Hessiano coinciden con integrales espaciales
Gauss independientes dentro de 2e−12 relativos.

Se incluye flexión Euler con el área circular de acero supuesta, obtenida de
la masa lineal: E=200 GPa, densidad=7850 kg/m³. Se conserva la geometría nativa
y no se reajustan las frecuencias resultantes para igualarlas al MIDI.
Las masas son iguales a velocidades 60/124 en C4/C5/C6/F#6; la anomalía de masa
en F#2 registrada previamente sigue pendiente. No se usa el área total de una
cuerda entorchada como área del alma.

El campo puede tener ambas polarizaciones; en estos golpes verticales desde
reposo, la horizontal permanece nula por simetría. No se introduce detune,
imperfección aleatoria o reparto nuevo de unísono.

## Criterios y resultados de resolución

Se comparan 12→24→48 modos transversales por dirección, con 24→48→96 axiales.
C4 fuerte necesita además el control de 72 transversales/144 axiales. La
ventana es de 4 ms desde primer contacto; fieltro conservativo para aislar la
resolución. DOP853 usa rtol=1e−9, atol=1e−13 y paso máximo ligado a la frecuencia
más alta, incluida la axial.

Los límites se fijaron antes del barrido: cambio de impulso y energía de cuerda
<0,2%, tiempo de contacto <0,1%, forma de fuerza de contacto y reacción del
extremo filtradas al ancho de banda de 44,1 kHz <1%, y estado final ponderado
por energía <1%. Se muestrea la reacción a tasa alta y **se filtra antes de
reducirla**, para evitar alias de los modos axiales y sus productos.

| Caso | Último refinamiento transversal | Cambio de energía de cuerda | Diferencia de reacción del extremo | Diferencia de estado final |
| --- | --- | ---: | ---: | ---: |
| C4, 60 | 24→48 | 0,10011% | 0,15026% | 0,15112% |
| C4, 124 | 48→72 | 0,00427% | 0,06938% | 0,01373% |
| C5, 60 | 24→48 | 0,00645% | 0,49239% | 0,00703% |
| C5, 124 | 24→48 | 0,05241% | 0,72817% | 0,07265% |
| C6, 60 | 24→48 | 0,00120% | 0,30472% | 0,00064% |
| C6, 124 | 24→48 | 0,01712% | 0,43549% | 0,01321% |
| F#6, 60 | 24→48 | 0,00003% | 0,04786% | 0,00002% |
| F#6, 124 | 24→48 | 0,00293% | 0,16820% | 0,00172% |

Los ocho casos pasan todos los límites. En C4 fuerte, 12→24 cambiaba la
reacción filtrada **6,94%**; 24→48 daba 0,719% pero todavía excedía ligeramente
los límites de energía y duración. Conservar energía no validaba el número de
modos. El error relativo máximo de balance de todo el barrido es 2,89e−14.

La reacción en x=L incluye tensión local y cortante de flexión:

```
strain = u_x + (v_x²+w_x²)/2
F_vertical = (T + EA*strain)*v_x - EI*v_xxx
F_horizontal = (T + EA*strain)*w_x - EI*w_xxx
```

Es una reacción con extremo inmóvil, **no una potencia transferida a un puente
que ya se mueve**. Su componente de cortante y tensión no lineal se verifican
contra un campo seno analítico. Alimentar una tabla con esta reacción sin
reacción sobre la cuerda sería un control forzado, no un acoplamiento conservativo.

### Precisión temporal independiente

C4 fuerte, 12 transversales/24 axiales, Euler, 3 ms: refinar DOP853 de 12 a
24 puntos por ciclo máximo y tolerancias 100 veces más estrictas cambia el
estado ponderado 3,12e−14. El integrador AVF independiente tiene errores de
estado **0,02191% / 0,005478% / 0,001370%** a tasas nominales 705600 / 1411200 /
2822400 Hz. Sus cocientes de refinamiento son 3,9997 / 3,9990; el mayor error
de balance es 8,45e−13. Son medidas de este caso, no una validación temporal
de todos los estados del piano. El coste sigue siendo offline.

## Golpes sobre una cuerda que ya vibra

Se produce un primer golpe fuerte, se conserva su estado y se deja evolucionar
libremente. Luego se ensayan cuatro tiempos separados por un cuarto del
período fundamental y velocidades 60/124 en C4/C5: **16 controles**. Esos
tiempos no implican que todo el estado no lineal tenga una única fase.

El martillo nuevo comienza en primer contacto con la cuerda desplazada; sus
coordenadas existentes no se borran ni normalizan. La energía cinética del
lanzamiento es trabajo externo explícito. Se conserva:

```
cambio de energía de cuerda + energía de martillo saliente + calor
    = energía del martillo incidente
```

Se utiliza el fieltro pasivo supuesto d=0,1 s/m de la etapa anterior, no la
memoria de fábrica. Estos controles usan **12 transversales/24 axiales** para
verificar el procedimiento y balance; sus cifras de fase no heredan la
convergencia de fuerzas de los modelos de 48/72 modos.

El error relativo máximo de balance es 1,11e−13. Un golpe suave puede extraer
energía de una cuerda fuerte, mientras otro tiempo de contacto puede aportarla.
Por ejemplo, C4 suave en el offset2 pierde 0,008103 J de energía de cuerda:
el martillo se lleva parte de la vibración y otra parte se disipa en el fieltro.
No se representa ese fenómeno sumando siempre un ataque calculado desde reposo.

### Separación temporal y regreso del martillo

En dos controles suaves, la primera separación ocurre con el martillo aún
avanzando. Hay contactos posteriores en el modelo de martillo libre:

| Caso, offset3 | Primera separación | Velocidad del martillo | Separación con regreso | Velocidad al regresar |
| --- | ---: | ---: | ---: | ---: |
| C4, 60 | 0,7623 ms | +0,8244 m/s | 3,4114 ms | −1,0790 m/s |
| C5, 60 | 0,2550 ms | +1,8224 m/s | 2,1843 ms | −2,9111 m/s |

En C5, cortar en la primera separación sugería un cambio de energía casi nulo;
al seguir hasta el regreso, el cambio es −0,006394 J. En C4 cambia de
+0,001787 J a +0,000964 J. Se verifica el balance en el evento de regreso.
El selector omite la raíz inicial y separaciones con velocidad de martillo positiva.

**El regreso es un criterio mínimo de diagnóstico, no un backcheck identificado.**
La posición y tiempo reales de captura no están modelados; C4 libre presenta
incluso otro contacto posterior. No trasladar automáticamente una captura
instantánea en el primer regreso al instrumento real.

Ocho lanzamientos de C4 cada 10 ms, con velocidades alternadas, verifican además
el presupuesto acumulado. En esa secuencia las ocho primeras separaciones ya
tenían velocidad saliente negativa y un único segmento de contacto; se confirmó
antes de considerar su retirada ideal. Se contabilizan 0,202916 J de entrada,
0,134080 J retirados con los martillos, 0,026442 J de calor y 0,042394 J restantes
en cuerda. Error relativo final 3,01e−14. Es una prueba de esfuerzo con acción
ideal, no una tasa de repetición medida ni una verificación de pedal/apagador.

## Comprobación de fábrica y alcance del hallazgo

Se añadió **sólo al exportador ignorado** la velocidad firmada del martillo
saliente. Las diez notas/intensidades nativas desde reposo salen con velocidad
negativa y sin alcanzar el cap; su transferencia física es idéntica por bytes
a la exportada antes. Por tanto, los contactos temporales del ensayo en movimiento
no demuestran un fallo de separación en esas diez colisiones de fábrica.

El código actual `simulate_strike` inicializa q/v en cero y no recibe el estado
de la voz existente. La reiteración nativa conserva la fase audible y entrega
el contacto nuevo sobre esa voz, pero la colisión se calculó desde reposo.
Falta la reacción de su movimiento sobre el martillo durante el contacto.
El piloto dispone de esa inicialización sin alterar la fábrica.

Flexión Euler no completa P4. El modelo de
[Chabassier, Chaigne y Joly](https://perso.ensta.fr/~touze/PDF/Batwoman/chabassier-jasa.pdf)
incluye cizallamiento e inercia de rotación con Timoshenko y acoplamiento con la
tabla. Nuestro ensayo verifica una aproximación más limitada; la convergencia
numérica no identifica esos mecanismos ni demuestra mayor fidelidad al Steinway.

## Artefactos, pruebas y reproducción

En `C:\SHARED\Fisica-pendiente-2026-10-03\P3-resolucion-reiteracion`:

- `resolution/`: ocho reportes de refinamiento, fuerza filtrada CSV y estados NPZ.
- `accuracy/`: referencia y convergencia temporal AVF.
- `restrikes/`: los 16 controles de fase, antes de corregir la interpretación
  de las dos separaciones temporales.
- `recoil/`: comparación de ambas separaciones y estados de regreso.
- `sequence/`: ocho lanzamientos y presupuesto acumulado.
- `native-separation/`: exportación nativa con velocidad firmada.

Los resultados y hashes están en `resumen-P3-resolucion-reiteracion.json`, junto
al plan actualizado en la carpeta superior. Fuentes y nuevo inspector congelados
en `local/concert-grand-resolved-restrike/build/`:

- `source-audit.zip`: SHA256 `703d790499f893ee25f14ee8dddf4f44bd3aa6e44d8a104f647e0610852a4ef9`.
- `inspector.exe`: SHA256 `97d30731b0c75b968d548ddf0938418a5973fdc7c4bb55d956d5acbaf061ea95`.

Cinco pruebas Python pasan: integrales independientes, reacción analítica,
conservación del estado y pérdidas, efecto de velocidad existente y selección
de separación con regreso. Seis pruebas Rust, el exportador explícito y ambas
huellas nativas pasan: `232b085ed4248398` / `4371ab4adc1542ab`. El paquete conserva
SHA256 `ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

```
python -m unittest discover -s tools/tests -p test_concert_contact_study.py -v
python tools/audit-concert-grand-resolved-restrike.py --input <input> --output <resolution> --phase resolution
python tools/audit-concert-grand-resolved-restrike.py --input <input> --output <restrikes> --phase restrikes
python tools/audit-concert-grand-contact-accuracy.py --input <input> --output <accuracy>
python tools/audit-concert-grand-contact-sequence.py --input <input> --output <sequence>
python tools/audit-concert-grand-contact-recoil.py --input <input> --output <recoil> --phase controls
```

Python requiere numpy/scipy; un solo hilo BLAS. El exportador Rust usa
`CG_P3_TRANSFER_DIR` en una carpeta nueva y `RUST_MIN_STACK=33554432`.
Los resultados previos y sus fuentes congeladas permanecen preservados.

## Siguiente paso

1. Conservar un estado SI por cuerda, con masa/referencia independientes de
   velocidad; evitar reconstruirlo desde amplitudes audibles normalizadas.
2. Acoplar un puente móvil con reacción recíproca y masa positiva compartida:
   contabilizar el trabajo de contacto y la potencia cuerda/tabla durante el
   golpe y sostenido. Retener las limitaciones geométricas de P2.
3. Observar esa tabla por la misma proyección física que recibe la carga;
   después comparar audios con control congelado y una ganancia común.
4. Mantener abiertos Timoshenko, backcheck real, apagador/pedal, unísono completo,
   registros entorchados e integración a coste viable. No refit ni promoción
   del piloto antes de validar esa ruta de salida.
