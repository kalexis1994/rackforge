# P6 — Hann gruesa y refinamiento de la sonda de 16 kHz

Fecha: 2026-10-08. Continúa Hann local, conservando productores e informes
anteriores. Sin audio, paquete nuevo, duplicados en SHARED o nueva R/cápsula.

## Diseño fijado antes de medir

- Autenticar contacto completo, checkpoint fino, cierre fino y todas sus
  fuentes/padres. Mantener límites, condiciones y geometría. El cierre
  fino aprobó Hann y dejó fallando sólo malla; no reinterpretar su alcance.
- Reconstruir **sólo 1536 kHz**, 0–3 ms, C4 supuesta, velocidad 5 m/s,
  96 transversales por dirección/192 axiales. Trece medidas físicas contra
  contacto grueso anterior <1e−10; balance e impulso <1e−9 y calor no negativo.
  La cápsula fina se autentica como vínculo del sistema; no equivale a una
  referencia del endpoint grueso. No guardar otro estado.
- Hann local V/A contra **los mismos oracles originales** en
  0/60/6/8/10/12/16/20/24 kHz, **1e−10**. Orden 8/12 y bloques 256/127,
  mismo límite. Conservar resta global. Gauss original V 12/16 es diagnóstico,
  no reemplazo de oracle. Heredar control fino aprobado, sin reintegrarlo.
- Comparar direcciones temporales en siete sondas con los vectores finos
  autenticados, como diagnóstico. No certificar integral espectral temporal
  de bandas a partir de esos puntos ni sustituir el fallo antiguo.
- Reutilizar vector **original** fino de V Hann a 16 kHz: base completa
  de 606 coordenadas, tabla, ventana y escala verificadas. Tres pruebas
  comprueban fase/escala y rechazan base/ventana alteradas, truncamiento,
  valores no finitos, cero e incompletitud. Sin duplicar esos vectores.
- Rayleigh **512×384/1024×768/2048×1536**, orden 12. Reproducir cambio
  anterior 512→1024 y Z frente a 1024 <1e−10. Cambio 1024→2048 **<2%**;
  cuadratura 12/16 en 2048 **<0,02%**. Conservar fallo anterior y comparación
  de Z vigente contra 2048; no actualizar operador o memoria acústica.
- Fases secuenciales: borrar historias nativas en RAM antes de la malla.
  Estimación conservadora de contacto heredada; para malla, doce arrays
  complejos y ocho reales del tamaño FFT más base 4 GiB. Máximo <12 GiB.
  Comprobar otra vez hashes de fuentes/padres, tabla, única R/manifiesto y
  cápsula al terminar. Global falso y demás controles antiguos intactos.

```text
python tools/audit-concert-grand-contact-hann-coarse.py --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-gruesa-malla16.json
```

## Alcance

Este ensayo cierra una observación de nueve frecuencias por tasa y una
acción armónica de malla. No valida operador completo, causalidad frente a
la malla fina, convergencia temporal de las bandas, ledger angular, duración,
release, unísono o timbre. No reemplazar los gates de contacto anteriores
por los locales ni escoger banda física sólo por el tamaño de una cola.

## Resultado del ensayo completo

[Informe](medidas-P6-contacto-banda-superior/Hann-gruesa-malla16.json):
**816,736 s (13,61 min)**, contacto/captura **516,391 s (8,61 min)**,
pico **4,309 GiB**. Estimación previa **8,911 GiB** (<12), fase de malla
**7 GiB** conservadora y secuencial, sin historia angular. El resto incluye
preparación/oracles y las cuatro acciones de malla; no es costo del contacto.

**11/12 controles locales**; falla sólo A Hann local gruesa a 24 kHz.
Global falso. Trece medidas físicas reproducidas con **error 0**. Balance
mecánico **2,42070e−12**, impulso **3,01265e−15**, calor no negativo.
Hashes de fuentes, padres, tabla, única R/manifiesto y cápsula verificados
al terminar. La cápsula no se modifica ni se trata como extremo grueso.

### Hann gruesa, mismo oracle

| Control | Error relativo máximo |
| --- | --- |
| V, modulación global anterior | 4,96309e−10 — falla |
| V, pesos locales | **9,00934e−11 — pasa** |
| A, pesos locales | **1,06024e−10 — falla** |
| A, integración global anterior | **4,72080e−12 — pasa** |
| Orden local 8/12, V/A | 0 en los datos calculados |
| Bloques locales, V/A | 9,41047e−12 — pasa |
| Oracle original V, Gauss 12/16 | 4,24702e−11 |

Límite **1e−10** intacto. La discrepancia del oracle V al cambiar su orden
es comparable a parte del margen; la coincidencia en doble precisión no
certifica exactitud con aritmética de intervalos. Cotas de Taylor tampoco
acotan redondeo de fase/contracciones ni error relativo en una cola nula.
Las nueve transformadas locales V/A cuestan **0,133 s**, sin acelerar la
dinámica. V local mejora el control que originó esta investigación; A local
introduce un fallo adicional, conservado aquí y en el control separado.

La dirección normalizada de V Hann cambia hasta **2,287629374%** entre
tasas (16 kHz), tanto con método original como local. La diferencia entre
ambos diagnósticos es mínima: el cambio temporal no se cura mejorando la
observación. Esto no es una integral de error temporal de banda ni una
certificación del paso más grueso.

### Sonda reutilizada de 16 kHz

| Control | Resultado |
| --- | --- |
| Reproducción del vector, cambio 512→1024 y Z frente a 1024 | **0** |
| Cambio 512→1024 | **2,55585% — fallo anterior conservado** |
| Cambio 1024→2048 | **0,651384% — pasa 2%** |
| Cuadratura 12/16 en 2048 | **1,15314e−15 — pasa 0,02%** |
| Z vigente frente a 1024 | **15,9305%** |
| Z vigente frente a 2048 | **16,3568%** |

El cambio de malla pasa sin volver a integrar el contacto fino; se usa el
vector completo del informe anterior, sin duplicarlo. Las acciones de
orden 12 tardan **5,382 / 21,373 / 86,666 s** en 512/1024/2048.
La carga causal conserva su error: esta malla más convergida es una
referencia de diagnóstico, no un operador físico nuevo.

Las siete sondas tienen ahora controles direccionales de malla aprobados,
con **referencias distintas**: seis heredan 512→1024 y la de 16 kHz usa
1024→2048. No afirmar convergencia uniforme de 2048 en las siete,
convergencia de operador completo o sustituir el gate original 256→512.

## Control adicional diseñado tras medir Hann gruesa

La trayectoria gruesa reprodujo trece medidas físicas con error 0. V local
pasa el límite **1e−10**, máxima **9,00934e−11** a 24 kHz. A local da
**1,06024e−10** y falla; no cambiar el límite ni borrar el ensayo. La A
analítica anterior da **4,72080e−12** máxima en la gruesa y **6,14836e−12**
en la fina: ya pasaba con margen mayor.

Se fija un **perfil separado de observación**: pesos locales para V,
integración analítica original para A. Ambas integran V/A reales de la
misma trayectoria Hermite y la misma Hann en aritmética exacta; no implica
fuerzas nuevas, amplitudes distintas, ajuste de frecuencias o diferenciación
de una variable enventanada. El helper anterior no se modifica.

El control enlazará los informes al terminar el ensayo actual, con las
nueve frecuencias y **mismo límite**, conservando el fallo experimental
de A local y todos los gates anteriores. No reconstruir física ni duplicar
vectores. Tres pruebas comprueban fase absoluta/extremos sin fuerza de
ventana, conservación de fallos y rechazo de límites/datos incompatibles.

```text
python tools/audit-concert-grand-hann-profile.py --coarse docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-gruesa-malla16.json --fine docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-local-malla.json --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/perfil-Hann-dos-tasas.json
```

### Resultado del perfil separado

[Perfil autenticado](medidas-P6-contacto-banda-superior/perfil-Hann-dos-tasas.json):
**pasa ambas tasas**, sin nueva dinámica, cambio de límites ni datos duplicados.

| Tasa | V local, máximo | A anterior, máximo |
| --- | --- | --- |
| 1536 kHz | 9,00934e−11 | 4,72080e−12 |
| 3072 kHz | 2,60361e−11 | 6,14836e−12 |

El helper `concert_grand_hann_observation.py` implementa esa combinación
para futuros diagnósticos. No modifica productores congelados, el perfil
no valida por sí solo bandas completas ni reemplaza el fallo de A local.
Las tres pruebas del perfil pasan en **0,096 s**; las tres de vectores en
**0,092 s**. El diagnóstico anterior de 13 gates sigue intacto.

## Decisión y siguiente paso

Se cierra la observación Hann prevista de V en las dos tasas y el
refinamiento direccional que faltaba. Mantener A analítica anterior; no
continuar trasladando un cálculo ya aprobado al método local que dio peor
acuerdo con su oracle. Conservar todos los informes y criterios.

Siguen pendientes **convergencia temporal de las bandas altas** y **error
de la carga vigente frente a referencias espaciales más finas**. El error
de Z a 16 kHz no disminuye al converger malla; resolverlo requiere estudiar
el aproximante/operador, no repetir cuadratura. No generar otra R o usar
una cápsula vieja con una carga cambiada sin reconstrucción autenticada.

Antes de otro contacto fino más costoso: fijar presupuesto y método del
**ledger angular del contacto completo**, y derivar/validar el **puerto
discreto** para reutilizar la respuesta lineal de tabla/aire. La dinámica
todavía domina el costo. Después duración/release, unísono, identificación
real y escucha. Global falso; plugin instalado intacto, sin WAV en SHARED.
