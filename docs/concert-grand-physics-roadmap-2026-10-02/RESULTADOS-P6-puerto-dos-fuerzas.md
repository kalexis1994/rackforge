# P6 — Dos fuerzas y velocidades distintas en el puerto

Fecha: 2026-10-10. Etapa anterior guardada en `170daa3f`.

## Hipótesis y alcance

El control anterior mejoró parcialmente la respuesta armónica del aire,
pero empeoró la memoria acoplada. Aquí se prueba si una única velocidad
media y una fuerza constante por paso explicaban ese fallo. Se conservan
las ecuaciones y los coeficientes físicos; cambia la integración temporal.
No se supone que ésta sea la única causa ni que pasar esta prueba acerque
automáticamente el timbre al Steinway.

Modelo de tres teclas C4/G4/C5 y nueve cuerdas, 4065 coordenadas mecánicas,
606 modos de tabla, aire reducido de rango 192 y 649 intervalos causales.
Todos los gradientes geométricos cúbicos/cuárticos siguen activos. Mismo
estado C4/84 tras 2 ms de contacto fino, mismos micrófonos, ganancia y
ventana de 8 ms. No se repite el ataque ni se duplica R al disco.

## Resolución y trabajo

Dos portadoras mecánicas exactas de medio paso reciben fuerzas distintas:

```
qm = Ch*q0 + Sh*v0 - Qh*(N1+F1)
q1 = C*q0 + S*v0 - Qh*((1+2*Ch)*(N1+F1)+N2+F2)
v1 = -Omega²*S*q0 + C*v0 - Sh*((2*Ch-1)*(N1+F1)+N2+F2)
u1 = Cb*(qm_board-q0_board)/(h/2)
u2 = Cb*(q1_board-qm_board)/(h/2)
```

Los gradientes AVF `N1/N2` satisfacen la diferencia exacta del potencial
no lineal en cada mitad. Cada fuerza hace trabajo contra su propio
desplazamiento. El aire recibe ambas velocidades en las dos etapas reales
de Gauss y devuelve cargas individuales. Trabajo común:
`h/2*(u1·load1 + u2·load2)`.

La rama compleja normalizada usa
`ub=(1/2+sqrt(3)/4-i/4)*u1+(1/2-sqrt(3)/4+i/4)*u2`.
Se reconstruyen las dos fuerzas y los dos paquetes radiativos reales,
sin promediar fuerzas antes de resolver la mecánica. La matriz R sigue
siendo real e inmutable; se contraen por separado partes real e imaginaria
de los offsets. No se crea una R compleja ni se reinicia memoria entre etapas.

Para eliminar el puerto, `d1=qm_board-q0_board`, `d2=q1_board-qm_board`,
`H=Qh` y `J=2*Ch*Qh` restringidos a la tabla:

```
d = dpred - [[H,0],[J,H]] * (E*d/(h/2) + external)
```

Se factoriza un sistema de 1212 coordenadas de tabla. No se invierte H
ni se divide por respuestas oscilatorias que pueden anularse. No se
factoriza el sistema de todas las cuerdas y la memoria causal.

La pasividad del aire se mide con la cuadratura de ambas etapas, incluidas
sus pérdidas de relajación y radiación. El disipador material se calcula
con ambas velocidades de medio paso. Los nodos de Gauss 0,2113/0,7887 y
las medias mecánicas 0,25/0,75 no coinciden: **no se atribuye orden cuatro
al integrador completo**.

## Controles independientes

Cuatro pruebas pasan (4,185 s):

- Etapas con entradas distintas frente a un sistema real de Gauss ensamblado
  a partir de la ecuación continua; memoria inicial no nula, fuerzas,
  estados, paquetes radiativos y trabajo a 16/32/128/512 kHz.
- Entradas iguales recuperan el plan acústico constante ya medido.
- Trabajo/pérdidas del sistema no lineal completo y equivalencia entre
  observación radiativa directa y diferida. No se mutan estados de entrada.
- Convergencia hacia DOP853 independiente de la ecuación no lineal continua.

Fuentes nuevas: `concert_grand_staged_gauss_air.py`,
`concert_grand_staged_port.py`, `measure-concert-grand-staged-port.py`,
`tests/test_concert_staged_port.py`. Las fuentes de controles previos
permanecen intactas.

## Medición en el modelo grande

Termina en **446,39 s (7,44 min)**. Evidencia en
`medidas-P6-puerto-dos-fuerzas-2026-10-10/puerto.json` y `presion.npz`.
Los criterios previos se conservan: 1 % por banda y en las cotas de estado,
reproducción del control a 128 kHz, cuadraturas refinadas y trabajo mecánico.

Todos los porcentajes de presión de la tabla son errores complejos en
6–12 kHz con ventana Hann frente a la referencia de 512 kHz. Las cotas
de estado se calculan mediante el control reproducido de 128 kHz y su
envolvente ya medida frente a 512. No se alinean fase o ganancia.
El costo es para los mismos 8 ms de trayectoria, incluyendo la preparación
de cada tasa dentro de `native_run`; no se extrapola una velocidad WASM.

| Método | Tasa, kHz | Error Hann alto | Cota onda | Cota relajación | Segundos | Control local |
|---|---:|---:|---:|---:|---:|---|
| Splitting actual | 128 | 0,453207 % | 0,105293 % | 0,014088 % | 26,919 | Pasa |
| Dos fuerzas | 32 | 5,816616 % | 1,442414 % | 1,208261 % | 21,702 | Falla |
| Dos fuerzas | 64 | 1,650428 % | 0,790616 % | 0,325862 % | 50,059 | Falla |
| Dos fuerzas | 128 | 0,334674 % | 0,407433 % | 0,092672 % | 96,770 | Pasa |
| Dos fuerzas | 256 | 0,083738 % | 0,237526 % | 0,037463 % | 178,433 | Pasa |

A 32 kHz la cota inferior de onda es **1,229015 %** y la de relajación
**1,179748 %**: el fallo de estado no se debe sólo a una cota superior
conservadora. A 64 pasan las cotas de estado, pero falla presión Hann alta.
Las bandas Hann inferiores pasan en todas las tasas (máximo **0,595209 %**
en 2,5–6 kHz a 32). Las bandas rectangulares también pasan: máximo
**0,099438 %**. La banda Hann alta tiene sólo **1,82526e−8** de la energía
de referencia de esta ventana; su error relativo no prueba un defecto audible.

Respecto al puerto Gauss anterior con una única velocidad, la mejora a
128 kHz es clara: Hann alto **1,267961 → 0,334674 %**, cota de onda
**2,199571 → 0,407433 %**. Es evidencia a favor de representar la
variación del puerto dentro del paso. La cota de onda del splitting actual
sigue siendo menor, y pasar el mismo control local no prueba mayor
fidelidad de una nota completa.

**Ninguna tasa que pasa es más rápida que el splitting actual a 128 kHz.**
La primera que pasa (128) cuesta **3,595×**; 256 cuesta **6,628×**.
Se conserva el candidato como control de integración, pero se rechaza
como aceleración del renderer. No seleccionarlo para fábrica ni para
los renders de escucha por este resultado.

### Verificación de evidencia

- Reproducción del control 128 exacta, error 0 en ambas ventanas.
- Trabajo mecánico relativo máximo **4,72997e−12** frente al límite 1e−9.
- Oráculo Hann independiente máximo **5,09902e−12**, límite 1e−10.
- Refinamiento de normas por cuadratura máximo **0,0578014 %**, límite 0,1 %.
- Cambio absoluto de error por cuadratura máximo **2,38558e−6**, límite 1e−4.
- Hashes de fuentes, tabla, única R y estado inicial verificados nuevamente;
  arrays finitos, normas espectrales y cotas superior/inferior recalculadas
  independientemente del productor. Balance de trabajo y costos reproducidos
  desde sus componentes registrados. No se guardan otros estados finales.

Archivo espectral: **714.671 bytes**, SHA256
`e8d5c2a46360010092cb006eef4632bcc22b54f9961b0ce0b27bf90adaee8679`.

## Siguiente control: duración y release con escucha

Se cierra esta hipótesis: variar las dos fuerzas mejora este control,
pero no resuelve el costo. No encadenar otros cambios de integrador sin
una ventaja concreta de precisión/costo. Volver al splitting ya medido
como base para una nota completa C4/84, sin pedal, sostenida 160 ms,
seguida de apagador físico y cola suficiente para evaluar su caída.
Primero una nota; después frase y ampliación con F4. Mantener la ganancia,
sin EQ, muestras superpuestas ni fade que oculte el release.

Antes de lanzar el render, comprobar la ruta contacto/coast. La función
congelada `concert_grand_integrated_phrase.trajectory` elige contacto si
`step==1`: igualar ambas tasas a 128 kHz enviaría toda la trayectoria al
solver de contacto, aunque los martillos ya estuvieran separados. Usar
contacto 256/coast 128 kHz con eventos alineados, o un nuevo selector
contrastado, sin alterar el productor de la frase anterior. Verificar
contadores y continuidad; el inicio con apagadores cerrados tampoco queda
certificado por este coast de 8 ms con todos abiertos.

La escucha será diagnóstica: este resultado no autoriza afirmar
convergencia de duración completa. Medir presión por bandas y estado
durante el release sin bloquear la entrega audible por nuevos controles
de 8 ms en cadena. Si el costo exige ajustar el alcance, reducir cantidad
de notas antes de sacrificar la dinámica o afirmar precisión no demostrada.

## Límites que siguen pendientes

Esta prueba no certifica una nota completa, el release, todo el teclado o
la polifonía. El estado inicial no tiene convergencia global demostrada.
La reducción acústica espacial sigue fallando: 9,2021 % frente al 5 %
requerido. No hay nuevo WAV ni cambios en el plugin instalado durante
este control numérico. La selección audible y la identificación física
siguen separadas de la conservación de energía.
