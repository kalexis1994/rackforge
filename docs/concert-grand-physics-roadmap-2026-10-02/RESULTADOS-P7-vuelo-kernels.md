# P7 — Vuelo certificado, FFT real y reutilización de gradientes

2026-10-13. Continuación hacia el motor tocable, sobre **79f1a5f0**.
Se conserva el candidato de 96/192 modos por cuerda, tres cuerdas, puente
fijo y bloque 16 a 128 kHz que pasó el control de 250 ms.

**El costo sigue fallando. No hay aún instrumento integrado para tocar.**
El estado largo conserva error 0,324171% frente al control monolítico, pero
el coro aislado cuesta 4,7170 ms medios y hasta 10,3108 ms frente a 2 ms.
Se preservan rechazos, la ruta portable y todos los estados físicos.

## Cambios exactos, sin cambiar el modelo de fuerza

- **061691aa** prepara cotas de desplazamiento de las tres cuerdas sobre
  todos los ticks del bloque. Sólo compone el avance vertical si esas cotas
  y el movimiento futuro de la cabeza demuestran compresión negativa en
  todos los contactos, con margen de redondeo. Un martillo separado que se
  aproxima no pasa automáticamente la cota. Si la prueba no certifica,
  se ejecuta el contacto original a cada micro-paso.
- **f71e206e** prepara twiddles contiguos para mariposas AVX2 y diferencias
  contiguas para el solver Anderson. El Gram usa su simetría y productos
  escalares vectorizados. Detección CPU y reservas ocurren en preparación;
  `--portable` selecciona los kernels escalares. Se mantienen profundidad
  tres, regularización 1e−10, límites de coeficientes y tolerancias.
- **4f0c97ec** transforma pares reales con N/2 complejos y reconstruye
  el espectro real/DCT. Los 256 nodos de cuadratura usan 128 complejos,
  conservando la misma integral polinómica finita. Vectores exactamente
  cero devuelven exactamente cero sin FFT; no se usan umbrales ni se
  desactivan coordenadas. El control limpio es plano, con dirección
  horizontal cero. Los estados con ambas direcciones perturbadas siguen
  usando el cálculo completo.
- El gradiente/potencial calculado en el extremo aceptado de un bloque se
  reutiliza al comenzar el siguiente. Recolocar sólo la cabeza conserva
  esa caché; los avances monolítico y de gradiente constante la invalidan.
  Las iteraciones rechazadas no publican una caché nueva. `--uncached`
  permite contrastar el cálculo completo y da estados/fuerzas/energía
  idénticos, incluida una reiteración fuera de la frontera usual.

Estas optimizaciones no cambian masas, tensiones, potencial extensible,
fieltro, disipación, estados o ecuaciones de convergencia del bloque. No
resuelven la inercia mixta del puente móvil, pedales o presión del piano.
Las cotas necesitan ~0,57 MiB adicionales al mapa de portadores de ~5,72 MiB
por coro; sigue pendiente preparar/escalar la memoria de todo el teclado.

## CPU final y costo del ataque

Un hilo, C4, intensidad 84, 1153 coordenadas, 256 microframes a 128 kHz,
sin calentamiento con una nota ya sostenida. El primer impacto está incluido;
preparación queda fuera del tiempo. No hay tabla/aire, micrófonos, apagadores,
pedales, polifonía, MIDI ni host en estos tiempos.

| Ruta | Buffers | Media | P99 | Máximo / primer buffer | Plazo |
| --- | ---: | ---: | ---: | ---: | ---: |
| SIMD final | 512 | 4,7170 ms | 5,7415 ms | **10,3108 ms** | 2 ms |
| Portable final | 128 | 6,0589 ms | 7,4481 ms | **15,1163 ms** | 2 ms |

[SIMD final](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-final.json) y
[portable final](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-portable.json).
Ambas rutas tienen **cero asignaciones** durante el avance. La nueva
telemetría identifica el peor buffer como el primero, que además es el único
con fuerza de fieltro en estas corridas. El sostenido también falla CPU;
omitir el primer ataque no permitiría aprobar el motor.

SIMD certifica 8177 de 8192 bloques, reutiliza 8191 gradientes iniciales y
omite 433845 transformadas de cero exacto. Portable certifica 2033 de 2048
y reutiliza 2047. Estas cantidades describen este control plano; no son
un límite de costo con todas las direcciones/modalidades excitadas.

Los tiempos no tienen el mismo número de buffers en todas las etapas y
conservan variabilidad de sistema. No se presenta su diferencia como una
garantía del host. Los checkpoints anteriores conservan fuentes y datos:
[primer vuelo](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-primer-costo.json),
[primer SIMD](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-SIMD.json) y
[primera FFT real](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-FFT-real.json).

## Precisión y cierre autenticado

A 6 ms, estado contra monolítico al mismo reloj: **0,100690%**; fuerza de
ataque: **0,000489%**. Ledger mecánico máximo **1,5513e−14**. Esos controles
siguen pasando estado/fuerza ≤1% y ledger ≤1e−9, con trabajo verificado en
fronteras del bloque, no a cada micro-paso del avance por bloques.

[Cierre final de 250 ms](medidas-P7-cuerdas-nativas-2026-10-13/vuelo16-reiteracion250ms.json):
reiteración a 125 ms, velocidad 0,1 m/s y estados vivos. Error de estado
**0,324171%**; ledger máximo **6,6873e−13**. Trabajo de lanzamiento inicial
0,0077716424 J y de reiteración **−0,0036678346 J**, conservando su signo;
calor de fieltro 0,0010211818 J. El control monolítico tiene defecto de
trabajo 3,4538e−10 y pasa también el 1e−9.

El verificador autentica fuentes, ejecutable, padre y coeficientes. Reutiliza
el estado monolítico final del refinamiento anterior únicamente después de
comprobar los hashes de todas las fuentes de física/fixture y la identidad
de los bytes de entrada; así evita repetir el avance lento o duplicar ese
estado. Registra ruta, hash y padre de la referencia. Las optimizaciones
numéricas nuevas se contrastan además con Python y cuadratura independiente.
Este cierre compara contra el mismo micro-reloj, no prueba convergencia
continua de todos los modos o exactitud del instrumento completo.

**14 pruebas pasan**: diez de cuerdas/contacto y cuatro de cuerpo/presión.
Incluyen Gauss compuesto independiente 4/12/48/96, paridad con Python,
refinamiento DOP853 de pocos modos, composición contra micro-pasos completos,
impacto futuro, vuelo con ambos campos perturbados, paridad SIMD/portable y
reiteración con caché activada/desactivada. Los productores guardan fallos
y exigen rutas nuevas. No se generan WAV, factores acústicos ni paquetes.

## Trabajo pendiente

La parte que sigue dominando el sostenido es la solución no lineal repetida
del extremo y su corrección de trabajo; el ataque aún necesita el contacto
fino. Sigue reducir ese trabajo manteniendo precisión a duración completa,
después cerrar inercia y trabajo del puerto cuerda/cuerpo y agregar presión,
apagadores/pedales y host MIDI/audio. Los bloques 64 y la tasa 44,1 kHz que
fallaron duración siguen rechazados; estas optimizaciones no los rehabilitan.

## Instalador pendiente y chequeo del escritorio

También se conserva el cambio previo de `apps/rackforge-desktop/src/main.rs`:
el marcador de primera instalación ya no bloquea actualizaciones de un
Concert Grand conocido. Se usa la misma instalación/reemplazo que los
plugins oficiales, se conserva un plugin deshabilitado y el marcador sigue
respetando su eliminación intencional. No se ejecuta el instalador durante
esta validación ni se incorpora el coro experimental al paquete de fábrica.

`cargo +1.98.0-x86_64-pc-windows-msvc check -p rackforge-desktop --locked --offline`
pasa con entorno Visual Studio/LLVM y `CPAL_ASIO_DIR` apuntando al SDK local
en `local/build-dependencies/asio-sdk/ASIOSDK`. El chequeo GNU inicial carecía
de dlltool; el MSVC inicial falló en el extractor PowerShell del SDK.
Se extrajo el ZIP ya descargado al directorio ignorado, comprobando sus rutas,
y se repitió el chequeo con esa dependencia preparada. No se modifica la
configuración global del sistema ni se instala una nueva app.
