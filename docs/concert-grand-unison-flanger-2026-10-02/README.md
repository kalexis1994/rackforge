# Diagnóstico del piloto rechazado: efecto parecido a flanger

El usuario rechaza **B**, el piloto I, en la comparación de C4 sin drenaje.
B no se instaló y se descarta. A sigue siendo el control anterior de investigación;
la calibración instalada conserva su afinación de fábrica.

## Prueba de causa

Mismo renderer congelado, eventos, programa Concert Grand, velocidad 124,
44,1 kHz, bloque 256, tecla sostenida 2 s y release 2 s. Sin normalización ni EQ.
En esta familia `DIAGNOSTIC_STRING_DRAIN=0`: se quita el drenaje resistivo,
pero la carga reactiva del puente sigue activa excepto en su ablación explícita.
Este control no representa una nueva física calibrada para instalar.

| Control | UNISON_WIDTH | UNISON_EXTRA_LOSS | REACTIVE_BRIDGE |
| --- | ---: | ---: | ---: |
| A | 0 | 2 | 1 |
| Sólo afinación | 0,65 | 2 | 1 |
| Sólo pérdidas | 0 | 1 | 1 |
| B rechazada | 0,65 | 1 | 1 |
| B sin reactividad | 0,65 | 1 | 0 |

`UNISON_WIDTH` es un multiplicador, no una diferencia en cents. Cada cuerda
conserva una afinación fija a través de su escalera de parciales. La separación
de sus frecuencias aumenta con el orden del parcial. No se agregó un LFO ni
un procesador flanger, pero la suma de estados cercanos puede producir un
movimiento de color espectral que el usuario describe de esa manera.

## Medición del movimiento relativo

Se demodulan H1..H16 hasta 10 kHz a la tasa original, incluyendo la energía
de ambos canales. Los parciales elegibles se seleccionan sólo por la referencia
(pico > máximo de −85 dB y pico más fuerte menos 35 dB). Se usa una frecuencia
de análisis común a todas las variantes nativas, obtenida de A.

Entre 250 y 1800 ms se mide el cambio de cada envolvente respecto de A, se
resta la mediana simultánea entre parciales y luego el promedio temporal de
cada parcial. Así se separa movimiento de balance de un cambio común de volumen
y de una diferencia tonal constante. La tabla muestra la mediana de los rangos
percentil 95 menos percentil 5, en dB. Es una descripción acústica relativa a A,
**no un umbral perceptivo ni un score de fidelidad al Steinway**.

| Control | C4 | C6 |
| --- | ---: | ---: |
| A, por definición | 0 | 0 |
| Sólo afinación | 2,329 | 9,177 |
| Sólo pérdidas | 0,250 | 0,953 |
| B rechazada | 7,644 | 13,951 |
| B sin reactividad | 4,120 | 10,032 |

La afinación separada mueve el balance mucho más que el cambio de pérdidas
solo. Su combinación lo amplifica. Quitar reactividad no elimina ese movimiento.
El diagnóstico coincide con el rechazo auditivo, sin demostrar que esta medida
identifique todos los sonidos que alguien describiría como flanger.

El cambio de pérdidas solo sube el nivel entre 0,5 y 1,5 s en 2,09 dB para C4
y 2,45 dB para C6. No se presenta ese aumento de volumen como recuperación de
cuerpo ni se propone ese control como candidato sin evaluar su decaimiento.

La auditoría anterior contaba mínimos con recuperación ≥3 dB y destacaba los
tardíos. B ya tenía mínimos tempranos; su ausencia después de 1,5 s no permitía
concluir que el color permanecía estable durante el ataque y primer sostenido.
Los dos cambios simultáneos de B tampoco permitían atribuir el resultado a uno.
Esta prueba corrige esas limitaciones. No se retoca la física basándose sólo
en el score medio favorable de B.

## Archivos y reproducción

`C:\SHARED\Unisono-diagnostico-flanger-2026-10-02` conserva los controles
individuales, las curvas completas, identidades y comparadores de C4/C6.
En cada comparador: A a 0 s; sólo afinación a 4,4; sólo pérdidas a 8,8;
B a 13,2; B sin reactividad a 17,6; Steinway a 22.
El único fade del comparador está en los últimos 10 ms de release.

Renderer SHA256:
`a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831`.
`results.json` conserva el resumen; `report.json` en SHARED conserva curvas y
manifiestos. A y B reutilizan las capturas congeladas previas.

```powershell
python tools/audit-concert-grand-unison-flanger.py --binary local/concert-grand-decay/build/renderer.exe --work local/concert-grand-unison-flanger/renders --output C:/SHARED/Unisono-diagnostico-flanger-2026-10-02
```

No se modifica DSP, fábrica, programa, paquete o instalación. No hay commit.
La siguiente propuesta necesita conservar la estabilidad de A y mejorar su
balance de cuerpo frente al original; reducir otro detune sin resolver esa
limitación no demuestra un avance.
