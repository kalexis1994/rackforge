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

Resultados pendientes.

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
