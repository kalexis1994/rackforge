# P7 — Dinámica y observador con proyección variable

2026-10-12. Etapa anterior guardada en **747f3917**. Diagnóstico de 161 modos
contra el maestro físico asumido de P6; no se mide similitud al Steinway.

## Qué cambia

En cada evaluación de K/C se resuelve el mínimo lineal **real** del observador
causal de aceleración. La dinámica usa el error de movilidad, el error de
presión del observador óptimo y su penalización. Conserva masas/B, los dos
momentos de almacenamiento, matriz de pérdida positiva y retardos físicos
de cuatro grupos por micrófono, 1,914–3,469 ms. No hay presión en el lazo de
realimentación mecánica, nuevos modos, ajustes de fieltro ni ganancias por nota.

El problema interno escala columnas una sola vez con la dinámica inicial.
La penalización es `lambda * |beta|²/2`, lambda=1e−6, con beta=escala fija ×
coeficiente físico. La escala permanece fija durante las iteraciones. Así la
derivada de la función minimizada se obtiene manteniendo el observador óptimo
constante en esa derivada; la condición de mínimo elimina su derivada implícita.
La penalización se incluye en el valor del objetivo. Reescalar columnas en
cada iteración cambiaría esa función y no justificaría el mismo gradiente.

Dos pruebas nuevas pasan: diferencias finitas que **vuelven a resolver** el
observador para cada perturbación, y contraste del adjunto con la formulación
anterior de observación fija. Las factorizaciones LU escaladas se reutilizan
para estados y adjuntos de los dos micrófonos. El residuo de las ecuaciones
normales del observador queda ~1e−14 durante el ajuste.

## Condiciones y resultados

Se usan las 226 frecuencias de entrenamiento del ajuste conjunto; 111 distintas
para validación. La base de rigidez permanece fija. Preparación con uno/seis
hilos BLAS sobre el mismo objetivo/gradiente: se elige un hilo por mediana.
300 iteraciones, **433,88 s**. Alcanzó el límite, sin afirmar convergencia.
Se conserva un checkpoint único actualizado y K/C/observador antes del cambio
modal. No se crea otra R, historia de desplazamientos o audio.

| Variante | Movilidad RMS / máximo | Presión RMS / máximo |
| --- | ---: | ---: |
| Etapa anterior, observador recalculado | 4,7021 % / 14,8962 % | 9,9881 % / 24,0357 % |
| Proyección variable, observador sobre 226 frecuencias | **4,5449 % / 14,5328 %** | **6,7297 % / 19,2423 %** |
| Misma dinámica, observador sobre las 1189 frecuencias | 4,5449 % / 14,5328 % | **5,9368 % / 12,0030 %** |

El último control tarda 4,50 s, con **dinámica, escala de regularización,
lambda y retardos idénticos**. Sólo amplía el entrenamiento lineal del
observador. No usa las frecuencias reservadas. El observador inicial del
productor se prepara con una pequeña perturbación positiva de pérdidas; su
control se mide con el cuerpo padre sin esa perturbación. No se presenta ese
control como reproducción exacta del estado inicial del optimizador.

La presión mejora **40,6 % relativo en RMS** respecto a la etapa anterior.
Los gates siguen siendo RMS ≤5 % y máximo ≤10 % por operador; **ambos
operadores todavía fallan**. No se acepta un score combinado para ocultarlo.
Informes en `medidas-P7-cuerpo-cargado-2026-10-12`:

- `variable-161.json`: ajuste de dinámica/observador.
- `variable-161-observador-completo.json`: control con todas las frecuencias.
- `verificacion-variable-161.json`: replay y autenticación independientes.

## Conservación y verificación

La flexibilidad estática final difiere 3,51e−9 del maestro; inercia 1,54e−15.
Pérdida mínima positiva 5,6841. Transformación por bloques: movilidad 1,67e−11,
presión 1,01e−11. El verificador reconstruye K/C desde el checkpoint y compara
con los operadores guardados; comprueba ortogonalidad y el complemento de
almacenamiento. Ensambla de nuevo el problema lineal con **solves sin escalar**
y verifica que el observador del ajuste satisface sus ecuaciones normales.
Reproduce movilidad y ambos observadores de presión, y recalcula los gates.
No vuelve a ejecutar el aire completo en el tiempo.

## Costo nativo de cuerpo

La realización usa 179 resonadores, incluyendo polos reales sobreamortiguados,
sin recortar polos. Dos controles Rust/Python de estado final/checksum/trabajo
pasan; la energía por paso se comprueba en la trayectoria reconstruida Python.

| Tasa / 256 frames | Media | P99 | Máximo observado | Asignaciones |
| --- | ---: | ---: | ---: | ---: |
| 44,1 kHz | 0,8354 ms | 1,2318 ms | 1,6930 ms | 0 |
| 48 kHz | 0,8129 ms | 1,1923 ms | 1,8126 ms | 0 |

P99 consume **21,22 %/22,36 %** del plazo. Falla la reserva de 20 % en ambas
tasas, aunque entra en el plazo aislado. Se mantienen los límites; este costo
no incluye presión, cuerdas/contacto/pedales ni garantiza el callback del host.
Informe: `runtime-variable-161.json`. Fábrica y paquete siguen intactos.

## Siguiente ensayo

El promedio mejoró; ahora hay que reducir **picos de error del entrenamiento**.
Usar residual sobre las 1189 sondas, enriquecer el conjunto de ajuste con sus
peores puntos y fijar pesos de identificación antes de optimizar. Mantener
escala/regularización del observador y la misma familia positiva, con controles
por diferencias finitas. No seleccionar puntos a partir de la validación.
Si sigue faltando capacidad, seleccionar estados adicionales por el residual
conjunto de movilidad/presión. El costo y la validación de presión digital/
transitorios continúan abiertos, antes de integrar cuerdas y escuchar el piano.
