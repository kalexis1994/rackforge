# P6 — Paso conjunto con portadoras y puerto de desplazamiento

Fecha: 2026-10-09. Sigue el
[control cuadrático y subdivisión del aire](RESULTADOS-P6-coast-cuadratico.md).
Refinar el aire dentro del splitting empeoró su memoria. Este ensayo
mantiene la física completa y cambia sólo el integrador del coast.

## Método y trabajo del puerto

`tools/concert_grand_port_avf_coast.py` conserva todos los modos, el
residuo geométrico no lineal, masas, rigidez, pérdidas y estados acústicos.
Sólo se usa con cabezas separadas; un recontacto requiere el solver de
contacto. No tiene selector de fábrica ni está integrado al renderer.

En coordenadas de portadoras, `C=cos(Omega*h)`, `S=sin(Omega*h)/Omega`
y `Q=(1-C)/Omega²`, evaluados con sinc para evitar cancelación. Para
el gradiente no lineal discreto `gN` y la carga constante del paso `F`:

```
q1 = C*q0 + S*v0 - Q*(gN + F)
v1 = -Omega²*S*q0 + C*v0 - S*(gN + F)
u  = Cb*(q1_board-q0_board)/h
F_board = Db*(q1_board-q0_board)/h + Cb.T*(A*u + constant)
```

El AVF retiene el trabajo del potencial no lineal entre extremos; el
puerto acústico y la pérdida de tabla usan el mismo desplazamiento medio.
El aire mantiene su midpoint pasivo, sin reinicio o truncamiento de
memoria. No se alterna un coast mecánico con un impulso de aire.

Se elimina el acoplamiento lineal del puerto mediante un sistema de
606 coordenadas `I+Qbb*E/h`, con `E=Db+Cb.T*A*Cb`. La respuesta Qbb
incluye todas las portadoras verticales/tabla; no se invierte Qbb ni se
recorta un modo cerca de un cero de su respuesta. El resto de la
iteración conserva el gradiente no lineal completo. El método preserva
trabajo, pero eso no presupone precisión suficiente en frecuencia/fase.

## Pruebas antes del banco grande

Dos pruebas nuevas pasan en un sistema pequeño con geometría no lineal:

- Energía mecánica + almacenamiento acústico + calor/radiación, error
  menor que 1e-9; identidad del trabajo del puerto usando el desplazamiento,
  ecuación midpoint de los estados de aire y ausencia de mutación de entrada.
- Contraste contra ODE continuo independiente DOP853: tasas 64/128/256 kHz,
  errores de estado que disminuyen más de 3× por refinamiento y error fino
  menor que 0,1 %. No es una referencia de un piano identificado.

La primera etapa añade otras dos pruebas de controles; **cuatro pruebas
nuevas pasan** en total. No certifican el sistema grande por sí solas.

## Ensayo físico local

`tools/measure-concert-grand-port-avf-coast.py` reutiliza el estado completo
C4/84 de 2 ms ya autenticado, sin repetir el ataque. C4/G4/C5, nueve
cuerdas, 606 coordenadas de tabla y aire de rango 192; apagadores abiertos.
Ocho milisegundos de coast con las ecuaciones y coeficientes físicos
anteriores, sin ablaciones. Misma fuente Hermite y micrófono de 128 kHz.

Referencia de presión: los espectros físicos nativos de 512 kHz ya
medidos. El replay de 128 kHz reproduce **exactamente** los vectores de
presión previos, rectangular y Hann. Las comparaciones no corrigen fase
ni ganancia y conservan los umbrales originales.

No se dispone de otro estado final 512 en disco. Se conserva el control
128 recalculado y se usa la cota triangular de las normas de extremos:

```
e(candidato,512) <= e(candidato,128)*(1+e(128,512)) + e(128,512)
```

La segunda diferencia proviene de la evidencia previa, con la misma
métrica y estado inicial. La cota es conservadora; no se presenta como
error exacto contra el estado de 512. Se mide mecánica en norma energética
del núcleo y onda/relajación en sus normas previamente fijadas.

## Resultados

Completado en **211,660 s (3,53 min)**, incluidos preparación y cinco
recorridos. Pasan todos los controles de medición a 6,25 Hz: oráculo máximo
4,660e-12 y variación máxima del error entre grillas 0,000013072 absoluto.
Balance mecánico máximo 4,647e-12 relativo; es un balance de mecánica,
calor material y trabajo del puerto, no certificado de radiación total
del prototipo grande. Los estados, fuentes y caché quedan intactos.

Errores porcentuales Hann frente a la presión física de 512 kHz:

| Integrador / tasa | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz | Tiempo de 8 ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Referencia anterior / 128 kHz | 0,000990 | 0,008477 | 0,052499 | 0,453207 | 30,058 s |
| Puerto conjunto / 32 kHz | 0,016477 | 0,168216 | **2,414063** | **31,778702** | 8,678 s |
| Puerto conjunto / 64 kHz | 0,003411 | 0,031258 | 0,631035 | **6,957089** | 15,024 s |
| Puerto conjunto / 128 kHz | 0,000840 | 0,007633 | 0,137280 | **1,824584** | 28,540 s |
| Puerto conjunto / 256 kHz | 0,000211 | 0,001750 | 0,033304 | 0,377727 | 50,997 s |

Todos pasan presión rectangular: error máximo 0,538928 %. Se preservan
los fallos Hann; no cambiar ventana para seleccionar la tasa. La banda
superior tiene poca energía, como en el ensayo previo; estos porcentajes
no predicen por sí solos una diferencia audible de esa magnitud.

Cotas porcentuales de extremos respecto a 512 kHz:

| Puerto conjunto | Mecánica | Onda | Relajación | Gate local completo |
| --- | ---: | ---: | ---: | --- |
| 32 kHz | **1,266027** | **1,775508** | 0,191311 | Falla |
| 64 kHz | 0,052152 | 0,439858 | 0,052414 | Falla presión Hann |
| 128 kHz | 0,009539 | 0,107597 | 0,018685 | Falla presión Hann |
| 256 kHz | 0,003262 | 0,189812 | 0,026017 | Pasa sólo este tramo |

El límite local de presión/estados es 1 %. El candidato a 256 kHz pasa
estos controles, pero tarda **1,697× más** que la referencia anterior de
128 kHz medida en la misma ejecución. Las tasas rápidas fallan. El paso
conjunto no entrega un reemplazo con mejor relación precisión/coste.
Los tiempos se limitan a estos 8 ms y no estiman dos segundos o una pieza.

## Verificación, decisión y siguiente hipótesis

Verificación independiente completada: hashes de fuentes, tabla, R,
estado e informes de origen; replay exacto de las referencias, espectros
finitos y dimensiones, normas/energías de todas las bandas, grillas,
cotas de extremos y speedups. Datos únicos: `puerto.json` y `presion.npz`
(714.513 bytes) en `medidas-P6-puerto-acoplado-2026-10-09/`. Ningún
trabajador queda activo. No se crean WAVs ni duplicados de R/estado.

**Decisión:** conservar el integrador como experimento verificado de
trabajo acoplado; rechazar su selección para acelerar el renderer.
La física y el backend de las escuchas existentes se mantienen.
Conservar todos los fallos, los umbrales y `all_gates_passed=false`.

La siguiente hipótesis debe separar el error del residuo no lineal en
este integrador del de su puerto: comparar su versión cuadrática a las
mismas tasas contra la referencia cuadrática ya medida. El método actual
evalúa un AVF en el tramo completo; el splitting anterior usaba dos
mitades mecánicas. Esa diferencia de resolución es una hipótesis concreta,
**no una causa demostrada** de la banda superior. Si el error se concentra
en ese residuo, contrastar una evaluación que siga las portadoras dentro
del paso, manteniendo trabajo y el mismo puerto; no corregir el timbre
ni elegir una ablación como piano.

Siguen pendientes duración y release completos, banco con F4 para escucha,
precisión acústica (9,2021 %, límite 5 %), balance de radiación completo e
identificación real. El plugin 0.172.0 conserva su versión. Ningún resultado
de ocho milisegundos certifica un instrumento polifónico completo.
