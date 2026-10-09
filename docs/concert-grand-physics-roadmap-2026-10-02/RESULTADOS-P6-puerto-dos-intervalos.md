# P6 — Dos intervalos no lineales con un único puerto acústico

Fecha: 2026-10-09. Sigue al [control cuadrático del puerto](RESULTADOS-P6-puerto-cuadratico.md).
El error alto persiste sin no linealidad. Esta etapa comprueba cuánto
cambia al duplicar la resolución del residuo geométrico dentro del paso,
sin duplicar los estados ni el intercambio acústico.

## Método

`tools/concert_grand_port_half_avf.py` mantiene el mismo potencial,
rigidez, modos, tabla, pérdidas y aire pasivo. Para h/2 se definen
`Ch=cos(Omega*h/2)`, `Sh=sin(Omega*h/2)/Omega`, `Qh=(1-Ch)/Omega²`.
Los gradientes discretos completos son `N1=AVF(q0,qm)` y `N2=AVF(qm,q1)`.
La composición de portadoras bajo una misma fuerza de puerto F es:

```
qm = Ch*q0 + Sh*v0 - Qh*(N1+F)
q1 = C*q0 + S*v0 - Qh*((1+2*Ch)*N1+N2) - Q*F
v1 = -Omega²*S*q0 + C*v0 - Sh*((2*Ch-1)*N1+N2) - S*F
```

Aquí C/S/Q son las funciones del paso completo; todas las ecuaciones
se evalúan en coordenadas modales. La fuerza F sólo actúa en la tabla.
Se mantiene `u=Cb*(q1_board-q0_board)/h` y el mismo midpoint acústico.
La eliminación de F usa el mismo sistema `I+Qbb*E/h` de 606 coordenadas;
se calculan respuestas Q y Qh sin invertirlas ni recortar sus ceros.
La iteración conserva ambos gradientes completos.

Cada mitad conserva el trabajo del potencial y de la fuerza constante.
Al sumarlas, el trabajo de F es el del desplazamiento completo y coincide
con el puerto del aire. Se mantiene la memoria original. Las cabezas deben
estar separadas al inicio, mitad y final; el recontacto requiere el solver
de contacto. Método experimental, todavía fuera del renderer.

## Pruebas

Tres pruebas pasan:

- Balance de energía mecánica + almacenamiento acústico + pérdidas,
  trabajo del puerto y ecuación midpoint del aire; entradas sin mutación.
- Convergencia frente a ODE continuo independiente DOP853 con geometría
  no lineal, errores decrecientes más de 3× al refinar y fino menor que 0,1 %.
- Con el residuo desactivado, las dos mitades reproducen las mismas
  portadoras y los mismos cuatro estados que el método de un intervalo.

El primer intento de las pruebas tuvo tres errores por nombrar `residues`
en lugar del atributo existente `packed_residues`; se corrigió el fixture.
No se había iniciado un ensayo físico con ese error. Las tres pruebas
posteriores pasan. No se modifica el código de los productores anteriores.

## Medición

El productor `tools/measure-concert-grand-port-half-avf.py` reutiliza el
estado completo C4/84 a 2 ms, banco C4/G4/C5 con nueve cuerdas, 606
coordenadas de tabla y rango acústico 192. Física no lineal activa,
apagadores abiertos, ocho milisegundos, mismo micrófono a 128 kHz.
Referencia de presión física nativa a 512 kHz ya medida; replay de 128
y cotas triangulares de estados iguales a las del ensayo anterior.
Las referencias no se alinean ni normalizan. Se guardan también las
energías inicial/final, trabajo del puerto y calor para auditar el balance.

## Resultados

Ensayo completo **235,009 s (3,92 min)**. Replay de presión 128 exactamente
igual al anterior en ambas ventanas. Todos los controles de medición pasan:
variación de energía entre grillas máxima 0,057836 %, variación absoluta
del error 0,000007326 y oráculo Hann 4,624e-12. El balance mecánico,
calor material y trabajo acústico tiene máximo 4,795e-12 relativo; no es
certificado de radiación completa del banco grande.

Errores porcentuales Hann frente a la referencia física de 512 kHz:

| Método / tasa | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz | Tiempo de 8 ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Splitting previo / 128 kHz | 0,000990 | 0,008477 | 0,052499 | 0,453207 | 28,464 s |
| Dos intervalos / 32 kHz | 0,014046 | 0,126047 | **2,235782** | **23,732885** | 10,580 s |
| Dos intervalos / 64 kHz | 0,003511 | 0,031043 | 0,571772 | **6,023875** | 19,341 s |
| Dos intervalos / 128 kHz | 0,000872 | 0,007533 | 0,142384 | **1,499929** | 37,116 s |
| Dos intervalos / 256 kHz | 0,000218 | 0,001722 | 0,034815 | 0,364860 | 66,520 s |

Todas las bandas rectangulares pasan: máximo 0,442542 %. El método
sólo pasa todos los controles locales a 256 kHz. La banda alta Hann
sigue fallando en las tasas menores; tiene poca energía, de modo que
el porcentaje no cuantifica por sí solo una diferencia audible.

Cotas porcentuales de extremos respecto a 512:

| Tasa | Mecánica | Onda | Relajación |
| --- | ---: | ---: | ---: |
| 32 kHz | 0,053007 | **1,778237** | 0,209456 |
| 64 kHz | 0,009798 | 0,440016 | 0,052096 |
| 128 kHz | 0,003191 | 0,107584 | 0,018606 |
| 256 kHz | 0,004294 | 0,189799 | 0,026029 |

No interpretar el aumento de algunas cotas finas como error exacto:
usan una referencia de estado intermedia y desigualdad triangular.

## Atribución, coste y decisión

El método de un intervalo había dado error mecánico acotado 1,266027 %
a 32 kHz; ahora baja a 0,053007 %. Sus errores Hann altos bajan de
31,778702/6,957089/1,824584/0,377727 % a los de la tabla. La resolución
del residuo sí contribuía, pero no alcanza para pasar a 128 kHz.
El control cuadrático del mismo puerto ya falla ahí con 1,481181 %;
no hace falta retocar fieltro o desactivar física para explicar ese límite
numérico. No atribuir todo el error a un único mecanismo ni restar normas
de referencias diferentes para estimar porcentajes de contribución.

A 128 kHz esta variante tarda **1,304×** el splitting previo medido en
la misma ejecución; a 256, **2,337×**. Son costes de ocho milisegundos,
no predicciones de una frase o pieza. **Se rechaza como reemplazo para
acelerar el renderer**, aunque conserve trabajo y mejore algunos errores.
No se elige ninguna ablación ni se modifica la física audible o instalada.

Verificación independiente completada: SHA de fuentes e informes de
origen, estado reutilizado, igualdad exacta de referencias, todos los
vectores finitos y sus dimensiones, normas/energías/grillas, ledger de
trabajo, cotas y costes. Datos únicos en
`medidas-P6-puerto-dos-intervalos-2026-10-09/puerto.json` y `presion.npz`
(714.590 bytes; SHA256 `496ea68798d8ffb4581b33b3b3f58d5bf84d5dfead99738e90692366ef092b63`).
No hay procesos de medición activos, WAV nuevo ni duplicados de R/estado.

**Siguiente:** analizar la respuesta de fase del sistema lineal acoplado
en un banco pequeño frente a su ODE/matriz exacta y contrastar un puerto
de mayor orden. Validar trabajo, fase y coste antes de otro ensayo grande.
El control cuadrático identifica una limitación del integrador completo;
no demuestra que refinar sólo el aire dentro del splitting la resuelva.
Esa subdivisión aislada ya fue rechazada por empeorar la memoria.

Se conservan umbrales, fallos y gates globales falsos. Después siguen
duración/release y F4 para escucha; precisión acústica (9,2021 % frente
a 5 %), balance de radiación completo e identificación física pendientes.
El plugin instalado 0.172.0 y las escuchas de referencia se conservan.
