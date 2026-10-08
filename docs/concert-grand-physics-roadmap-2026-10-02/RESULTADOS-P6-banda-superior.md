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

Pendiente la ejecución sobre la fuente física. Comando desde la raíz:

```text
python tools/audit-concert-grand-source-band.py --report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-superior/fuente-carga-24k.json
```
