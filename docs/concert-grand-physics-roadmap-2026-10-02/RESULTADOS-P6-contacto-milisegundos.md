# P6 — Contacto y continuación de tres milisegundos

Fecha: 2026-10-06. Se continúa el
[observador acotado validado en prefijos](RESULTADOS-P6-observador-acotado.md).
La primera ejecución usa el auditor de **af78ecb**. La reutilización de
inputs del observador y el refinamiento acotado quedan en **ea06e59**;
no cambian la construcción física de la carga ni las ecuaciones del contacto.
El ensayo pertenece al prototipo SI; no cambia programas, audio ni paquete.

## Hipótesis y alcance

Los prefijos anteriores terminan demasiado pronto para observar separación
del martillo y flujo de onda apreciable. Se alargan, conservando las mismas
ecuaciones, geometría supuesta, pérdidas mutuas y memoria acústica completa.
No se identifica una propiedad nueva del Steinway ni se altera el fieltro.

- 606 coordenadas de tabla, 787.194 coordenadas acústicas, misma R de 1,91 GB.
- Misma cuerda C4 supuesta y velocidad incidente 5 m/s, sin reiniciar memoria.
- Tramo frío de 1,5 ms a 192/384/768 kHz. Cada continuación de otros 1,5 ms
  parte del **mismo extremo frío de 768 kHz**. No relanza el martillo.
- 288/576/1152 pasos por tramo; 4032 pasos totales en los seis casos.
- El recorrido fino frío + continuación es un golpe seguido durante 3 ms.
  Los recorridos gruesos continuados parten del extremo fino, para comparar
  la integración desde condiciones iniciales iguales; no son la prolongación
  de sus respectivos estados fríos gruesos.
- Fuerza constitutiva y trayectoria se observan en tiempos comunes a 192 kHz.
  Se conserva también el máximo de fuerza AVF promediada por intervalo.

Se conservan las tolerancias declaradas de energía, interfaz, fuerza,
trayectoria y observación. El refinamiento de almacenamiento, flujo, pérdidas
y trabajo usa el mismo **0,2 %** y piso absoluto **1e−14 J** que la etapa anterior.
Cuando hay intervalos de observación, el gate usa su diferencia máxima y
la menor magnitud de referencia admisible, con ese piso. También conserva
la diferencia de los valores centrales como diagnóstico.

La separación registrada sólo encierra el primer cambio de signo de la
compresión entre dos pasos; no interpola el evento ni certifica una raíz
continua. Sus tiempos se suman a 1,5 ms si ocurre en la continuación.
El control automático `cold_hammer_separated` del auditor se refiere al
primer tramo únicamente; la separación del golpe puede ocurrir después.
Se mantiene ese dato original, con su alcance explícito.

## Separación observada

Los tres tramos fríos terminan todavía en contacto. La primera separación
aparece en las tres continuaciones. Tiempo desde el golpe inicial:

| Tasa | Intervalo observado, ms | Ancho, µs |
| --- | ---: | ---: |
| 192 kHz | [1,630208333; 1,635416667] | 5,208333 |
| 384 kHz | [1,632812500; 1,635416667] | 2,604167 |
| 768 kHz | [1,634114583; 1,635416667] | 1,302083 |

Los intervalos registrados se anidan y su ancho se reduce a la mitad.
No es una medida del contacto real del Steinway: la cuerda y el fieltro
del banco siguen siendo supuestos.

La reconstrucción desde puertos reproduce exactamente los seis extremos
acústicos. Las historias ocupan **19.547.136 bytes**, frente a
12.705.638.400 bytes de inputs completos. La base de observación elige
**241 columnas**, con cola relativa de datos 9,647630e−13; la dinámica
permanece completa. La selección de base y el cierre de memoria por sí
solos no aprueban sus cotas energéticas ni el flujo no nulo.

## Comprobación del refinamiento acotado

Dos pruebas dirigidas pasan: valores centrales iguales no ocultan intervalos
amplios; se rechaza un intervalo que no corresponde a la energía registrada.
El piso de flujo casi nulo sigue siendo absoluto y el trabajo negativo
conserva su escala. No se introduce una tolerancia nueva para aceptar resultados.

## Coste detectado y repetición preparada

[Primera ejecución incompleta](medidas-P6-contacto-milisegundos/contacto-1p5ms-replay-incompleto.json):
se preservan construcción y los seis contactos, pero se detiene antes de
las medidas independientes del campo. El informe conserva su etapa incompleta
y `all_gates_passed=false`; sus 609,660 s corresponden al último checkpoint,
no al tiempo total hasta detener el proceso. No se utiliza para aprobar energía.

La recurrencia de observación repetía 4032 pasos por cada uno de los 384
bloques: **1.548.288 actualizaciones**. Una prueba de coste local, 20 updates
de 649 secciones y 241 columnas a 768 kHz, BLAS de un hilo, midió una media
de 0,009281 s por update. Sólo extrapolar esa repetición da unas cuatro horas.
La prueba convivió con el auditor: es un diagnóstico de coste, no una duración
medida de un certificado completo ni una promesa de velocidad del renderer.

La opción `--store-observer-inputs` guarda los inputs proyectados una sola
vez en RAM, durante la pasada del residuo completo. Los bloques vuelven a
leer esos mismos inputs. No crea otra R ni guarda historias en disco.
Antes de escoger Q, el auditor presupuesta el peor rango posible (606):
17.262.376.256 bytes de arrays del certificado, frente al máximo explícito
de 24 GiB. Es un presupuesto de arrays, no un límite del working set del SO.

Una cuarta prueba del observador comprueba **igualdad exacta** de energía,
flujo, intervalos y replay al reutilizar los inputs frente a reconstruirlos
por bloque, con memoria viva. Pasan las cuatro pruebas de campo y las dos
de refinamiento. La repetición completa usa esa opción con lotes de 256;
debe medir allí su precisión, coste y memoria efectivos.

La repetición reproduce **exactamente** los 66 diagnósticos mecánicos
comparados y las 1734 observaciones de fuerza en tiempos comunes de la
pasada detenida. Son 11 diagnósticos y una curva de 289 puntos por caso.
Se conserva su separación observada y vuelve a seleccionar rango 241.
Guardar los inputs es una opción de observación, no una reducción nueva
de la mecánica ni de las fuerzas acústicas.

## Resultado completo y fallo localizado

[Contacto completo de esta duración](medidas-P6-contacto-milisegundos/contacto-1p5ms-inputs.json)
y [refinamiento independiente](medidas-P6-contacto-milisegundos/refinamiento-trabajo.json).
Pasan los **11 controles del contacto**: balance independiente, pérdidas
no negativas, interfaz, trayectoria/tabla/fuerza, sus dos razones de segundo
orden, replay, balance con cotas y precisión de observación.

| Medida máxima en seis casos | Resultado | Límite |
| --- | ---: | ---: |
| Balance total con extremos adversos | 2,414908e−11 | < 1e−9 |
| Balance del aire con extremos adversos | 1,864494e−11 | < 1e−9 |
| Cota relativa de observación, con piso 1e−14 J | 8,410185e−6 | < 1e−4 |
| Replay de memoria completa | 0 | < 1e−11 |
| Norma del factor regenerado frente a R[0] | 0 | < 1e−10 |

Las razones de refinamiento de mecánica/fuerza están entre 0,200008 y
0,200023. El par fino 384/768 kHz cambia la trayectoria < 0,003139 %, la
tabla < 0,017855 % y la curva de fuerza común < 0,004124 %.

Hay ahora flujo de onda con límite inferior claramente por encima del piso:
**1,318446e−12 J** frío y **3,533789e−8 J** continuado a 768 kHz. Las energías
finales de onda son 1,153657e−7/2,939801e−6 J. La radiación no se obtiene como
resto del balance. Las cotas siguen siendo analíticas para proyección con
salvaguardas numéricas; no son una prueba formal de redondeo por intervalos.

Tiempo **2428,924 s** (40,482 min); certificado **1840,744 s**; pico propio
**8,678902 GiB**. Los inputs proyectados ocupan realmente 5.052.902.400 bytes
en RAM y se liberan al terminar. No se extrapola ese coste a audio o tiempo
real. La ejecución lenta anterior está incompleta: no ofrece una medida
comparable de tiempo total con la que afirmar un factor de aceleración.

El refinamiento de siete términos energéticos pasa en la continuación;
máximo acotado **0,034568 %** (flujo de onda). En frío pasan seis de siete:
el flujo cambia **0,264677 %**, por encima del **0,2 %** original. Se conserva
el fallo y `finest_pair_work_passed=false`. Su energía es diminuta respecto
al golpe, pero no se ensancha el criterio ni se aprueba el refinamiento frío.

El siguiente control dirige el coste a ese tramo: 1,5 ms **sólo frío** a
384/768/1536 kHz, con la misma construcción, fuerzas, tolerancias y observación.
La opción y las fases explícitas quedan congeladas en **09f151d**. No vuelve
a calcular la continuación ya medida. Son condiciones iniciales frías
idénticas, tres tasas anidadas y una nueva referencia más fina para el flujo.

## Refinamiento frío completado

[Control frío 384/768/1536 kHz](medidas-P6-contacto-milisegundos/frio384-1536.json)
y [refinamiento con sus intervalos](medidas-P6-contacto-milisegundos/refinamiento-frio.json).
Pasan sus **11 controles** y los **siete términos de trabajo/almacenamiento**
del par 768/1536 kHz. El mayor cambio acotado es el flujo de onda:
**0,067444 %**, dentro del 0,2 % original. El fallo 0,264677 % del par
anterior permanece registrado; no se modifica su informe.

| Control frío | Resultado |
| --- | ---: |
| Balance total máximo con extremos adversos | 1,768863e−12 |
| Balance del aire máximo con extremos adversos | 4,668476e−13 |
| Máxima cota relativa de observación | 8,127075e−6 |
| Razón de refinamiento mecánico / fuerza | 0,200002 / 0,200006 |
| Flujo de onda más fino, límite inferior | 1,319314e−12 J |
| Replay completo / diferencia de norma de H | 0 / 0 |
| Rango de observación | 189 de 606 |

Los 22 diagnósticos mecánicos de los dos casos compartidos y sus 578
observaciones comunes de fuerza reproducen exactamente el ensayo de tres
milisegundos. Las cuatro medidas compartidas de almacenamiento/flujo,
con bases de observación diferentes (241/189), quedan mutuamente dentro
de sus intervalos; diferencia central máxima 5,823352e−22 J. No se altera
el fieltro, cuerda, geometría, amortiguamiento ni construcción de la carga.

Tiempo **2187,679 s** (36,461 min), certificado **1588,518 s**, pico propio
**7,400826 GiB**; inputs proyectados **3.962.649.600 bytes** en RAM. Las
fuentes coinciden con los SHA256 capturados al inicio. No quedan historias
en disco ni se genera audio o paquete.

La continuación está refinada a 384/768 kHz desde el extremo frío de
768 kHz del primer ensayo. El nuevo banco refina sólo los primeros 1,5 ms.
**No se construye ni se declara un recorrido uniforme de 3 ms a 1536 kHz**
mezclando esos estados. Son controles de ambos tramos desde sus condiciones
iniciales explícitas. El primer recorrido fino de 3 ms sigue siendo el de
768 kHz, con separación observada cerca de 1,635 ms y flujo no nulo.

## Siguiente etapa

1. Propagación después de la separación y presión temporal de los dos
   micrófonos sobre esta referencia completa. Medir y mejorar el coste
   antes de extenderla a una nota audible; el paso mecánico sigue siendo caro.
2. Segunda pulsación natural desde el sostenido y apagador/release con sus
   reacciones, pérdidas y memoria viva. La continuación medida aquí no es
   una nueva pulsación.
3. Cobertura de otras notas/intensidades e identificación real; unísono,
   caras/borde y mecanismos pendientes del plan. No se aprueba el timbre
   ni se integra automáticamente el prototipo al plugin.

La puerta general permanece falsa en todos los informes. La aceptación
se limita a los contactos, balances, cotas y refinamientos descritos.

## Reproducción

Python/NumPy/SciPy; BLAS de un hilo y una ruta nueva para cada informe:

```powershell
python tools/audit-concert-grand-pade-contact-full.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --duration 0.0015 --rates 192000 384000 768000 --compact-certificate --bounded-observer --store-observer-inputs --certificate-input-block 256 --max-array-gib 24 --report NUEVO_CONTACTO.json
python -m unittest discover -s tools/tests -p test_concert_bounded_observer.py -v
python -m unittest discover -s tools/tests -p test_concert_work_intervals.py -v
python tools/audit-concert-grand-prefix-work-refinement.py --prefix-report NUEVO_CONTACTO.json --report NUEVO_REFINAMIENTO.json
python tools/audit-concert-grand-pade-contact-full.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --duration 0.0015 --rates 384000 768000 1536000 --cold-only --compact-certificate --bounded-observer --store-observer-inputs --certificate-input-block 256 --max-array-gib 24 --report NUEVO_FRIO.json
python tools/audit-concert-grand-prefix-work-refinement.py --prefix-report NUEVO_FRIO.json --report NUEVO_REFINAMIENTO_FRIO.json
```

Los informes permanecen sólo en el repo. No se guardan estados/historias en
disco ni se duplican factores acústicos. Separación y balance del golpe no
aprueban por sí solos la presión temporal, repetición natural, release,
unísono, geometría real, timbre ni integración de fábrica.
