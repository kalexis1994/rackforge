# P6 — Fuerza, separación y contacto en C4/C5

Fecha: 2026-10-09. Etapa anterior guardada en `ade6b7e`.

## Hipótesis y diseño

La aceleración del contacto sólo se había contrastado en C4/84. Se mantiene
la misma física y se comprueba C5/124, C4/124 y C5/84: un golpe fuerte y una
cuerda más corta pueden exigir más resolución aun con portadoras exactas.
No se cambia el fieltro, la afinación ni el número de modos para ajustar el
resultado. Se conservan tres teclas, nueve cuerdas 96/192, tabla de 606
coordenadas y el aire reducido congelado de rango 192.

El observador registra la fuerza continua unilateral en los estados de cada
paso: `k max(delta, 0)^p exp(clip(rate * delta_dot, -40, 40))`.
Esta fuerza instantánea no es la fuerza media de trabajo que devuelve el
solver. Se comparan su curva a 128 kHz, máximo nativo e impulso trapezoidal
nativo por cuerda. El máximo de la suma se calcula antes de maximizar; no se
suman máximos individuales tomados en distintos instantes.

La primera separación se guarda como intervalo entre dos estados nativos
que pasan de compresión positiva a no positiva, por cuerda. No se reporta
un instante exacto interpolado ni se interpreta el cero inicial como una
separación. Se cuentan recontactos observados. El refinamiento sigue siendo
necesario para detectar contactos completos dentro de un paso grueso.

Dos pruebas del observador pasan: signos/separación/recontacto individual;
fuerza dependiente de velocidad, impulso, interpolación y ausencia de
mutación de estados. Las tres pruebas anteriores del integrador vuelven a
pasar antes del commit.

## Protocolo de comparación

- Ataques de 2 ms desde reposo. C5/124 compara midpoint 1536/3072 kHz y
  portadoras 128/256/512 kHz. C4/124 y C5/84 comparan portadoras 128/256
  contra midpoint 1536 kHz; esas dos referencias no se refinan aquí.
- Todas las comparaciones incluyen presión del intervalo fuente finito,
  estado mecánico ponderado por frecuencia, onda aérea y relajación. El
  control local del 1 % se aplica también a curva de fuerza e impulso.
- Transferencia del estado separado de C5/124: continuar 2 ms con contacto
  o pasar a coast, ambos a 128 kHz y con idénticos estados iniciales vivos.
  Se exige equivalencia de los cuatro arrays bajo 1e-8.
- Desde ese estado, primeros 8 ms de descenso del apagador a 64/128 kHz.
  La rampa dura 30 ms: este ensayo no valida su final ni la cola completa.

Los vectores pequeños de fuerza y los resultados se guardan una sola vez
en [medidas](medidas-P6-contacto-registros-2026-10-09/contacto-registros.json).
No se generan WAVs ni otra copia del operador R o de estados cálidos.

## Resultados

Ensayo completado en **1074,376 s (17,91 min)**. Se verifican por separado
los hashes de los ocho productores, curvas finitas/no negativas, controles
locales y solapamiento de todos los intervalos de separación comparados.
El informe ocupa 309.225 bytes; no quedan trabajadores en ejecución.

### Ataque a 128 kHz

Errores relativos respecto a la referencia indicada, expresados en %:

| Caso | Presión | Mecánica | Onda aérea | Relajación | Curva fuerza | Impulso |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| C5/124 vs midpoint 3072 kHz | 0,007094 | 0,015106 | 0,096515 | 0,027083 | 0,003259 | 0,002714 |
| C4/124 vs midpoint 1536 kHz | 0,007093 | 0,018697 | 0,105577 | 0,037359 | 0,003655 | 0,002502 |
| C5/84 vs midpoint 1536 kHz | 0,005717 | 0,007737 | 0,047691 | 0,017758 | 0,001792 | 0,001853 |

Los tres casos pasan el criterio local del 1 %. El contacto nuevo tarda
14,102 / 12,230 / 12,664 s, respectivamente, frente a 293,639 / 141,269 /
148,275 s de las referencias. **Estos cocientes sólo describen el ataque;
no son aceleraciones del render completo.**

El refinamiento midpoint 1536→3072 kHz de C5/124 cambia presión 0,001952 %,
mecánica 0,002932 %, onda 0,001587 %, relajación 0,002033 %, curva de fuerza
0,0000616 % e impulso 0,0000491 %. La referencia conserva incertidumbre
temporal; una diferencia menor que ese cambio no demuestra error absoluto
menor. No se refina aquí la referencia de C4/124 ni la de C5/84.

Los pasos nuevos de 256 kHz también pasan en los tres casos. C5/124 a
512 kHz pasa: presión 0,000624 %, mecánica 0,000991 %, onda 0,005618 %,
relajación 0,001744 %, curva 0,000199 % e impulso 0,000154 %. La mecánica
de C5/124 mejora poco entre 128/256 (0,015106→0,010325 %), aunque después
baja a 0,000991 %; no asumir una ley uniforme de error para cada componente.

### Fuerza y separación

| Caso de referencia | Máximo de fuerza total | Primera separación por cuerda |
| --- | ---: | ---: |
| C5/124, 3072 kHz | 30,394708 N | 1,211263–1,211589 ms |
| C4/124, 1536 kHz | 27,462177 N | 1,389323–1,389974 ms |
| C5/84, 1536 kHz | 15,428770 N | 1,444661–1,445313 ms |

Las tres cuerdas de cada caso caen en el mismo intervalo a esta resolución;
eso no prueba tiempos idénticos ni elimina sus diferencias de tensión.
A 128 kHz los intervalos son 1,210938–1,218750 / 1,382813–1,390625 /
1,437500–1,445313 ms, respectivamente. Todos contienen los intervalos de
referencia; sus puntos medios difieren como máximo 3,58 µs. Todos los
intervalos comparados a 256/512 también solapan su referencia. No aparecen
recontactos en ninguno de los registros nativos; no es una prueba universal
contra contactos que puedan quedar enteramente dentro de un paso.

### Sostenido y descenso inicial del apagador

Desde el mismo estado separado de C5/124, continuar 2 ms con contacto o
pasar a coast a 128 kHz cambia presión 8,51e-11 y estado mecánico 1,79e-10
en norma relativa. Los arrays q/v/onda/relajación difieren respectivamente
4,27e-13 / 7,84e-11 / 1,43e-11 / 4,53e-11. Pasa equivalencia bajo 1e-8,
sin reiniciar cuerdas, cabezas, tabla ni aire en la transferencia.

En los primeros 8 ms de descenso del apagador, 64→128 kHz cambia presión
**0,080089 %**, mecánica **0,313721 %**, onda **0,917074 %** y relajación
**0,070836 %**. Pasa el criterio local del 1 %, con poco margen en la onda.
Esto no valida el resto de la rampa de 30 ms ni una cola musical completa;
no elegir automáticamente 64 kHz para cualquier intensidad/duración.

## Alcance y continuación

Una norma global pequeña no demuestra brillo correcto ni parecido al
Steinway. La reducción aérea sigue fallando su umbral del 5 % por el error
heredado del 9,202 %. Permanecen pendientes la frase temporal completa,
comparación por bandas, coste total y ampliación del banco con F4.
Los WAVs aceptados y el plugin instalado permanecen como referencias.

El contacto a 128 kHz queda como candidato numérico para integrar con el
backend por lotes en la frase compartida. El siguiente control debe conservar
eventos/ganancia y comparar el recorrido completo con el productor congelado,
incluyendo reiteración y pedal; medir por bandas y registrar coste completo.
La transferencia breve ya pasa, pero no reemplaza ese control temporal.
Si el sostenido resulta demasiado costoso, perfilar y acelerar sus operadores
con equivalencia antes de repetir renders largos. Luego ampliar con F4.
