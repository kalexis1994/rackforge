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

Resultados pendientes. Mantener informes fallidos y aprobación global falsa.
