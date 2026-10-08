# P6 — Hann local, contribución absoluta y refinamiento de malla

Fecha: 2026-10-08. Continúa el contacto completo. Sin cambios físicos,
audio, paquete instalado ni archivos en SHARED. Informes sólo en repo.

## Diseño fijado antes de medir

- Reconstruir **sólo la tasa fina 3072 kHz**, 0–3 ms, C4 supuesta a 5 m/s,
  96 transversales por dirección/192 axiales. Mismos tabla, R, fieltro y
  condiciones. Trece medidas físicas <1e−10 y cuatro arrays del extremo
  exactamente iguales a la cápsula autenticada. Una R, una cápsula;
  trayectorias únicamente en RAM. Presupuesto conservador heredado <12 GiB.
- Integrar Hann local sobre V/A Hermite reales, conservando fase absoluta
  y espaciados almacenados. Taylor local de `sin²` antes de contraer el
  espectro, con cotas absolutas de resto, momentos exponenciales de 28
  términos y suma compensada por bloques. Rechazar pasos fuera del dominio
  de expansión. No diferenciar una posición/velocidad enventanada.
- Comparar contra **los mismos observadores originales**, 9 frecuencias
  0/60/6/8/10/12/16/20/24 kHz y **mismo límite relativo 1e−10**. Mantener
  resultados originales. Orden local 8/12 y bloques 256/127 (<1e−10).
  Oracle V Gauss 12/16 como diagnóstico adicional; no reemplaza el de 12.
- No afirmar que se resolvió el gate anterior de dos tasas: aquí no se
  vuelve a medir la gruesa ni la convergencia temporal. Global falso.
- Guardar siete vectores complejos de V Hann por método en el informe,
  sin normalización, con coordenadas, tabla y ventana identificadas. Ninguna
  historia o cápsula nueva. Reutilizar dirección original para reproducir
  sondas antiguas (<1e−10) antes de refinar celdas a 1024×768.
- Rayleigh en 256×192/512×384/1024×768, orden 12; control 12/16 en última
  malla (<0,02%). Cambio 512→1024 <2% en cada sonda. Conservar columnas
  contra Z vigente; no cambiar R ni aprobar operador/causalidad/sonido.
- Cuatro pruebas de Hann pasan (0,216 s), incluidas cuadratura independiente
  de A, extremos vivos/fase, cola con cancelación, bloques/orden y rechazo.
  El oracle antiguo de A tiene error absoluto ~4,5e−14 en la grilla sintética
  gruesa; se comprueba aparte contra nodos independientes. Esto no altera
  el límite de 1e−10 del contacto físico.

## Contribución absoluta sin reintegrar

[Informe](medidas-P6-contacto-banda-superior/contribucion-error-contacto.json)
autentica el contacto anterior y usa **sus mismos trapecios** y errores.
Para cada banda, `E² = error_relativo² × norma_fina²`; su contribución
respecto al total es `sqrt(E²/norma_total²)`. La suma de tres E² debe
reproducir el error total guardado, sin mezclar pesos Simpson. Dos pruebas
pasan, incluida detección de particiones/pesos incompatibles.

Presión Hann 1536→3072:

| Banda | Error relativo de banda | Contribución respecto al total |
| --- | --- | --- |
| 0–6 kHz | 0,00229322% | 0,00229321% |
| 6–12 kHz | 0,433597% | 0,000826485% |
| 12–24 kHz | 2,03722% | 0,000271615% |

La suma cuadrática reproduce **0,00245269%**. No demuestra irrelevancia
audible ni elimina los fallos relativos: permite separar precisión de una
cola pequeña del error de la señal total, en esta C4/ventana/micrófonos.

## Ejecución del control físico

```text
python tools/audit-concert-grand-contact-local-hann.py --parent docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/contacto-24k.json --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-local-malla.json
```

## Resultados y cierre del informe

[Mediciones y vectores](medidas-P6-contacto-banda-superior/Hann-local-malla.json),
[controles finales autenticados](medidas-P6-contacto-banda-superior/Hann-local-malla-validado.json).
**1523,050 s (25,38 min)** de medición, contacto/captura **1023,455 s
(17,06 min)**, pico **5,256 GiB**. Estimación conservadora heredada
**8,911 GiB** (<12). El resto incluye preparación, oracles y las siete
sondas de malla; no atribuir todo ese tiempo al observador Hann.

La medición guarda las siete sondas y termina con un error del resumidor:
`maximum_nested` del productor antiguo sólo acepta diccionarios/escalares,
no las listas nuevas. Se conservan productor y checkpoint sin cambios.
Un cierre independiente autentica fuentes/padres, tabla, R/manifiesto y
cápsula; calcula los mismos controles desde los datos ya guardados, sin
reintegración. Dos regresiones pasan (0,021 s), incluida conservación del
fallo de malla y rechazo de un conjunto de sondas incompleto.

```text
python tools/audit-concert-grand-contact-local-finalize.py --checkpoint docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-local-malla.json --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-local-malla-validado.json
```

El cierre es pequeño y referencia el checkpoint; **no duplica los vectores**.
Resultado **8/9 controles locales**: falla únicamente el cambio de malla
512→1024. Los controles antiguos de dos tasas siguen intactos, global falso.

### Reproducción y precisión de Hann

Trece medidas físicas reproducidas con **error 0**; cuatro arrays finales
**exactamente iguales** a la cápsula. Balance mecánico **1,03736e−12**,
impulso **5,16455e−15**, calor no negativo. El cierre vuelve a comprobar
todos los hashes; ninguna R, cápsula o historia nueva.

| Control fino | Error relativo |
| --- | --- |
| V Hann a 24 kHz, resta global contra oracle original | 4,37686e−10 |
| V Hann a 24 kHz, pesos locales contra mismo oracle | **2,60361e−11** |
| A Hann, peor sonda (24 kHz), pesos locales contra oracle | **9,60383e−11** |
| V/A local, cambio de bloques, máximo | 9,74113e−12 |
| V/A local, orden 8/12 | 0 en los datos calculados |
| Oracle original V, Gauss 12/16, máximo | 3,13854e−11 |

Todas las sondas locales pasan **1e−10** en esta tasa. A a 24 kHz pasa
con poco margen; la coincidencia con un oracle en doble precisión no es
una cota rigurosa del error frente a la solución exacta. Se conservan el
oracle de 12 y sus resultados; el de 16 no sustituye un fallo.

Las nueve transformadas locales de V/A cuestan **0,314 s**; no comparar
directamente con tiempos de 772 frecuencias o prometer ahorro de dinámica.
Cotas de truncamiento local: peso Hann absoluto **4,38773e−35**, momento
exponencial absoluto **7,66187e−67**, excluyendo redondeo/suma. Estas cotas
no representan errores relativos de un espectro casi nulo.

Los pesos locales evitan la resta global y usan espaciados reales. El
resultado apoya un problema de observación numérica, pero no aísla por
separado todos los efectos de espaciamiento y orden de suma. **No cambia
la física** ni prueba una causa del timbre. La tasa gruesa sigue sin
volver a medirse con este método; no declarar resuelto el gate de dos tasas.

### Refinamiento de celdas, misma fuente original

| Hz | Cambio 256→512 | Cambio 512→1024 | Z vigente frente a 1024 |
| --- | --- | --- | --- |
| 6000 | 3,82568% | **0,975070%** | 5,36621% |
| 8000 | 5,81416% | **1,49048%** | 7,87733% |
| 10000 | 6,31231% | **1,63764%** | 8,43180% |
| 12000 | 6,63437% | **1,73023%** | 8,74362% |
| 16000 | 9,50396% | **2,55585% — falla** | 15,9305% |
| 20000 | 4,65857% | **1,27145%** | 47,7541% |
| 24000 | 1,19678% | **0,335381%** | 72,4386% |

Cuadratura Rayleigh 12/16 en 1024: máximo **2,48363e−15** (<0,02%).
Reproducción de sondas anteriores: error 0 hasta 20 kHz; máximo a 24 kHz
**3,63621e−12** (<1e−10). La pequeña diferencia en esa sonda conserva
el efecto del bloqueo del observador global. No se reajustaron sus valores.

Seis sondas pasan el límite de malla 2%; **16 kHz sigue fallando**.
La disminución del error de malla no elimina el error de Z. En particular,
la carga histórica certificada en su discretización original no queda
certificada contra esta malla más fina, tampoco en la sonda de 6 kHz.
No extrapolar estas siete acciones a todo el operador, una fuente distinta,
memorias vivas, presión temporal o energía angular.

Los vectores complejos completos, siete por método, quedan ligados a tabla,
base de 606 coordenadas, ventana absoluta y ganancias sin normalizar.
El checkpoint ocupa **862567 bytes**; sin historia completa ni duplicados
en SHARED. Una siguiente malla puede reutilizar esos datos autenticados.

## Siguiente orden

1. Comprobar Hann local en la tasa gruesa; mantener el gate antiguo hasta
   medir ambas tasas. No confundir esa mejora con convergencia temporal.
2. Refinar la sonda de 16 kHz a 2048×1536 usando su vector guardado,
   conservando el límite 2%, cuadratura y comparación contra Z vigente.
3. Elegir la siguiente comprobación de paso/carga desde sus errores
   relativos y absolutos, sin borrar colas pequeñas ni cambiar el operador
   de una cápsula viva. Continúan pendientes temporal alto y carga física.
4. Ledger angular del contacto y duración/release, con presupuesto válido;
   derivar/validar el puerto discreto para reducir el costo repetido.

Después: unísono, identificación real y escucha. Plugin instalado intacto.
