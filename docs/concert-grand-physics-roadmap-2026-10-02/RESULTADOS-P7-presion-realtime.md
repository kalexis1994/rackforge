# P7 — Cuerpo cargado y micrófonos en el consumidor nativo

2026-10-13. El usuario pide seguir hacia un motor que pueda tocar en tiempo
real. Se parte del ajuste residual de 161 modos, aún fuera de fábrica.
**Esta etapa entrega un componente de cuerpo/presión, no un piano tocable.**

## Resultado

El consumidor Rust produce velocidades medias recíprocas en los 88 puertos y
presión estéreo causal. Conserva los 180 resonadores del cuerpo de 161 modos,
las pérdidas completas, sus momentos y el mapa espacial 88→20. Los micrófonos
usan ocho historias escalares, cuatro por micrófono; ninguna historia por modo.
Preparación, lectura, detección de AVX2/FMA y movilidad instantánea ocurren
fuera del procesamiento. Se conservan rutas portable y vectorizada.

La presión es una observación: no cambia K/C, masas, puertos, fieltro o ganancias
del cuerpo. No se usan muestras de notas ni se cambia el paquete 0.172.0.

La primera conversión directa del observador continuo falla. La primera
comparación además contabilizaba un medio intervalo extra en la fase; queda
conservada como incidencia, no como comparación final válida. El retardo del
hold y el adelanto de la observación media se compensan; sólo se retira para
comparar la latencia **declarada** del FIR, siete muestras (0,159/0,146 ms).
El consumidor mantiene esa latencia y nunca necesita muestras futuras.

Reajustar sólo la aceleración digital reduce el peor error a 5,987/5,762 %, pero
sigue fallando el 1 %. La observación final identifica coeficientes reales de
estado/fuerza para el consumidor muestreado, con los mismos cuatro retardos
físicos por micrófono y FIR causal de 16 taps. Se impone algebraicamente que
una fuerza estática no genere presión constante ni un término de primer orden
en frecuencia. La fuerza directa del observador no realimenta la mecánica.

Se ajusta sobre las 1189 frecuencias de entrenamiento, nunca sobre las 111 de
validación. Regularización del nuevo problema lineal: 1e−8. Las frecuencias de
validación se reutilizan durante el desarrollo; no son un test final nuevo.

| Medida final | 44,1 kHz | 48 kHz |
| --- | ---: | ---: |
| Peor error de conversión frente al mismo cuerpo continuo | **0,298804 %** | **0,294668 %** |
| Gate de conversión ≤1 % | pasa | pasa |
| Movilidad contra maestro P6, RMS / máximo | 4,746019 / 13,449507 % | 4,724316 / 13,226738 % |
| Presión contra maestro P6, RMS / máximo | 5,780425 / 9,144772 % | 5,780290 / 9,146841 % |
| Gates de fidelidad RMS ≤5 %, máximo ≤10 % por operador | fallan | fallan |

Resolver la conversión digital no resuelve la fidelidad heredada del cuerpo.
Estos porcentajes comparan contra el maestro asumido de P6, no contra Steinway.
Tampoco prueban transitorios del aire original o la banda por encima de 6 kHz.

## Costo y variabilidad

Benchmark del consumidor actual: un hilo, 88 entradas/salidas, 256 frames,
2048 buffers después de calentamiento, cuerpo **más presión** incluidos.
Mismos coeficientes en ambas rutas; no se vuelve a ajustar para medir.

| Tasa / ruta | Media | P99 | Máximo | Reserva 20 % |
| --- | ---: | ---: | ---: | --- |
| 44,1 / SIMD | 0,718982 ms | 0,9935 ms | 1,4068 ms | pasa |
| 44,1 / portable | 0,913205 ms | 1,2525 ms | 1,7860 ms | falla |
| 44,1 / repetición SIMD | 0,720701 ms | 0,9811 ms | 1,2371 ms | pasa |
| 48 / SIMD | 0,723853 ms | 1,0198 ms | 1,5010 ms | pasa |
| 48 / portable | 0,926602 ms | 1,2721 ms | 1,4314 ms | falla |
| 48 / repetición SIMD | 0,746562 ms | 1,1387 ms | 1,2871 ms | falla |

Todas registran **cero asignaciones** durante el procesamiento. Plazos:
5,804989/5,333333 ms. El máximo observado del consumidor final entra en ambos,
pero a 48 kHz la reserva no es consistente y la ruta portable no la cumple.
Las corridas previas también se conservan: una alcanzó 7,5473 ms a 48 kHz.
No se atribuye la variación a una causa todavía no medida. No es un benchmark
del callback ASIO, del instrumento completo, de WASM o de otros dispositivos.

## Controles e interfaz para conectar cuerdas

Se corrige el verificador residual para incluir el informe padre
`acoplado-4-woodbury.json`; el replay residual pasa. Sus siete controles
unitarios pasan. Cuatro controles nuevos pasan: reproducción nativa/Python,
rutas portable/SIMD, respuesta temporal/armónica causal, salida general de
estado/fuerza y conservación de los dos ceros de presión en DC.

El replay del consumidor final en ambas tasas/rutas reproduce presión a
3,64e−15/3,58e−15 relativos, estados, velocidades y trabajo. Además expone
`free_latent_velocity`, `held_mobility` y `step_latent` para cerrar el puerto
antes de aceptar un avance. La identidad `v_media = v_libre + M_hold*F`
pasa a 2,25e−15/2,08e−15 relativos, sin adelantar estados ni retardos durante
la consulta. No constituye por sí sola una integración de las cuerdas.

El verificador independiente usa una exponencial real aumentada, distinta
de los polos complejos del consumidor. Reproduce movilidad y presión de las
111 frecuencias reservadas, los dos momentos de DC y los gates de CPU, sin
reescribir los resultados fallidos. No reintegra el aire original en el tiempo.

Fuentes: `concert_grand_realtime_pressure.py`, `concert-grand-pressure-body.rs`,
`measure-concert-grand-realtime-pressure.py`,
`remeasure-concert-grand-pressure-runtime.py` y
`verify-concert-grand-realtime-pressure.py`. Evidencia en
`medidas-P7-presion-realtime-2026-10-13/`; cierres actuales:
`estados-DC.json`, `runtime-final.json`, `verificacion-final.json`.
Los informes intermedios registran versiones anteriores del productor;
el cierre final autentica el consumidor actual, sin sustituir esas corridas.

Coeficientes, consumidores y entorno Python permanecen en `local/`, ignorado
por Git. No hay nuevo WAV, instalación, paquete o duplicado de R.

## Siguiente integración necesaria para una prueba tocable

1. Implementar cuerdas/contacto en Rust con estados vivos y puertos cerrados
   sobre esta interfaz. Conservar el trabajo, la inercia compartida y las
   pérdidas; no alimentar el cuerpo con samples o voces de otro modelo.
2. Validar ataques, reiteraciones y apagado frente al banco físico congelado,
   incluyendo movimiento continuo de CC64/67 y eventos dentro del buffer.
3. Medir **todo** el recorrido con polifonía, pedales y buffers pequeños, luego
   el callback del host. Mantener visibles los fallos de fidelidad y reserva.
4. Preparar el ejecutable de escucha MIDI con alcance explícito del banco;
   después ampliar teclado, cerrar fidelidad y evaluar integración portable.

No presentar el componente aislado como un instrumento que ya se puede tocar.
