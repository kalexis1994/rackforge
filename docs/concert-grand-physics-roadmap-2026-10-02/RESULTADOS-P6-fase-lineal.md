# P6 — Fase lineal y dispersión temporal del aire

Fecha: 2026-10-09. Sigue a los [dos intervalos del puerto](RESULTADOS-P6-puerto-dos-intervalos.md).
Commit previo: `9df8674`. Objetivo: contrastar una referencia continua
independiente y distinguir la dispersión del integrador de un cambio de física.

## Banco lineal pequeño

`tools/concert_grand_linear_phase.py` construye C4/G4/C5 con una cuerda
por tecla, 24 modos verticales, 24 horizontales y 48 axiales por cuerda,
tres modos de tabla (110/270/680 Hz), seis secciones Padé de rango dos y
seis estados de relajación. Mantiene los 291 grados mecánicos y la energía
acústica positiva completa; 612 estados lineales. Las cabezas están aparcadas
y se excluyen de esta aplicación homogénea sin contacto.

Las longitudes 0,62/0,51/0,31 m y el factor acústico sintético son controles,
no parámetros identificados de pianos. **Este banco no es una reducción
validada del prototipo grande**, ni reutiliza su estado vivo o cápsula.

El generador continuo se construye de la rigidez, amortiguamiento y
derivada original del aire, independientemente de los métodos discretos.
La métrica se transforma a energía euclídea, sin recortar autovalores.
Los bloques de portadoras se transforman por separado para conservar
sus ceros estructurales, incluso con autovalores degenerados.

La referencia es `exp(t*L)`, evaluada numéricamente y contrastada contra
DOP853 para el recorrido de 8 ms: diferencia **3,008e-11** relativa.
La identidad de potencia continua, comparada con pérdidas materiales,
relajación y radiación originales, tiene error máximo **5,748e-15**.

Se miden 68 modos cargados de 60–12000 Hz con participación mecánica
mayor o igual que 50 %. Para cada autovector continuo se guarda el factor
proyectado de un paso discreto, su fase/decaimiento y la fuga a otros
componentes; **esa fase proyectada no se presenta como un autovalor del
método discreto cuando hay mezcla**. También se guarda el error completo
del modo tras 8 ms y el de una excitación determinista de velocidad.
No se corrige fase, ganancia ni se genera audio.

## Control de orden cuatro

Se contrasta Gauss de dos etapas en los 180 estados del bloque acoplado.
Las 432 componentes de portadoras libres siguen exactas. Para `hL=dt*L`:

```
D = I - hL/2 + hL*hL/12
Y1 = D^-1*(I-sqrt(3)*hL/6)
Y2 = D^-1*(I+sqrt(3)*hL/6)
M = I + hL*(Y1+Y2)/2
```

Es un control lineal de integración, no una variante de piano. Sus etapas
permiten verificar `E1-E0 + h*(loss(Y1)+loss(Y2))/2 = 0`. Balance máximo
de recorrido **2,367e-14** relativo. Conservar energía/pasividad no garantiza
precisión de fase en una nota alta ni en estados rígidos del aire.

Referencia primaria: [Hairer, Geometric Numerical Integration, Lecture 2](https://www.unige.ch/~hairer/poly_geoint/week2.pdf),
sección de Runge–Kutta y conservación de invariantes cuadráticos. La identidad
disipativa aquí se deriva con la métrica del banco y se verifica frente a sus
pérdidas originales; no se presume conservación de un potencial no lineal.

## Resultados y rechazo

Medición completa: **3,048 s**, quince comparaciones. Todos los controles
de medición, linealidad y energía pasan. Splitting y puerto actuales pasan
los criterios de fase/estado del banco pequeño incluso a 32 kHz: errores
de la excitación **0,0000754/0,0000811 %** y de los modos como máximo
**0,0004685/0,0006033 %**. **El fallo del prototipo grande no se reproduce.**
Esos resultados no certifican su memoria de 649 secciones ni su tabla de 606.

Gauss en el bloque acoplado, porcentajes frente al flujo continuo:

| Tasa | Estado de excitación a 8 ms | Peor modo a 8 ms | Peor frecuencia proyectada |
| --- | ---: | ---: | ---: |
| 32 kHz | 69,697340 | 199,895750 | 2,384092 |
| 64 kHz | 37,604291 | 101,453169 | 0,188896 |
| 128 kHz | 8,184246 | 7,032709 | 0,012488 |
| 256 kHz | 0,534606 | 0,445679 | 0,000791 |
| 512 kHz | 0,033666 | 0,027949 | 0,000050 |

Los umbrales registrados antes del ensayo son 1 % de estados/modos y
0,01 % de frecuencia proyectada. Gauss pasa sólo a 256/512 en este banco;
se rechaza como reemplazo global de las portadoras acopladas actuales.
Los tiempos de construcción de matrices no son speedups de un renderer.

Tres pruebas nuevas pasan: energía/RHS/linealidad y contraste continuo;
trabajo y orden asintótico de Gauss con portadoras libres exactas; identidad
de frecuencia midpoint frente a los estados discretos vivos de un aire
pequeño. Al principio se detectó mezcla numérica por raíz global de K:
se corrigió por bloques. La prueba inicial de orden tampoco estaba en el
régimen asintótico de los polos acústicos; se comprueba ahora con pasos
que cumplen `h*|pole|<1`. Eso no autoriza las tasas gruesas, que se miden
y conservan sus fallos. No se relajaron los umbrales de precisión.

Datos únicos: `medidas-P6-fase-lineal-2026-10-09/fase.json` y
`operadores.npz` (4.211.434 bytes), con generador, matrices, modos y extremos.
Verificación independiente de hashes, flujo, residuos de autovectores,
fase/fuga, estados, cotas energéticas y ledger. Productores previos intactos.

## Operador acústico real

Para no extrapolar el banco pequeño, `tools/measure-concert-grand-air-warp.py`
reconstruye **el mismo operador acústico reducido de rango 192 del prototipo
grande**, sobre sus 606 coordenadas de tabla y 649 secciones. Verifica las
fuentes y la única R de origen. No calcula contacto, no integra otra
trayectoria ni crea un estado o una R adicional.

En una señal armónica, midpoint evalúa el operador continuo en
`s_hat = i*2/h*tan(omega*h/2)`. El puerto de desplazamiento suministra además
`omega_disp = 2/h*sin(omega*h/2)`. Se comparan las matrices:

```
Z_cont = Z(i*omega)
Z_step = Z(i*2/h*tan(omega*h/2))
load_cont = i*omega*Z_cont
load_step = i*2/h*sin(omega*h/2)*Z_step
```

La fórmula se contrasta en una prueba con matrices de transición/fuerza
de los estados vivos, sin asumir simplemente que el mapeo es correcto.
Se incluye explícitamente la relajación paralela preservada. Son normas
Frobenius de operadores, **no errores de micrófono, respuesta cargada,
polifonía ni fracciones del error de la trayectoria anterior**.

Ensayo real completo **82,114 s (1,37 min)**, ocho frecuencias y cuatro tasas.
Error porcentual de la matriz de carga de desplazamiento frente a continua:

| Frecuencia | 32 kHz | 64 kHz | 128 kHz | 256 kHz |
| --- | ---: | ---: | ---: | ---: |
| 2,5 kHz | 14,805878 | 3,693286 | 0,920529 | 0,229924 |
| 6 kHz | 63,953153 | 29,401033 | 7,908513 | 1,979488 |
| 10 kHz | 37,292434 | 17,074796 | 5,083967 | 1,328854 |
| 12 kHz | 48,036301 | 11,143764 | 3,107021 | 0,813392 |

A 128 kHz, la frecuencia evaluada cambia 0,729197 % a 6 kHz y 2,057569 %
a 10 kHz. La respuesta del operador varía más que esas frecuencias:
la impedancia cambia 7,921615/4,769614 %, respectivamente. Esto demuestra
dispersión temporal medible del operador real; no demuestra por sí solo
que sea toda la causa del error alto del sonido. La precisión acústica
espacial frente al operador completo sigue fallando 9,2021 % frente a 5 %.

Fuentes/tabla/R verificadas por SHA y normas de las 32 comparaciones
recalculadas independientemente. Reciprocidad máxima 1,069e-15 relativa.
Informe único en `medidas-P6-dispersion-aire-2026-10-09/aire.json`.
Las matrices de estas frecuencias están sólo en el caché ignorado
`local/concert-grand-physics/P6-dispersion-aire-2026-10-09/operadores.npz`
(225.639.645 bytes), para reutilizar el operador sin volver a preparar la
proyección en la próxima prueba. No son otra R ni otra cápsula; no se
copian a docs o SHARED. El banco pequeño guarda sólo 4,2 MB de evidencia.

## Próxima implementación concreta

**Rechazar Gauss sobre todas las portadoras acopladas.** Probar en cambio
dos etapas Gauss sólo para la evolución del aire, conservando el avance
mecánico exacto cuadrático, gradientes geométricos y un único trabajo de
puerto. No refinar el aire aislado dentro del splitting rechazado.

Derivación propuesta para una velocidad constante u del puerto durante
el paso: la matriz Runge–Kutta tiene autovalores
`a = 1/4 + i*sqrt(3)/12` y su conjugado. El peso de la rama es
`w = 1/2 - i*sqrt(3)/2`. Una resolución compleja del aire en `s=1/(h*a)`
y su conjugada permite construir fuerza media real y estados finales:

```
force_avg = 2*Re(w*force_branch)
state_new = state_old + 2*Re((w/a)*(state_branch-state_old))
```

En las secciones Padé, la rama resuelve `I-h*a*J` y la cascada sigue
siendo una recurrencia por secciones. La relajación usa el mismo a.
Las dos fuerzas/estados de etapa se reconstruyen para mantener **dos
paquetes de radiación independientes** y su cuadratura positiva; no
colapsarlos como si fueran un único paquete. Así el trabajo acústico
es `h*u*force_avg` y puede emparejarse con el desplazamiento completo
del puerto mecánico. Esta derivación es una hipótesis de implementación,
**todavía no código medido ni una mejora aceptada**.

Próximos controles, en orden:

1. Equivalencia de la resolución compleja con Gauss real de dos etapas
   en banco pequeño, estado y fuerza, memoria no nula.
2. Energía acústica + pérdidas de ambas etapas y trabajo de puerto;
   acoplamiento con portadoras y residuo no lineal sin mutar entradas.
3. Impedancia/fase y coste frente a midpoint, luego el mismo coast C4/84
   de 8 ms autenticado, sin recalcular el ataque ni otra R.
4. Seleccionar sólo si mejora precisión/coste; después duración/release,
   F4 y escucha. No compensar esta dispersión cambiando el fieltro.

Las ramas complejas podrían exigir dos contracciones reales de residuos;
su coste está pendiente. Tampoco se presume que el integrador acoplado
completo tenga orden cuatro: la velocidad media constante y el gradiente
mecánico tienen sus propios errores. Todos los gates globales permanecen
falsos. Plugin 0.172.0 y audios existentes intactos; no hay trabajadores activos.
