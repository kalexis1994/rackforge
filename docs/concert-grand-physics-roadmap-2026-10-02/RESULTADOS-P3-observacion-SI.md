# P3/P6 — Presión SI desde la superficie y diagnóstico de carga acústica

Fecha: 2026-10-03. La investigación acumulada del piano, herramientas y
documentación quedó en el commit **346b6c8**. Esta etapa posterior construye
y verifica el observador; todavía no es un programa audible seleccionado.

Datos y fuentes:
`C:\SHARED\Fisica-pendiente-2026-10-03\P3-observacion-SI`.
El plugin instalado y su paquete 0.172.0 siguen preservados.

## Modelo y alcance

Se observa el movimiento **intrínseco** de los 35 modos físicos bajo 800 Hz
del activo P2, con sus formas normalizadas por masa. El desplazamiento B
está en sqrt(kg)·m; la forma modal está en kg^(-1/2). No se convierten
ganancias empíricas de voces nativas a unidades físicas.

Para una superficie plana en una pantalla rígida infinita, se aplica la
integral de Rayleigh a la aceleración con retardo de cada punto:

`p(m,t) = rho/(2 pi) integral_S a(s,t-R/c)/R dS`.

El signo corresponde a desplazamiento positivo hacia el semiespacio observado.
En frecuencia se usa exp(+i omega t), con propagación exp(-i omega R/c).
Las formas modales firmadas se suman coherentemente sobre la superficie.
La derivada temporal no se sustituye por un coeficiente de brillo ajustado.

La formulación y la integración de potencia sobre la superficie/semiespacio
se contrastaron con [Putra y colaboradores, Inter-noise 2014](https://acoustics.asn.au/conference_proceedings/INTERNOISE2014/papers/p898.pdf).
Su geometría de pantalla rígida es una aproximación explícita del piloto.
No reproduce por sí sola las dos caras, borde, tapa y cavidad del piano.

Geometría de ensayo, fija en todos los controles:

- Panel P2 supuesto de 1,4 × 1 m, en z=0.
- Cápsulas omnidireccionales en (0,55; 0,5; 0,66) y (0,85; 0,5; 0,66) m.
- Aire: rho=1,2 kg/m³ y c=343 m/s.
- Ganancia SI=1. Sin normalización por nota, room, patrón de micrófono,
  proximidad, ecualización o nueva calibración contra las muestras.

Este observador es **de lectura**: no devuelve presión a la tabla. El
presupuesto mecánico anterior no incluye energía del campo acústico. Obtener
presión correcta para una vibración prescrita no completa esa interacción.

## Pruebas independientes: seis pasan

1. Presión de un disco uniforme contra la solución analítica del pistón
   circular en su eje, incluyendo fases, unidades y el límite de frecuencia cero.
2. Fuente puntual: ganancia y primer momento del retardo; ausencia de llegada
   antes del tiempo geométrico discretizado; respuesta temporal/frecuencial.
3. Campo temporal contra funciones analíticas evaluadas en sus tiempos
   retardados, refinando 48/96/192 kHz.
4. Superposición, superficie con signos opuestos y cápsulas reflejadas.
5. Rechazo de geometrías inválidas y fuentes no finitas.
6. Potencia radiada contra fuente puntual analítica; resistencia modal
   simétrica y positiva para una superficie con varios modos.

El kernel temporal coloca su primer tap en ceil(R/c * rate) para no generar
precursor. Dos taps conservan ganancia constante y primer momento del retardo;
el segundo puede ser negativo por la extrapolación numérica. No representa
otra fuente física. Su precisión se verifica en la banda declarada.

## Acoplar la captura mecánica sin cambiarla

Se repitió C4/124 con extremo móvil, banco bajo P2, 48/72 modos de cuerda y
2N axiales. En ambos casos, **el estado final es exactamente igual** al NPZ
congelado de la etapa anterior. La nueva observación no modifica el contacto,
la cuerda o la carga del soporte.

Se conserva B(t) y su velocidad en 401 puntos durante los primeros 4 ms.
Para analizar su aceleración en 60–800 Hz se integra por partes:

`A(omega) = [B' exp(-i omega t)] + i omega [B exp(-i omega t)]`
`           - omega² integral B(t) exp(-i omega t) dt`.

Así se evita diferenciar la velocidad muestreada o plegar directamente la
aceleración de los modos axiales altos. Los términos de borde corresponden
a la **aceleración limitada a esa ventana**; no se omiten. Refinar la integral
temporal de 201 a 401 puntos cambia el resultado 4,157e-7 relativo.

Las salidas son transformadas de presión en **Pa·s**, no SPL ni amplitudes
de un piano sostenido. La ventana de 4 ms no permite elegir un timbre o
evaluar el decaimiento. No se creó un WAV a partir de ella.

## Convergencia de superficie, retardo y fuente

Comparación contra 96×72 puntos de Gauss, sobre la misma geometría y fuente:

| Cuadratura | Error RMS complejo de transferencia | Error de presión de esta fuente |
| --- | ---: | ---: |
| 16×12 | 0,90481% | 2,43022% |
| 32×24 | 0,09902% | 0,22774% |
| 64×48 | 4,679e-15 relativo | 8,887e-15 relativo |

64×48 pasa el criterio de 0,1% frente al siguiente refinamiento. La coincidencia
en este espacio modal finito no mide la exactitud de la geometría real.

Kernel temporal contra la integral continua, en 60–800 Hz:

| Tasa | Error de transferencia | Error de presión de esta fuente |
| --- | ---: | ---: |
| 48 kHz | 0,22458% | 0,27436% |
| 96 kHz | 0,05492% | 0,06208% |
| 192 kHz | 0,01368% | 0,01628% |

La salida de presión cambia **0,01020% RMS complejo** al pasar la mecánica
48→72, con el mismo observador continuo. Pasa el criterio de 1% en este
caso/banda. Estos ensayos no habilitan una banda audible completa ni sustituyen
una verificación del filtro de remuestreo en una implementación de tiempo real.

Dos ablaciones con la misma aceleración muestran sensibilidad a la proyección:
quitar los retardos cambia 165,78% RMS complejo; congelar cada transferencia
en la frecuencia natural de su modo cambia 400,96%. **No son porcentajes de
volumen**, errores contra el Steinway o mediciones del observador de fábrica.
Indican que fase y proyección de superficie importan incluso sin cambiar
la vibración. No se selecciona un candidato por esos valores.

## Hallazgo adicional: presupuesto de radiación

La potencia media de un movimiento armónico se calcula como
`P = 0,5 V* R_ac V`, en vatios. R_ac actúa sobre velocidades modales
intrínsecas y tiene unidades s^(-1). Se construye mediante una integral
angular positiva del campo lejano en el semiespacio. El refinamiento angular
16×32→32×64 cambia la matriz 6,771e-8 relativo en las frecuencias probadas.

Para diagnosticar la escala se excita **el banco P2 solo** con una fuerza
armónica de amplitud 1 N en el punto de C4, conservando su pérdida 0,046 omega.
No es la captura transitoria anterior ni una medición del plugin de fábrica.

| Frecuencia | Potencia radiada | Potencia viscosa modelada | Radiación / pérdida |
| --- | ---: | ---: | ---: |
| 80 Hz | 0,000391245 W | 0,000556471 W | 70,31% |
| 261,626 Hz | 0,000070313 W | 0,000119723 W | 58,73% |
| 523,251 Hz | 0,000061095 W | 0,000297586 W | 20,53% |
| 784,877 Hz | 0,000060410 W | 0,000359905 W | 16,78% |

En esta geometría supuesta, el aire no es una carga disipativa pequeña frente
al amortiguamiento elegido. Algunas combinaciones modales tienen una razón
de resistencia acústica / amortiguamiento de hasta 4,059, aunque la fuerza
puntual probada no las excita de esa manera. No se puede identificar la pérdida
viscosa como presupuesto acústico suficiente para cualquier estado sólo porque
el cociente de la excitación puntual sea menor que uno.

**Tampoco corresponde sumar toda esa resistencia al 0,046 existente:** ese
amortiguamiento supuesto podría incluir ya radiación. Falta separar las
pérdidas del material y la transferencia al aire. La resistencia calculada
no incluye la parte reactiva de la impedancia acústica; usarla sola omitiría
energía almacenada en el aire y cambios dinámicos de carga.

En el [modelo completo de Chabassier, Chaigne y Joly](https://perso.ensta-paris.fr/~touze/PDF/Batwoman/chabassier-jasa.pdf),
el campo acústico forma parte del sistema acoplado. Nuestro observador todavía
no implementa esa interacción recíproca. Esta distinción queda como **P6**,
con evidencia numérica de escala para el piloto, sin atribuirle todavía el
déficit de cuerpo que escucha el usuario.

## Siguiente trabajo y límites conservados

1. Ensayo P6 reducido y recíproco tabla/aire, con resistencia y parte reactiva
   causales, pasividad y energía acústica explícitas. Validar primero una
   geometría analítica; después conectar el banco de tabla.
2. Declarar y probar la aproximación de pantalla rígida frente a geometría
   de ambas caras/borde. Separar amortiguamiento estructural y radiación para
   evitar contabilizar dos veces las pérdidas.
3. Extender la fuente SI y su banda/duración antes de producir audio: banco
   alto, sostenido, unísono y otros registros. No presentar una continuación
   lineal arbitraria o una ventana de 4 ms como nuevo piano completo.
4. Mantener abiertos masa de graves, Timoshenko, captura/apagadores/pedales
   y coste de tiempo real/WASM. Ningún mecanismo se da por identificado con WAVs.

## Reproducción y procedencia

Herramientas nuevas: `concert_grand_si_observer.py`,
`audit-concert-grand-si-observer.py` y `tests/test_concert_si_observer.py`.
El resumen JSON conserva hashes de entrada, activo, estados congelados,
fuentes/capturas y paquete, versiones del entorno y resultados completos.

El archivo `local/concert-grand-si-observer/build/source-observer.zip`
congela las tres fuentes nuevas, SHA256:
`2b47085e2b54eb8ace353a7d655259f06a2db6d46e5920637d82e856532d8ebf`.
Sus dependencias mecánicas están en el commit 346b6c8 y en el archivo de
fuentes de la etapa de puente móvil. El log de las seis pruebas se conserva.

Con NumPy/SciPy y un hilo BLAS:

```powershell
$inputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input'
$outputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-observacion-SI'
$bridgeDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-puente-movil'
$assetPath = 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz'
python -m unittest discover -s tools/tests -p test_concert_si_observer.py -v
python tools/audit-concert-grand-si-observer.py --phase source --input $inputDir --asset $assetPath --output "$outputDir/source48" --transverse 48 --frozen "$bridgeDir/p2-fine/60-124-P2-low-band-T48.npz"
python tools/audit-concert-grand-si-observer.py --phase source --input $inputDir --asset $assetPath --output "$outputDir/source72" --transverse 72 --frozen "$bridgeDir/p2-finest/60-124-P2-low-band-T72.npz"
python tools/audit-concert-grand-si-observer.py --phase summary --input $inputDir --asset $assetPath --output $outputDir --commit 346b6c8
```

Paquete preservado:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
