# P6: presión SI de los contactos con 35 y 59 modos

Fecha: 2026-10-04. Base: `42e0b3c`, banco de contactos compactos de 59 modos.
Se observa la presión producida por las trayectorias congeladas, sin alterar
sus fuerzas, estados o ganancias. El plugin 0.172.0 no cambia.

## Qué se mide

La misma superficie de celdas constantes 128×96 utilizada para la carga del
aire emite hacia un semiespacio con bafle rígido infinito. La presión es

`p(r,t) = rho/(2*pi) * integral_surface[a(x,t-R/c)/R] dS`.

La aceleración de la superficie es la combinación de las formas modales
en kg⁻¹/² y aceleraciones intrínsecas en sqrt(kg)·m/s². La salida temporal
tiene unidades Pa; sus transformadas aquí medidas tienen unidades **Pa·s**.
No se añade una carga acústica adicional: las trayectorias mecánicas ya
incluyen su puerto recíproco. No se añaden caja, EQ, sala ni detune al leerlas.

Se integra `exp(-i*omega*R/c)/R` dentro de cada celda con cuadratura Gauss,
conservando distancia y fase completas. No se congela cada modo en una fase
evaluada sólo a su frecuencia propia ni se toma el centro de la celda como
un punto radiante equivalente.

Las primeras dos cápsulas están en (0,55; 0,50; 0,66) y (0,85; 0,50; 0,66) m,
como en el ensayo P3 SI anterior. Dos posiciones adicionales conservan x/y
y elevan z a 2 m, como control separado. **z es altura sobre la tabla**, no
la distancia de todas sus regiones al micrófono. Ambas bases usan exactamente
las mismas posiciones, rho=1,2 kg/m³, c=343 m/s y ganancia 1, sin normalización.
Estas posiciones y la geometría P2 son hipótesis, no datos identificados del Steinway.

## Fuente y continuidad

Los datos son los cuatro contactos de cada base: frío y relanzado a 384/768 kHz,
con captura física de B/V a 128 kHz. Se reconstruye B con interpolación Hermite
C¹ usando posición y velocidad; la integral de su aceleración se calcula con
momentos exponenciales analíticos estables también en frecuencia cero.
No se diferencia una señal de velocidad con un filtro elegido a oído.

El primer y segundo golpe duran 4 ms cada uno. Sus posiciones/velocidades de
tabla coinciden bit a bit en la unión, y la secuencia conserva sólo una copia
del nudo común. El espectro de 8 ms coincide con `A_frio + exp(-i*omega*4ms)*A_reiterado`
con error máximo 6,67e−15. En DC, A es V(T)−V(0), sin impulsos de inicio/final.
Esto impide introducir un click artificial al tratar el segundo golpe como
una nota nueva que parte de velocidad cero.

**Son ventanas finitas de aceleración de 4/8 ms**, no notas sostenidas ni
grabaciones completas. No se publica SPL ni se infiere percepción auditiva
desde una norma espectral. El corte de la ventana no simula soltar/apagar
la nota, y no se ha generado audio de esas ventanas.

## Precisión y controles

Los ocho gates pasan. Los umbrales estaban definidos antes de medir:
cuadratura de celdas 1e−5; transferencia y presión espacial 1%; captura
de fuente 0,1%; refinamiento mecánico de presión 1%; unión, subespacio y
descomposición 1e−10. La banda declarada es 60–6000 Hz.

| Control | Resultado máximo | Límite |
| --- | ---: | ---: |
| Factor de presión de los 35 modos comunes | Idéntico | 1e−10 |
| Gauss por celda, orden 4 contra 8, siete frecuencias | 8,63e−13 | 1e−5 |
| Transferencia: celdas 128×96 contra 192×144, bandas ponderadas | 0,216448% | 1% |
| Presión de fuentes: celdas 128×96 contra 192×144 | 0,114618% | 1% |
| Captura: 64 contra 128 kHz, interpolación de la misma trayectoria | 0,000286% | 0,1% |
| Trayectorias mecánicas: 384 contra 768 kHz | 0,313885% | 1% |
| Unión de fuentes | 6,67e−15 | 1e−10 |
| Descomposición en modos comunes y añadidos | 1,31e−15 | 1e−10 |

Una integral independiente sobre las formas continuas de la placa, con
Gauss global 96×72/128×96, cambia 6,86e−15 en los siete puntos. La superficie
de celdas difiere hasta 0,464047% de esa placa continua. Se registra esa
aproximación; el observador principal sigue leyendo la superficie de su carga,
no se sustituye silenciosamente por otra.

La integración de métricas se refina de pasos de 20 a 10 Hz. La mayor diferencia
es **0,070683 puntos porcentuales**, en la banda 800–1200 de la secuencia;
la métrica global cambia mucho menos. Es un diagnóstico adicional de integración,
no un cambio de ganancia ni un gate de convergencia estructural.
Pasaron 13 tests: siete nuevos de celdas/fuente y seis del observador SI anterior.
Incluyen integral rectangular independiente, límite de fuente puntual,
unidades/distancia, modos con signo, polinomio cúbico, ausencia de impulsos,
unión de golpes, refinamiento sinusoidal y geometría inválida.

## Diferencia 35 → 59 modos

La diferencia es una norma compleja ponderada por frecuencia:
`sqrt(integral |P59-P35|² df / integral |P35|² df)`. Incluye magnitud y fase;
no equivale a subir ese porcentaje el volumen. Se suman las dos cápsulas
en la norma, sin mezclarlas antes. Tabla con integración refinada a 10 Hz:

| Presión del par cercano | Primer golpe | Segundo golpe | Secuencia 8 ms |
| --- | ---: | ---: | ---: |
| Diferencia, 60–800 Hz | 7,387% | 3,896% | 4,401% |
| Diferencia, 800–1200 Hz | 29,401% | 26,798% | 38,941% |
| Diferencia, 1200–6000 Hz | 29,036% | 34,908% | 46,771% |
| Diferencia, 60–6000 Hz | **16,992%** | **10,342%** | **11,826%** |

La norma global de P59/P35, sobre la grilla de 20 Hz, es 1,02954 / 1,00560 /
1,01095. En la secuencia, la diferencia compleja de 11,83% convive con un cambio
de sólo **+1,10%** de norma global. Hay redistribución de la magnitud entre
frecuencias y cambios de fase, además del cambio de energía observada.

La separación exacta de diferencias cuadráticas usa
`|a-b|²=(|a|-|b|)²+2|a||b|*(1-cos(arg(a)-arg(b)))`:

| Contribución relativa en el par cercano, 60–6000 Hz | Primer golpe | Segundo golpe | Secuencia |
| --- | ---: | ---: | ---: |
| Magnitud espectral | 10,255% | 7,661% | 8,333% |
| Término de fase | 13,550% | 6,949% | 8,391% |

Sus cuadrados suman la diferencia compleja cuadrática; no son dos ganancias
que se suman. Se calculan de los espectros guardados sin modificar la fuente.

La diferencia global de la secuencia al promediar L/R antes de medir baja a
**4,408%**. A altura 2 m, usando las dos cápsulas sin promedio, es **13,185%**.
El resultado depende del reparto espacial y de la propagación, además del
movimiento local del puente. Es una diferencia en este campo calculado,
no una medida universal de anchura estéreo ni una preferencia de escucha.

La presión se descompone coherentemente en `P_comunes59 + P_24nuevos`.
El cambio de los modos comunes sólo aporta 0,327% de diferencia global frente
a P35; la norma de la contribución nueva es 11,795% de P35. Los términos
interfieren: no corresponde sumar sus energías como fuentes independientes.
Los 24 modos se seleccionan por la misma física P2, sin ajustar sus coeficientes
para este resultado. Su excitación efectiva sigue la proyección del contacto;
los modos nodales no se fuerzan artificialmente en este banco.

## Decisión y siguiente paso

La diferencia local de velocidad medida antes sí produce un cambio de presión.
El efecto supera la incertidumbre numérica de estos controles y se concentra
en las bandas donde el banco anterior estaba truncado. No demuestra cercanía
al Steinway ni convergencia de la tabla con 59 modos. Tampoco prueba que sólo
faltara radiación o que deban endurecerse/ablandarse los martillos.

Siguiente: ampliar P2 a **90 modos bajo 1500 Hz**, recalcular/verificar su carga,
repetir los contactos y esta observación con los mismos parámetros. Medir si
la presión se estabiliza antes de seleccionar una base para escucha. Después
quedan notas/intensidades, sostenido completo y coste de implementación.
Pérdidas identificadas, ambas caras/borde, unísono, P4, graves y acción/pedal
siguen pendientes. El paquete de fábrica permanece sin cambios.

## Archivos únicos y reproducción

Informe, resumen y fuentes sólo en el repo. Los datos derivados únicos están
en `C:/SHARED/Fisica-pendiente-2026-10-04/P6-observacion-1200/spectra.npz`:
transferencias, transformadas de aceleración, presión y sus contribuciones.
No se copian las trayectorias mecánicas ni los coeficientes de aire, ni se
generan WAV/CSV, informes duplicados o ZIP de fuentes. NPZ: 5,20 MB.

```powershell
python tools/audit-concert-grand-cell-observation.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --old-report docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-contacto-compacto.json --new-report docs/concert-grand-physics-roadmap-2026-10-02/resumen-P6-contacto-compacto-1200.json --old-data C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto/batched --new-data C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto-1200 --output C:/SHARED/Fisica-pendiente-2026-10-04/P6-observacion-1200
```

Para repetir, elegir `--output` y `--report` nuevos. El banco rechaza
reemplazar datos/resumen medidos; utiliza un hilo BLAS y registra versiones,
huellas de entradas, trayectorias, fuentes y medición. Las tablas secundarias
de magnitud/fase se reproducen aplicando la identidad anterior a los arrays
`pressure_M35-R768000-*` / `pressure_M59-R768000-*` del NPZ, con trapecios.
