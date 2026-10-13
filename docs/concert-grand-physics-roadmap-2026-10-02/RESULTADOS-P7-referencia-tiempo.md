# P7 — Referencia coherente y convergencia del reloj

2026-10-13. Continuación autorizada desde **39c5ceaf**. Se corrige el cierre
de energía de la referencia y se estudia su precisión temporal antes de
atribuir la discrepancia del caso mixto a la corrección de trabajo.

## Posición y velocidad coherentes

El monolítico aceptaba la posición aproximada de Newton, mientras calculaba
la velocidad con el portador exacto y la fuerza resuelta. Un residuo pequeño
en posición puede producir un error de energía apreciable en modos rígidos.
Ahora publica ambas coordenadas con el mismo portador y exige también el
cierre de trabajo **del estado que realmente va a publicar**.

Se conserva el residuo de posición **2e−14** de la escala habitual
`max(norm(q0), norm(S*v0), 1e−12)`, el máximo de **40 iteraciones**,
el potencial constitutivo y los 1153 grados mecánicos. El nuevo control local
es `|E1 + calor - E0|/max(|E0|,1e−30) <= 1e−14`. Si falla, continúa Newton;
no se proyecta la energía después del paso. La evaluación del candidato usa
el segundo espacio de campos, conservando los campos iniciales que necesita
el gradiente AVF. Se reutilizan buffers existentes.

`--legacy-reference-step` reproduce el paso anterior. El verificador impide
reutilizar sus estados como si fueran una referencia del nuevo método.
Los informes conservan por separado el gate de trabajo de la referencia.

[Control de 256 ms con ambos ejes perturbados](medidas-P7-cuerdas-nativas-2026-10-13/checked-reference-control.json):

| Método | Ledger máximo | Límite 1e−9 |
| --- | ---: | --- |
| Anterior, posición de Newton | 8,5007e−8 | Falla |
| [Sólo posición compuesta](medidas-P7-cuerdas-nativas-2026-10-13/reference-position-only-rejected.json) | 6,0311e−9 | Falla; descartado |
| Posición/velocidad compuestas y trabajo local | 5,3048e−11 | Pasa |

El control legacy reproduce exactamente posición, velocidad, fuerzas, calor,
checkpoints y ledger del ejecutable anterior. El método seleccionado rechaza
30800 propuestas que ya cumplían el residuo de posición pero no el trabajo;
trabajo local aceptado máximo **9,9521e−15**. El máximo sigue en diez
iteraciones en este ensayo. El primer candidato queda conservado mediante
su parche aplicable a 39c5ceaf, fuentes y hashes de los resultados.

Pasan **26 pruebas** de cuerdas y presión. Incluyen el cierre del caso rígido,
la comparación con Python/ODE, conservación de los campos iniciales y la
identidad de estado/cache del motor por bloques al cambiar sólo la referencia.

## El balance de energía no cierra la cuestión temporal

Mismo estado inicial, C4, intensidad 84, seed 83, tres cuerdas 96/192,
**6 ms** y puente fijo. Un hash de todos los parámetros e inputs salvo el
reloj confirma que se conserva la física entre tasas. Se compara el estado
final en la norma habitual `(omega*q, v)` y la fuerza media por intervalos
comunes de 1/128000 s; no se comparan arrays de distinta duración o resolución.

[Primera serie](medidas-P7-cuerdas-nativas-2026-10-13/checked-reference-short.json)
y [extensión hasta 8,192 MHz](medidas-P7-cuerdas-nativas-2026-10-13/checked-reference-short-8192.json):

| Refinamiento del monolítico, kHz | Cambio de estado | Cambio de fuerza media |
| --- | ---: | ---: |
| 128→256 | 90,9073% | 0,583223% |
| 256→512 | 60,6554% | 0,421168% |
| 512→1024 | 73,5694% | 0,716597% |
| 1024→2048 | 4,2435% | 0,0042045% |
| 2048→4096 | 1,1881% | 0,0009854% |
| 4096→8192 | **0,4264%** | **0,0002489%** |

Todos estos monolíticos pasan ledger 1e−9; máximo de la serie **9,7130e−13**.
El último par pasa estado/fuerza 1% y trabajo: referencia refinada **para este
estado y esta ventana de 6 ms**. No valida 250 ms, todo el teclado o el piano.
La referencia de 128 kHz difiere **108,4612%** de la de 8,192 MHz en el estado,
aun cerrando energía. Sus comparaciones largas anteriores no permiten aprobar
la precisión temporal de ese estado mixto.

## Separación de los errores

[Atribución con los mismos inputs y referencia fina](medidas-P7-cuerdas-nativas-2026-10-13/checked-reference-attribution.json):

| Ruta del caso mixto, 6 ms | Error de estado contra 8,192 MHz | Error de fuerza media |
| --- | ---: | ---: |
| Bloque de 1 a 128 kHz | 127,0558% | 0,958877% |
| Bloque de 16 a 128 kHz | 132,5188% | 2,433108% |
| Bloque de 1 a 8,192 MHz | **0,02954%** | **0,00006760%** |
| Bloque de 16 a 8,192 MHz | **28,6562%** | **0,571998%** |

La ruta de un paso mejora de 0,11680% a 0,02954% al pasar de 4,096 a
8,192 MHz. Esa tendencia, y el fallo que permanece al interpolar fuerzas
durante 16 pasos, apuntan a la resolución temporal de los campos como
limitación principal de estos controles. No basta con recuperar convergencia
o cerrar trabajo. Esta es una inferencia de los ensayos, no una aprobación
global ni una validación de la duración completa.

Se conserva el [control mixto de 256 ms contra el monolítico de 128 kHz](medidas-P7-cuerdas-nativas-2026-10-13/checked16-recovery.json):
la nueva referencia pasa trabajo, pero el cambio de estado permanece
**136,9885%** y fuerza **2,4689%**. Ese reloj sigue sin validación temporal
para la trayectoria mixta larga. Recuperación y subdivisión manual coinciden
exactamente; el fallo de precisión no se convierte en una aprobación.

## Alcance del control limpio y costo

El mismo estudio con C4 inicialmente limpia conserva todos los modos.
Referencia a 128 kHz frente a 8,192 MHz: estado **0,0232184%**, fuerza media
**0,0100738%**. El par 4,096→8,192 MHz cambia estado **0,00001017%** y fuerza
**0,00000739%**; sus ledgers pasan. La ruta rápida N16 a 128 kHz frente a
esa referencia fina da **0,1036664%** de estado, **0,0102851%** de fuerza y
ledger **1,4955e−14**: pasa ese control corto, aunque el caso mixto no pasa.

[250 ms con reiteración a 125 ms](medidas-P7-cuerdas-nativas-2026-10-13/checked16-reiteracion250ms.json),
con referencia nueva calculada a 128 kHz: estado **0,3241655%**, ledger del
bloque **2,6324e−12**, de la referencia **6,4738e−13**. Sin refinamientos.
La comparación larga a 8,192 MHz no se calcula; no se amplía el alcance
del refinamiento corto a esa duración.

La referencia corregida no modifica el paso rápido por bloques. Medición
de C4 limpia, 256 micro-pasos a 128 kHz, N16, plazo 2 ms:

| Ruta | Buffers | Media ms | P99 ms | Máximo/ataque ms |
| --- | ---: | ---: | ---: | ---: |
| [CPU preparada](medidas-P7-cuerdas-nativas-2026-10-13/checked16-final.json) | 512 | 1,9295 | 2,2641 | 3,6341 |
| [Portable](medidas-P7-cuerdas-nativas-2026-10-13/checked16-portable.json) | 128 | 2,0921 | 2,4383 | 4,0867 |

Cero asignaciones durante procesamiento; ambas rutas siguen fallando el
presupuesto. No se atribuye a esta etapa una mejora de CPU. El caso mixto
sostenido también conserva su recuperación y su costo mucho mayor.

Siguiente: representar las fuerzas durante el bloque aprovechando las fases
de los portadores rápidos, con controles de trabajo y de trayectoria contra
la referencia refinada. El reloj de 8,192 MHz se usa como control offline.
La interpolación actual no se aprueba para campos arbitrarios perturbados.
Cuerpo móvil, presión, pedales y polifonía aún no se integran en este coro;
no hay audio nuevo, render de La Campanella ni modificación de fábrica.
