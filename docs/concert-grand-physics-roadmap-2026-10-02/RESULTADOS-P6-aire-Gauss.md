# P6 — Dos etapas Gauss sólo en el aire

Fecha: 2026-10-10. Commit previo `d135f60`. Sigue al
[diagnóstico de fase y dispersión](RESULTADOS-P6-fase-lineal.md).
Se implementa la rama compleja propuesta sin cambiar las portadoras
mecánicas, los coeficientes físicos ni los estados vivos del prototipo.

## Implementación y trabajo

`tools/concert_grand_gauss_air.py` resuelve la pareja de etapas a través
de `a=1/4+i*sqrt(3)/12` y `w=1/2-i*sqrt(3)/2`. Una cascada compleja
de las mismas secciones Padé calcula los estados de rama y sus offsets.
Las fuerzas medias y los estados finales reales son:

```
force_avg = 2*Re(w*force_branch)
state_new = state_old + 2*Re((w/a)*(state_branch-state_old))
```

Las etapas físicas se reconstruyen con
`t_i=(c_i-conj(a))/(a-conj(a))`, `c_i=1/2 +/- sqrt(3)/6`:
`stage_i=2*Re(t_i*branch)`. La relajación paralela usa los mismos a/w.
Su fuerza de entrada contiene `B*2*Re(w/(1+alpha))*B.T`,
`alpha=h*a/tau`, y su constante incluye el estado vivo anterior.

La fuerza media se promedia **antes** de contraer los residuos reales.
Así se conserva un único GEMV real por paso sobre el layout compartido;
no se convierte la R a compleja, no se hace otra R en disco ni se recorta
una sección o un modo. Los offsets/estados auxiliares sí son complejos.

`tools/concert_grand_port_gauss.py` conserva las dos mitades de portadoras
y el gradiente geométrico completo del método anterior. La velocidad del
puerto permanece constante durante el paso y es el desplazamiento total
dividido por h. La fuerza media hace el mismo trabajo en mecánica y aire.
Se conserva el sistema de eliminación del puerto, sin invertir respuestas
cerca de sus ceros y con todos los modos. No está integrado al renderer.

Radiación: dos paquetes reales separados, uno por etapa, cuadratura
positiva con pesos 1/2. `wave_inputs_stages` tiene dimensión adicional
de dos etapas; no se devuelve como el paquete único del renderer anterior.
Con cálculo diferido se acumula la pérdida de relajación y se conservan
ambos paquetes para calcular después su radiación. No reiniciar memoria.

## Pruebas antes del ensayo grande

Cuatro pruebas nuevas pasan:

1. Cascada compleja frente a resolución real directa de las dos etapas,
   con memoria inicial no nula, tasas 16/32/128/512 kHz: estados, fuerza,
   paquetes, almacenamiento acústico, pérdidas y entradas intactas.
2. Respuesta armónica de las ecuaciones discretas vivas frente a la fórmula
   de dos resolventes continuos, en tres frecuencias/tasas independientes.
3. Energía no lineal + almacenamiento acústico + pérdidas en el sistema
   pequeño; equivalencia exacta de estados con radiación directa/diferida.
4. Convergencia al ODE no lineal independiente, tres tasas, reducción de
   error mayor que 3× y error fino menor que 0,1 %.

La primera ejecución de las pruebas detectó una inversión incorrecta del
factor de relajación (`alpha/(1+alpha)` en lugar de `1/(1+alpha)`). Fallaron
fuerza, trabajo y convergencia. Se corrigió la implementación; las cuatro
pruebas pasan. El ensayo grande empezó después de esa corrección.
No se relajaron umbrales ni se eligió el test energético como prueba de timbre.

## Trayectoria real del prototipo

`tools/measure-concert-grand-port-gauss.py` reutiliza el mismo estado
completo C4/84 de 2 ms, C4/G4/C5 con nueve cuerdas y todos los modos
96/192, tabla de 606 y aire de rango 192. No hay contacto nuevo, otra
R ni reseteo. Ocho milisegundos con apagadores abiertos, operador de
micrófono de 128 kHz y referencias físicas nativas 512 ya guardadas.

Replay de 128 kHz: ambos espectros exactamente iguales al anterior.
Ensayo **275,359 s (4,59 min)**. Controles de medición pasan a 6,25 Hz:
energía entre grillas máxima 0,057801 %, cambio absoluto de error
0,000008479 y oráculo Hann 4,861e-12. Balance mecánico/material/trabajo
máximo 4,800e-12 relativo. No es certificado de radiación total del banco.

Errores porcentuales Hann frente a referencia física nativa 512 kHz:

| Método / tasa | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz | Tiempo de 8 ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Splitting previo / 128 kHz | 0,000990 | 0,008477 | 0,052499 | 0,453207 | 27,771 s |
| Aire Gauss / 32 kHz | 0,018006 | 0,128371 | **2,281651** | **23,031604** | 12,872 s |
| Aire Gauss / 64 kHz | 0,004173 | 0,028869 | 0,525997 | **5,323172** | 23,755 s |
| Aire Gauss / 128 kHz | 0,000931 | 0,006650 | 0,124450 | **1,267961** | 52,803 s |
| Aire Gauss / 256 kHz | 0,000345 | 0,002415 | 0,035399 | 0,327450 | 85,413 s |

Las bandas rectangulares pasan (máximo 0,320526 %); conservar los fallos
Hann. La banda superior tiene poca energía, como antes: sus porcentajes
no se traducen directamente en una diferencia audible de esa magnitud.

Cotas porcentuales de extremos frente al estado fino de 512:

| Tasa | Mecánica | Onda | Relajación |
| --- | ---: | ---: | ---: |
| 32 kHz | 0,053845 | **10,081058** | **8,972287** |
| 64 kHz | 0,010307 | **5,155828** | **2,347126** |
| 128 kHz | 0,003785 | **2,199571** | 0,601450 |
| 256 kHz | 0,004400 | 0,635496 | 0,160315 |

Son cotas triangulares a través del replay 128 y su diferencia 128→512
anterior, no estados finos adicionales. Se verifica también la cota inversa
de onda: `max(0,e(actual,128)*(1-e(128,512))-e(128,512))`.
A 32/64/128 las cotas inferiores son **9,849486/4,934618/1,984579 %**.
El fallo de onda en esas tasas no es sólo una sobreestimación conservadora.

Sólo 256 kHz pasa todos los controles locales del tramo. Cuesta **3,076×**
el splitting anterior 128 de la misma ejecución. A 128 cuesta **1,901×**
y falla. Los tiempos no estiman una frase/pieza. **Rechazar este método
como reemplazo para acelerar el renderer**. El aire aislado se calcula
correctamente, pero esta combinación de discretizaciones no mejora la
precisión/coste del instrumento en estas condiciones.

## Respuesta armónica del mismo aire

La entrada del puerto es constante dentro de un intervalo. No se puede
usar la respuesta de Gauss a una sinusoide evaluada en sus dos nodos.
`tools/concert_grand_gauss_response.py` deriva la respuesta de los estados
discretos realmente usados. Si `s_hat=2i/h*tan(omega*h/2)`:

```
rho = sqrt(1-h*h*s_hat*s_hat/3)
p1 = 2*s_hat/(1+rho)
p2 = 6*(1+rho)/(h*h*s_hat)
Z_Gauss = (1+1/rho)/2*Z(p1) + (1-1/rho)/2*Z(p2)
load_Gauss = i*2/h*sin(omega*h/2)*Z_Gauss
```

Proviene de la resolución de la matriz generadora lineal con función
`(1-h²*s_hat*L/12)/(s_hat*(I+h²*L²/12)-L)`. La prueba de estados vivos
contrasta la fórmula directamente. No se corrige frecuencia ni ganancia.

`tools/measure-concert-grand-gauss-response.py` prepara el operador real
rank192/606 y reutiliza las matrices físicas continuas de su único caché
anterior. Doce comparaciones completadas en **62,891 s (1,05 min)**.
Error porcentual Frobenius de carga frente a continua a 128 kHz:

| Frecuencia | Midpoint anterior | Gauss con puerto constante |
| --- | ---: | ---: |
| 6 kHz | 7,908513 | 0,692769 |
| 10 kHz | 5,083967 | 2,184795 |
| 12 kHz | 3,107021 | 3,399377 |

Mejora clara en 6/10 kHz y empeora a 12; no es una mejora uniforme.
Reciprocidad máxima 1,037e-15 relativa. Las normas de operador no son
errores de presión ni fracciones del error del coast. **Una mejora del
operador armónico no implica una mejora del estado acoplado completo.**

Verificación independiente de fuentes, tabla/R, estados/referencias e
informes por SHA; replay exacto, todos los espectros/grillas/bandas, ledger,
cotas/speedups y normas armónicas. Sólo datos compactos nuevos:

- `medidas-P6-puerto-Gauss-2026-10-10/puerto.json` y `presion.npz`
  (714.641 bytes, SHA `56f358476158384c46a3cc6eaa0ecce559dda04bd40b8ae6952be79a8f5385ab`).
- `medidas-P6-respuesta-Gauss-2026-10-10/respuesta.json` y `normas.npz`
  (72.572 bytes, SHA `03f0857c8b1c3317d6a84af26c93ad14c5693f6bf53ade7bbc0427f4c6dcdbe9`).

El segundo archivo contiene energías por fila, no otro archivo de matrices
completas. No se duplica la R, el estado vivo, informes o audios en SHARED.
Todos los trabajadores terminaron. Plugin 0.172.0 y escuchas intactos.

## Siguiente: dos velocidades y fuerzas de puerto

Hipótesis concreta: mantener la misma velocidad media en ambas etapas
no representa suficientemente la variación dentro del paso de las
portadoras. La mejora armónica parcial y el empeoramiento de memoria
justifican probarla; **no prueban que sea la única causa del fallo**.

Probar puertos distintos en las dos mitades mecánicas, con sus propias
fuerzas `F1/F2` y gradientes `N1/N2`. En coordenadas modales:

```
qm = Ch*q0 + Sh*v0 - Qh*(N1+F1)
q1 = C*q0 + S*v0 - Qh*((1+2*Ch)*(N1+F1)+N2+F2)
v1 = -Omega²*S*q0 + C*v0 - Sh*((2*Ch-1)*(N1+F1)+N2+F2)
u1 = Cb*(qm_board-q0_board)/(h/2)
u2 = Cb*(q1_board-qm_board)/(h/2)
```

Las etapas del aire reciben u1/u2 y devuelven sus fuerzas individuales.
El trabajo mecánico de ambas mitades coincide con la cuadratura del
puerto acústico `h/2*(u1*load1+u2*load2)`; pérdidas materiales y radiación
también se calculan por etapa. No promediar las fuerzas antes de resolver
la mecánica ni tratar las dos entradas como una única pulsación.

La rama normalizada para dos velocidades reales se puede construir con
`u_branch=(1/2+sqrt(3)/4-i/4)*u1+(1/2-sqrt(3)/4+i/4)*u2`.
Las etapas se reconstruyen con los mismos t_i. Esa extensión requiere
contrastar primero la resolución real de Gauss con entradas distintas;
el plan implementado aquí admite sólo el puerto constante real.
La fuerza compleja puede exigir dos contracciones reales de residuos
y la eliminación mecánica pasa a dos bloques de tabla: coste pendiente.

Orden próximo: equivalencia de etapas distintas, trabajo/pasividad y ODE
independiente, coste, mismo estado C4/84 y sólo entonces selección local.
Los nodos de Gauss y las medias de las mitades no coinciden; no presumir
orden cuatro del integrador acoplado. Mantener los criterios y contrastarlo.
Después siguen duración/release, F4 y escucha; identificación y precisión
acústica espacial (9,2021 % frente a 5 %) aún pendientes. Gates globales falsos.
