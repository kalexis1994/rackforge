# Cuerpo y sostenido sobre la release publicada

2026-10-14. El usuario confirma que quiere continuar desde el piano incluido
en la release y prioriza **cuerpo y sostenido**. Base: RackForge v0.1.32,
Concert Grand **0.171.35**, commit `91eb43f88495f0485e7a7ed61718280ddce23a8d`.
La rama 0.172.0 y el prototipo SI se conservan como investigación.

## Decisión de trabajo

La última escucha del prototipo mejoró algo los ticks, pero sigue muy lejos
del sonido de la release según el usuario. La nueva base es el instrumento
publicado. Se mantienen su ataque, captación, programas y funcionamiento;
se investiga una debilidad concreta por vez y se compara su audio con esa base.

El error de proceso fue llegar tarde a esa comparación musical. Conservar
energía, superar pruebas numéricas y encontrar similares dos reducciones de
un fragmento corto no demostraba una mejora sobre el instrumento aceptado.
Se conservan los controles, solvers y aprendizajes de la investigación, sin
trasladar sus supuestos geométricos ni su calibración completa al piano actual.

La revisión completa se conserva en la rama `codex/concert-grand-restrike`,
commit **790335e9**, documento
`docs/concert-grand-physics-roadmap-2026-10-02/REVISION-release-y-aprendizajes-2026-10-14.md`.
Allí también están la identidad del snapshot publicado, seis pruebas sobre
él, las dos huellas distintas de 0.172.0 y el historial de las escuchas rechazadas.

## Qué se mide ahora

Se compila como biblioteca el **código de producción sin modificar** de
0.171.35, con su SDK del mismo tag. Un consumidor externo llama a sus métodos
públicos. No sustituye el motor por el nuevo solver ni por notas muestreadas.
El programa Concert se carga explícitamente; 44,1 kHz, bloques de 256,
200 ms de preparación silenciosa, tecla mantenida dos segundos y salida
de cuatro segundos. Doce casos: C3/A3/C4/D#4/C5/C6, velocidades 60 y 110.

Se vuelve a calcular también el modelo local 0.172.0 con su ejecutable
congelado autenticado. Se verifican las grabaciones originales y los metadatos
del corpus Steinway ORTF ya utilizado: nota raíz exacta, sin transposición.
Este corpus conocido no es una validación nueva ni ciega, y sus envolventes
no identifican por sí solas materiales, pérdidas o fase del instrumento real.

Se mide presión estéreo completa, usando ambas cápsulas para evitar
cancelación mono. La alineación por inicio se aplica sólo al análisis;
los archivos crudos conservan todas sus muestras, niveles y retardos.
No se ajusta ganancia por nota ni se ecualiza. La comparación de caída es
relativa al ataque de cada señal: no supone que ambas versiones o la
grabación tengan la misma calibración absoluta.

## Primera localización del problema

Caída del fundamental entre el nivel medio de 100–200 ms y el de
500–1400 ms después del inicio acústico. Valores en dB, velocidad **60**:

| Nota | Release 0.171.35 | Investigación 0.172.0 | Referencia |
| --- | ---: | ---: | ---: |
| C3 | −16,46 | −9,95 | −10,80 |
| A3 | −3,76 | −10,35 | −19,46 |
| C4 | −16,09 | −21,69 | −5,78 |
| D#4 | −22,20 | −13,34 | −16,87 |
| C5 | −14,01 | −10,34 | −13,20 |
| C6 | −19,76 | −21,61 | −16,85 |

La velocidad 110 conserva casi el mismo patrón en el registro medio.
El déficit de sostenido de C4 **sí aparece en la release**, pero es menor
que en 0.172.0. A3 presenta el problema opuesto frente a esta referencia,
y C5 tiene una caída total parecida. No corresponde alargar todo el piano.
Estos datos describen H1, no un porcentaje de calidad audible ni todo el cuerpo.

En C4/60, las pendientes locales de la release son −16,70 / −19,07 / −32,33
dB/s en 150–400 / 450–800 / 900–1400 ms. La referencia da −9,62 / −6,59 /
−7,21 dB/s. El dato tardío aún no separa pérdidas y cancelación del unísono.
Otras notas muestran recuperaciones de nivel: D#4 y C5 tienen pendientes
positivas en la última ventana. No convertir esas envolventes automáticamente
en constantes de pérdida ni afirmar que el modelo genera energía.

## Primer control: tabla de caída inicial

La release usa `PROMPT_MEASURED_POWER=1`, que hace seguir al fundamental una
tabla empírica de caída inicial. Se prueba **sólo** pasar ese valor a cero,
manteniendo contacto, detune, pérdidas, captación y niveles restantes.
Es una ablación de diagnóstico, no una calibración seleccionada.

| Nota, velocidad 60 | Caída base | Tabla desactivada | Referencia |
| --- | ---: | ---: | ---: |
| C3 | −16,46 | −3,88 | −10,80 |
| A3 | −3,76 | −7,93 | −19,46 |
| C4 | −16,09 | −17,73 | −5,78 |
| D#4 | −22,20 | −22,27 | −16,87 |
| C5 | −14,01 | −28,53 | −13,20 |
| C6 | −19,76 | −19,76 | −16,85 |

**Se rechaza quitar globalmente la tabla:** no recupera el sostenido de C4
y empeora mucho C5. La tabla tampoco explica por sí sola el déficit de C4.
No se modifica ningún coeficiente de fábrica, no se instala un paquete y
no se presenta el control como una mejora audible.

## Verificación y siguiente paso

El consumidor de producción reproduce exactamente la huella publicada
`e74df30116b39a5a` al usar la inicialización del control de CI. Para ese control
se omiten explícitamente la recarga de preset y los 200 ms de preparación
diagnóstica: comparar las dos inicializaciones sin igualarlas había dado una
huella distinta. No se alteró la biblioteca ni el consumidor que generó las
doce mediciones para superar la comprobación.

El medidor de envolventes pasa un control analítico: tono exponencial de
440 Hz, cápsulas en oposición de fase y pendiente conocida −6,080123 dB/s.
El error máximo es **7,99e−7 dB/s**. Todos los renders nuevos son finitos;
fuentes, eventos, binarios, referencias y audios crudos quedan autenticados.
El banco comparado completo tarda **9,73 s** en esta máquina, incluida la
ejecución de ambas versiones y el análisis; la ablación tarda unos cinco
segundos. Estos tiempos no son pruebas de reserva de CPU del plugin WASM.

El siguiente ensayo conserva la release y separa en C4 las magnitudes de
los estados del unísono y su suma proyectada, antes y después del receptor.
Esas magnitudes heredadas no son julios. Se busca distinguir pérdida real
de amplitud y cancelación, con C3/A3/D#4/C5 como controles, antes de elegir
un cambio local de la transición hacia el sostenido. El ataque seguirá
comparándose por separado y la selección final será por escucha.

- [Medición de la base y procedencia](baseline.json).
- [Control rechazado de tabla desactivada](prompt-table-off.json).
- [Identidad del consumidor con el piano publicado](probe-verification.json).
- `tools/concert-grand-release-probe.rs`: consumidor del motor original.
- `tools/measure-concert-grand-release-body.py`: banco y control analítico.
- `tools/audit-concert-grand-release-prompt.py`: ablación de un coeficiente.
- `tools/verify-concert-grand-release-probe.py`: comprobación de la huella.

Datos de trabajo en `local/concert-grand-release-body-2026-10-14/`.
Los archivos de referencia permanecen en su ubicación original de investigación;
no se duplican grabaciones ni factores acústicos.
