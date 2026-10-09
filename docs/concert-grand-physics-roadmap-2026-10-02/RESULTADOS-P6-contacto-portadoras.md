# Portadoras durante contacto y precisión del release — 2026-10-09

Base congelada y versionada en `0841c55`: frase con cuerpo/resonancia
aprobados como dirección de escucha, nueve cuerdas sobre una tabla de 606
coordenadas y un único aire reducido. Se conserva el audio y su regulación.

## Hipótesis y construcción

Durante el contacto anterior, midpoint avanza el bloque cuadrático; las
portadoras exactas sólo se usaban con el martillo separado. El nuevo
`tools/concert_grand_exponential_contact.py` extiende esas portadoras al
contacto, conservando el potencial de cuerda/fieltro y la pérdida por tasa.

La base es la ecuación 5 de [Wang y Wu, AAVF](https://arxiv.org/html/1807.01044v1).
Su análisis conservativo no valida el fieltro disipativo, contacto unilateral
ni el aire de nuestro prototipo. Esa extensión se deriva y prueba aquí.

En coordenadas de masa unitaria y portadora diagonal:

```text
q1 = C*q0 + S*v0 - Q*g
v1 = -omega²*S*q0 + C*v0 - S*g
C = cos(omega*h), S = h*sinc(omega*h/pi)
Q = (h²/2)*sinc(omega*h/(2*pi))²
```

`g` contiene el gradiente discreto exacto del resto de cuerda y cada contacto
de fieltro. Los martillos tienen omega cero: C=1, S=h y Q=h²/2. El cambio
de energía cuadrática es `-g·(q1-q0)`; el trabajo elástico discreto lo
cancela y la parte por tasa deja calor no negativo. La tracción conserva
la ley anterior y su límite exponencial, sin ajustar parámetros por nota.

El Jacobiano de los contactos precondiciona el residuo mediante Woodbury.
El residuo completo incluye todas las coordenadas y todos los contactos:
la aproximación del Jacobiano no elimina fuerza ni acoplamientos.
Se componen medio paso mecánico, paso de aire y medio paso mecánico.
Ambos medios pasos registran calor de fieltro; la memoria acústica vive.

## Pruebas realizadas

Tres pruebas pasan en `tools/tests/test_concert_exponential_contact.py`:

- Trabajo mecánico con seis cuerdas desiguales y dos martillos, con pérdida
  por tasa nula y positiva: energía más calor cierra con error menor a 1e-9.
- Trayectoria con aire recíproco no nulo y reiteración: conserva cuerdas y
  estados acústicos al relanzar, y cierra todas las pérdidas del fixture.
- Convergencia frente a DOP853 de las ecuaciones continuas independientes
  de un fixture pequeño. El refinamiento 64/128/256 kHz reduce el error
  más de tres veces por duplicación, y el último error es menor a 1e-4.

Estas pruebas no equivalen al balance completo de la tabla/aire grandes
ni validan todas las alturas, velocidades y condiciones de contacto.

## Ensayo grande y condiciones

`tools/measure-concert-grand-exponential-contact.py` usa los nueve puertos,
96/192 modos por cuerda, tabla 606 y aire congelado de rango 192. Compara
ataque y reiteración de C4 a velocidad prescrita 84. El ensayo mide presión
aportada por el intervalo finito de fuente, incluida su cola retardada;
no incorpora toda la historia previa de presión del micrófono.

La referencia fría es midpoint a 1536 kHz, contrastada previamente contra
768 kHz. La reiteración compara midpoint 1536/3072 kHz antes de medir las
portadoras 64/128/256 kHz. El release recorre los 30 ms completos de descenso
en un intervalo de 40 ms, con coast a 32/64/128 kHz.

Todas las variantes parten del mismo estado vivo a 40 ms: ataque a 1536
kHz y 38 ms de coast a 32 kHz, con apagadores abiertos. Esa historia común
no se declara convergida. Se conserva una cápsula pequeña en `local/`
para repetir esta condición sin reconstruir su trayectoria; está ligada
a hashes de fuentes, tabla, R y base acústica. No duplica el factor R.

Informe: `medidas-P6-contacto-portadoras-2026-10-09/contacto-release.json`.
El ensayo terminó en **983,94 s (16,40 min)**. Se verificaron los hashes de
fuentes, tabla y cápsula; ninguna variante falló en la resolución no lineal.
La cápsula común ocupa 2.069.446 bytes y se conserva sólo en `local/`.
No hay selección para escucha ni plugin; el certificado global sigue falso.

## Resultados de contacto

Todos los errores son relativos a la referencia indicada en cada tramo.
Las velocidades corresponden únicamente a esos prefijos de 2 ms, no al
render completo. Se incluyen construcción por paso/cachés cuando el solver
la necesita. La salida de presión conserva la misma escala SI.

| Caso | Tasa nueva | Error presión | Error mecánico | Error onda | Aceleración frente a referencia |
| --- | ---: | ---: | ---: | ---: | ---: |
| Ataque, referencia midpoint 1536 kHz | 64 kHz | 0,029899 % | 0,034428 % | 0,377584 % | 23,51× |
| Ataque, referencia midpoint 1536 kHz | 128 kHz | 0,006962 % | 0,011024 % | 0,094559 % | 12,56× |
| Ataque, referencia midpoint 1536 kHz | 256 kHz | 0,003400 % | 0,001190 % | 0,022876 % | 6,82× |
| Reiteración, referencia midpoint 3072 kHz | 64 kHz | 0,011377 % | 0,029608 % | 0,297669 % | 33,50× |
| Reiteración, referencia midpoint 3072 kHz | 128 kHz | 0,002327 % | 0,013796 % | 0,075660 % | 18,87× |
| Reiteración, referencia midpoint 3072 kHz | 256 kHz | 0,000754 % | 0,011098 % | 0,022606 % | 9,66× |

La referencia de reiteración 1536/3072 kHz difiere 0,000851 % en presión,
0,019880 % en estado mecánico y 0,008435 % en onda. No es una solución
exacta continua; su incertidumbre es relevante al interpretar errores
muy pequeños. Esta cápsula previa usa coast a 32 kHz; los resultados no
se comparan directamente con el 1,2826 % del ensayo anterior, cuya cápsula
previa usaba coast a 16 kHz y cuya pareja de tasas era 768/1536 kHz.

El estado mecánico se compara con desplazamiento ponderado por frecuencia
de portadora y velocidad, excluyendo el vuelo de las cabezas. Onda y
relajación se miden separadamente; sus unidades no se mezclan. A 128 kHz,
la relajación difiere 0,044693 % en ataque y 0,022924 % en reiteración.
Los picos de fuerza conservan su orden: 13,54140 N en el ataque nuevo
frente a 13,54150 N de referencia; 13,40490 N en reiteración frente a
13,40510 N. Todos terminan separados a 2 ms. El instante exacto de primera
separación y la curva completa de fuerza no se registraron en este ensayo.

## Resultados de apagado

| Comparación coast | Error presión | Error mecánico | Error onda | Error relajación |
| --- | ---: | ---: | ---: | ---: |
| 32 → 64 kHz | 0,121396 % | 0,740032 % | 2,105158 % | 0,214066 % |
| 64 → 128 kHz | 0,024790 % | 0,053521 % | 0,526690 % | 0,044434 % |

Mirar sólo presión y mecánica habría aceptado 32 kHz bajo un criterio
local del 1 %, pero la onda conserva una diferencia del 2,105 %. Con
64/128 kHz todas esas normas locales quedan por debajo del 1 %. La pérdida
por apagador converge a 0,00215223 J. Esto no identifica el apagador real ni
certifica dos segundos de cola; el refinamiento sólo dura 40 ms.

## Decisión y continuación

La hipótesis de portadoras durante el contacto merece continuar: en C4 a
velocidad 84, 128 kHz conserva los estados y la presión con diferencias
pequeñas frente a referencias mucho más finas. Se conserva como candidato
numérico; no cambia la afinación, el fieltro ni la regulación para mejorar
el score. **La aceleración del ataque no se transfiere automáticamente al
coste total:** el sostenido, la carga aérea y los apagadores aún dominan
los renders largos.

Pendientes antes de una nueva selección audible:

1. Registrar fuerza/primera separación, probar también C5 y velocidades
   mayores; la validación de C4/84 no se extrapola a todo el teclado.
2. Integrar el contacto candidato con coast y apagadores, manteniendo la
   vibración al cambiar de solver; verificar sostenido/reiteración/release
   en una frase completa contra el control congelado.
3. Comprobar presión por bandas y no sólo su norma global, especialmente
   ataque/agudos; una diferencia pequeña total puede ocultar cambios de brillo.
4. Medir el coste completo, y continuar la reducción de coste del aire sin
   perder pasividad ni la respuesta de referencia. El rango 192 aún hereda
   el error de presión del 9,202 % de la reducción acústica anterior.
5. Incorporar F4 al banco común y producir una frase de cuatro alturas con
   esta física. No reemplazar las cuerdas vivas por transposición de muestras.

Los WAV aceptados permanecen como controles. Fuentes e informes se guardan
sólo en el repositorio; no se crea otra copia de audio idéntico en SHARED.
