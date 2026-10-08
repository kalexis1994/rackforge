# P6 — Fuente de ventana finita y carga por encima de 6 kHz

Fecha: 2026-10-08. Continúa el coast de 512 kHz aprobado sólo para 0,5 ms.

## Diseño fijado antes de medir

- Autenticar el informe completo, sus fuentes/padres y la cápsula única
  del contacto de 96/192 modos, con su productor histórico exacto. Misma
  C4 supuesta, fieltro, 606 coordenadas, micrófonos y R; sin nuevo cache.
- Repetir 256/512 kHz durante 3–3,5 ms. Reproducir estado y ocho medidas
  mecánicas del coast con límite relativo 1e−10. No repetir el certificado
  angular: esta prueba no aprueba un balance nuevo ni el contacto completo.
- Medir transformadas de velocidad y aceleración de la tabla observada,
  rectangular y Hann. La ventana multiplica **V y A reales**; no derivar
  una posición enventanada ni introducir impulsos en los extremos.
  Hann cambia la observación, nunca la dinámica o la ganancia del modelo.
- Integrales de norma espectral en 0–6/6–12/12–24/0–24 kHz, malla de
  125 Hz contra 250 Hz; diferencia de integración <0,1%. Comparar fuente
  256→512 en ambas ventanas/bandas, <0,1%. Cuadratura temporal de V con
  órdenes 8/12, <1e−10. A usa momentos analíticos y modulación Hann exacta.
- Sondas a 6/8/10/12/16/20/24 kHz. Presión compleja del mismo Green de
  celdas, sin EQ/ambiente: cuadraturas 4/8, <0,02%; captura 256→512 <0,1%.
- La dirección de cada sonda de carga es el vector complejo de la
  transformada Hann de V, normalizado en las 606 coordenadas. Comparar
  acción de Z causal con Rayleigh continuo. Conservar el umbral heredado
  de 2%, sin suponer que una dirección aprueba el operador completo.
  Controlar cuadratura Rayleigh 8/12 (<0,02%) y celdas 256×192 contra
  512×384 (<2%). No construir R nueva ni matrices angulares completas.
- Hashes de fuente, informes, tabla, R y cápsula de nuevo al terminar.
  Historias sólo en RAM; informe numérico sólo en repo. Global falso.

## Alcance

Una ventana de 0,5 ms tiene extremos vivos. Sus integrales espectrales son
normas de esa observación, **no energía de una nota completa**, ni conteos
de modos propios físicos. La norma de velocidad mass normalized de tabla
no equivale a presión audible; la aceleración tampoco. Comparar rectangular
con Hann detecta sensibilidad a los extremos, pero no demuestra por sí
solo ausencia/presencia de modos altos. La dirección Hann de V sirve como
sonda armónica de carga; Z aplicada a ese vector no reconstruye el aire
temporal con memoria viva ni el flujo de este tramo.

La tabla proyectada, cuerda y geometría siguen supuestas. Ninguna sonda
por encima de 6 kHz amplía automáticamente la banda aprobada 60–6000 Hz,
ni valida el instrumento real, release, unísono o timbre. Sin WAV ni cambio
de plugin en esta fase.

## Resultados

Cuatro pruebas pasan (0,103 s): trayectoria cúbica con extremos vivos contra
cuadratura independiente, fase al trasladar el tiempo; velocidad constante
sin fuerzas artificiales al enventanar; presión proyectada contra transferencia
completa; integración de bandas con sus extremos obligatorios. La prueba
analítica usa cuadratura más alta para su intervalo deliberadamente grande;
el diagnóstico físico controla órdenes 8/12 en cada paso nativo.

Comando desde la raíz:

```text
python tools/audit-concert-grand-source-band.py --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-superior/fuente-carga-24k.json
```

Primera ejecución completa: **6/9 controles**, 130,549 s. Fallan captura de
fuente, integración en frecuencia y captura de presión. Se conserva el
[informe](medidas-P6-banda-superior/fuente-carga-24k.json) y sus límites.
Reproducción física exacta; cuadraturas de tiempo, micrófono, carga y
refinamiento de celdas pasan. No se aprueba todavía el diagnóstico numérico.

En Hann, la banda 12–24 kHz de A cambia **0,348854%** entre 256/512
(límite 0,1%); la sonda de presión a 24 kHz cambia **0,756371%**.
La integración 250→125 Hz cambia hasta **1,236941%** en la pequeña cola
Hann de 6–12 kHz, frente a 0,1%. No eliminar esa cola ni relajar límites.

La acción de carga supera 2% desde 10 kHz: **2,06477 / 3,34065 /
21,2811 / 52,0742 / 69,4099%** a 10/12/16/20/24 kHz sobre las mismas
celdas. Es un diagnóstico direccional del aproximante vigente, no un fallo
de pasividad o una medida de error audible. Debe reproducirse con fuente
refinada antes de concluir. Los valores de ventana inicial son provisionales.

## Refinamiento fijado después de esos fallos

- Conservar el primer informe y la herramienta, sin alterar sus fuentes.
  Autenticarlos antes de continuar desde la misma cápsula/carga.
- Tres continuaciones nativas **512/1024/2048 kHz**, mismas condiciones
  y 0,5 ms; comparar finalmente 1024→2048. Esto refina integración
  mecánica y captura Hermite conjuntamente, no sólo un muestreo posterior.
  No se aprueba un balance angular nuevo a esas tasas.
- Tres grillas de frecuencia **125/62,5/31,25 Hz**, mismos extremos;
  comparar finalmente 62,5→31,25. Mantener **todos los umbrales**.
- Reproducir física, ocho integrales espectrales por ventana y presión
  de la ejecución anterior a 512 kHz en su grilla de 125 Hz (<1e−10).
- Repetir las sondas de micrófono/carga con la fuente más fina y ambas
  superficies; no heredar convergencia direccional de una fuente distinta.
  Conservar comparación contra la tasa gruesa, no usarla para aprobar.
- Mismo control de hashes al terminar, sin R, WAV o historias nuevas.

```text
python tools/audit-concert-grand-source-band-refinement.py --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-superior/fuente-carga-24k-refinado.json
```

## Resultados del refinamiento

[Informe completo](medidas-P6-banda-superior/fuente-carga-24k-refinado.json):
**9/9 controles de diagnóstico**, 407,429 s (6,79 min), pico **3,323 GiB**.
Global falso. Las medidas físicas y las 16 integrales espectrales anteriores
a 512 kHz se reproducen con error **0**; también la presión. Consistencia
de los cuatro estados con el operador: **0**. Hashes verificados dentro
de la ejecución y otra vez al terminar, con criterios idénticos al padre.

| Control fino | Medida relativa máxima | Límite |
| --- | --- | --- |
| Fuente 1024→2048, V/A, ambas ventanas/bandas | 0,0215987% | 0,1% |
| Presión 1024→2048, siete sondas | 0,0455909% | 0,1% |
| Integración en frecuencia 62,5→31,25 Hz | 0,0776924% | 0,1% |
| Cuadratura temporal 8/12 de V | 3,58737e−14 | 1e−10 |
| Cuadratura de micrófono 4/8 | 1,13380e−11 | 0,02% |
| Cuadratura de carga 8/12 | 1,77465e−11 | 0,02% |
| Celdas de carga 256×192→512×384 | 1,82614% | 2% |

Los controles gruesos siguen sin pasar: fuente Hann 12–24 kHz,
512→2048, **0,108009%**; presión a 24 kHz **0,230725%**;
integración Hann 6–12 kHz, 125→62,5 Hz, **0,310459%**. No se usan
para aprobar ni se ocultan. Tabla en 257 tiempos nativos comunes a
512 kHz: errores 512→2048 **4,31821e−7**, 1024→2048 **8,31448e−8**,
razón **0,192545**, sin interpolar; diagnóstico adicional.

### Sensibilidad a los extremos de la ventana

Fracción de la norma espectral integrada entre 6 y 24 kHz respecto a
0–24 kHz, a 2048 kHz y grilla de 31,25 Hz:

| Fuente de tabla | Rectangular | Hann |
| --- | --- | --- |
| Velocidad | 2,36349% | 0,00645364% |
| Aceleración | 2,91969% | 0,0360188% |

Las integrales de norma de V son **1,93003e−8 / 7,51540e−9** y las
de A **0,170861 / 0,0625512**, rectangular/Hann. No se igualan ganancias
entre ventanas: Hann atenúa también contenido legítimo. Estos resultados
demuestran sensibilidad importante a los extremos del tramo, sin demostrar
que todo el contenido alto sea artificial o que falten/sobren modos físicos.
Los tiempos de la transformada son locales: cero corresponde a los 3 ms
del contacto. Las sondas complejas conservan esa referencia de fase.

### Error de acción de carga con la fuente refinada

Cada sonda usa las 606 coordenadas. La dirección de V cambia como máximo
**1,79371e−5** entre 1024/2048; repetir todas las integrales confirma el
resultado. Sin modificar R, relajación, fieltro, geometría o parámetros:

| Frecuencia | Z causal frente a Rayleigh, mismas celdas | Frente a celdas refinadas |
| --- | --- | --- |
| 6 kHz | 0,512289% | 2,02431% |
| 8 kHz | 1,15157% | 1,17136% |
| 10 kHz | 2,06477% | 2,03209% |
| 12 kHz | 3,34065% | 3,31498% |
| 16 kHz | 21,2811% | 21,2565% |
| 20 kHz | 52,0744% | 52,0174% |
| 24 kHz | 69,4099% | 69,3497% |

El límite de 2% se supera desde 10 kHz sobre la malla vigente; contra la
malla refinada también lo supera ligeramente la sonda de 6 kHz. La malla
y el aproximante producen errores distintos: conservar ambas columnas.
Pasar los nueve controles de diagnóstico **no significa pasar estas sondas**
ni extender la certificación completa de carga a la banda alta. Tampoco se
ha aislado qué parte del error corresponde al factor espacial/angular, al
lift Padé o a la relajación que completa sus límites; no atribuirlo sin un
ensayo adicional. La carga sigue pasiva por su construcción aprobada.

## Decisión y siguiente paso

El diagnóstico ya es reproducible y converge con los mismos límites;
conservar los tres fallos iniciales y sus controles gruesos. La cola alta
del coast tiene fuerte sensibilidad a la ventana y el aproximante de
carga tiene errores direccionales grandes a 16–24 kHz. **No tenemos aún
una medida de su efecto audible**, ni una explicación comprobada de la
falta de cuerpo frente al Steinway.

Siguiente: medir espectro de fuente y presión del **contacto completo
0–3 ms**, con resolución de contacto autenticada, extremos vivos y los
controles de ventana/captura aquí probados, antes de elegir una banda nueva
o rediseñar la carga. La cápsula no contiene esa historia: requiere una
reconstrucción del contacto o una eliminación lineal validada. Coordinarlo
con el ledger angular del contacto, cuyo presupuesto de memoria sigue
pendiente; no guardar una historia de 27 GiB. Después duración/release,
unísono, identificación real y escucha. Sin WAV, duplicados en SHARED ni
cambio de fábrica/paquete instalado 0.172.0.
