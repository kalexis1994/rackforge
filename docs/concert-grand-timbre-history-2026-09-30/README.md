# Revisión del timbre acumulado

30 de septiembre de 2026. El usuario rechaza el modelo 13: los agudos siguen delgados, casi como una cuerda de guitarra, y aclara que el deterioro percibido viene de varias iteraciones. Esto invalida la selección por mejoras numéricas como criterio suficiente de avance. No se propone otro candidato en esta revisión.

## Comparación realizada

Se comparan renders históricos 06 y 07 disponibles, los binarios congelados 11 y 12, C y 13, y dos condiciones de grabación originales del BitKlavier: stage/lid/ORTF y harp/omni. Notas exactas C4/C5/C6/C7, velocidades 60 y 124. Hay pares históricos ausentes: no se reconstruyen ni se extrapolan. La versión 06 disponible alcanza C6; 07 alcanza C5 en esta selección. Los modelos 11/12 se renderizan con sus propios binarios y parámetros congelados.

Se analiza el canal izquierdo. Cada archivo se alinea con el detector de comienzo histórico. Se localizan máximos próximos a los parciales en 50–400 ms y se integra su energía en una banda de semiancho máximo entre 18 Hz y el 2,5 % del fundamental nominal. Las ventanas son 50–200, 200–500, 500–1500 y 1500–2400 ms. Son medidas de grupos próximos a parciales, no una separación perfecta de cuerdas, tabla, sala y ruido. Los cocientes y caídas no dependen de la ganancia de exportación. No se modifica el audio ni se ajusta el modelo.

### Caída del fundamental en notas fuertes

Energía de H1 en 0,5–1,5 s respecto a 0,05–0,2 s, en dB:

| Nota | 06 disponible | 11 | C | 12 | 13 | Original stage | Original harp |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| C5 | −10,60 | −5,74 | −4,77 | −4,82 | −4,78 | −11,04 | −13,42 |
| C6 | −25,89 | −22,82 | −27,21 | −26,97 | −27,36 | −22,11 | −20,80 |
| C7 | ausente | −29,78 | −28,92 | −29,01 | −28,94 | −21,70 | −23,58 |

En C6/C7 el 13 no recupera el fundamental sostenido respecto a C: el cambio es inferior a 0,2 dB. Su caída supera las dos condiciones originales en unos 5–7 dB. En C5 sucede lo contrario: C y 13 conservan H1 más que las grabaciones; el 06 disponible se aproximaba más a stage en este indicador. Aumentar globalmente la duración no corrige ambas direcciones.

También difiere el equilibrio entre parciales. Por ejemplo, en C5 fuerte, H2–H6 respecto a H1 durante 0,5–1,5 s es −11,5 dB en 13 y −18,2 dB en stage. Las diferencias entre micrófonos son grandes: no es válido atribuir toda la divergencia a la excitación del martillo o a una propiedad única del alambre. El JSON conserva todas las ventanas, frecuencias seleccionadas y hashes; algunos parciales débiles o colas pueden incluir ruido o máximos ajenos al parcial buscado.

La duración sostenida de la tecla en cada muestra original no se identifica de forma independiente. Las caídas son medidas del audio grabado, no estimaciones directas de pérdidas intrínsecas ni una prueba causal del defecto perceptual. Deben separarse pérdidas, transferencia acústica y posible liberación antes de utilizarlas como objetivos físicos.

## Decisiones acumuladas relevantes

- C desactiva la desafinación de unísono y polarización. El 12 retiene una anchura pequeña; el 13 la reduce aún más. Esto elimina una fuente de variación, pero no identifica automáticamente la respuesta colectiva de un piano real.
- El nuevo contacto del 13 modifica la excitación, pero conserva el cálculo posterior de pérdidas y transferencia acústica. Por eso mejorar el ataque o los graves del pulso de caja no implica recuperar el fundamental sostenido.
- El contacto se normaliza por golpe para alcanzar un pico analítico y posteriormente se aplica la curva de sonoridad. Las pruebas conservativas del integrador no demuestran calibración absoluta de energía acústica en la salida.
- Las guardas recientes de cuerpo comparaban principalmente contra C, que ya conserva el defecto. Un candidato puede pasar esas guardas y seguir lejos del original.
- El error agregado reutiliza un corpus histórico y puede ocultar regresiones de notas concretas. Energía grave de ataque, mínimos de envolvente y número de estados simulados no equivalen a riqueza perceptiva ni fidelidad.

## Cambio de método

El siguiente ajuste debe partir de referencias congeladas, incluyendo una versión que el usuario aprobó, y contrastar conjuntamente el balance de los parciales y sus envolventes por registro. Primero hay que distinguir la contribución de excitación, pérdidas y observación acústica; después calibrar el conjunto con una condición de micrófono explícita y otras condiciones para comprobar sensibilidad. Las comparaciones por nota y dinámica deben conservarse visibles, en vez de decidir por un único promedio. Una mejora de consistencia física se documentará como tal, sin presentarla como mejora auditiva hasta comprobarla.

No se establece una causa única del timbre de guitarra ni se concluye que se llegó al límite del modelado físico. El modelo 13 queda rechazado por la escucha del usuario. El plugin instalado y el código de producción permanecen sin cambios.

## Archivos

`tools/audit-concert-grand-timbre-history.py` reproduce el análisis usando `profiles.json`. `report.json` contiene medidas, ausencias y hashes de los WAV; `provenance.json` conserva las configuraciones y procedencias de los renders anteriores. Copia para consulta: `C:\SHARED\Diagnostico-timbre`.

## Referencia física

[Woodhouse, Euphonics: múltiples cuerdas y dobles decaimientos](https://euphonics.org/7-3-multiple-strings-and-double-decays/) muestra que la simetría de las cuerdas y la movilidad del cuerpo determinan qué modos irradian y cuánto decaen. En el caso ideal simétrico, varias cuerdas pueden responder como una sola más gruesa. Es una explicación de mecanismos posibles, no una identificación de nuestros coeficientes ni del defecto escuchado por el usuario.
