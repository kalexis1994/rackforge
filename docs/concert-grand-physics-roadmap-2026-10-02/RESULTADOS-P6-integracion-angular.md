# P6: separar cuadratura angular y discretización temporal

2026-10-06. La construcción causal por bloques y su puerta de masa quedan
en **bee4f9c**, por pedido del usuario. Se conserva esa versión para reproducir
las corridas rechazadas. Esta etapa usa la misma tabla de 606 coordenadas,
superficie 256×192 y transformación modal; no identifica coeficientes nuevos.

## Agrupación de direcciones equivalentes

Esta geometría y excitación vertical centrada contienen sólo formas pares
en y. La reflexión de una pareja angular produce otra pareja equivalente;
su contribución puede agruparse con peso doble, o amplitud multiplicada por
`sqrt(2)`. Esto conserva las correlaciones y el certificado cuadrático de
energía/radiación. La realización mantiene el mismo número de estados y
todas las coordenadas mecánicas.

La opción `fold_y` comprueba la simetría de las formas de superficie antes
de agrupar; no las promedia para imponerla. Rechaza error relativo > 1e−12
y cuadraturas que no tengan las parejas correspondientes. La superficie
real da 2,133e−14, compatible con redondeo de su paridad estructural exacta.
La simetría no es aplicable automáticamente a una tabla real asimétrica,
acción fuera de ese eje, excitación horizontal o una futura geometría P4.

Dos pruebas nuevas verifican contra las direcciones completas:

- Correlaciones, impedancia, energía de estados activos y fuerza/radiación
  del paso midpoint; estados y trayectoria del cascade idénticos.
- Rechazo de superficie asimétrica y de cuadratura sin parejas equivalentes.

La masa por factor acumulado usa además `dsyrk`: se calcula un triángulo
del Gram real y se reconstruye el otro. Se conservan los términos mutuos
de la matriz. Las tres pruebas de masa anteriores pasan con esta operación.
En total pasan **ocho pruebas** de factor/certificado/masa (cinco y tres).

## Control sobre las 606 coordenadas

[Informe](medidas-P6-aire-bloques/masa96-tasas.json). Se exige reproducir la
cuadratura 32×64/128 kHz anterior antes de los refinamientos. El residual
mínimo se conserva (−0,0044861946); comparación de extremos propios contra
las correlaciones anteriores: 1,983e−14 relativa, debajo del límite 1e−10.

Esa masa tarda 34,860 s frente a 68,516 s en el control acumulado anterior.
Son dos corridas individuales con el mismo problema y operaciones algebraicas
equivalentes; no son estadísticas de rendimiento ni un benchmark de audio.

Después se ejecutan, en este orden:

1. Cuadratura **96×256**, factor **128 kHz**: cambia la integración angular.
2. Misma cuadratura **96×256**, factor **64 kHz**: cambia sólo la discretización
   temporal (histogramas y promedio de taps).

La tasa del factor es un parámetro del modelo de retrasos; no cambia la tasa
del driver de RackForge. Se mantiene la masa Rayleigh continua como objetivo.
El completado exige que el residual sea ya positivo con el mismo margen
relativo 1e−13 que la relajación anterior. No se reparan autovalores ni se
calibra la ganancia para forzar aceptación.

**Las dos tasas pasan la condición de masa.** Los controles anteriores
mantienen su rechazo; no se reparó ningún residual.

| Cuadratura | Tasa | Residual mínimo | Residual máximo | Tiempo de masa |
| --- | ---: | ---: | ---: | ---: |
| 32×64, control agrupado | 128 kHz | −0,0044861946 | 0,0057726914 | 34,860 s |
| 96×256 | 128 kHz | **1,28290e−5** | 0,0009724505 | 411,750 s |
| 96×256 | 64 kHz | **2,79279e−5** | 0,0014931297 | 275,860 s |

Al mantener tasa/celdas y refinar sólo las direcciones, el residual pasa de
indefinido a positivo. Esto identifica la cuadratura angular como causa
decisiva del rechazo observado a 128 kHz. El contraste temporal no vuelve
negativo el residual, pero modifica sus extremos propios; éstos no prueban
por sí solos un orden matricial entre las dos masas ni igualdad de transferencia.

Tiempo total 729,328 s; working set máximo 921.403.392 bytes (**0,858 GiB**).
Se evita reservar ~5,72 GB de Gram/correlaciones en los casos de 128 kHz y
~2,91 GB en el de 64 kHz. Se verificaron hashes de fuentes, informes padres
y tabla después de terminar.

La puerta evalúa masa DC; no construye Gram/correlaciones ni certifica
impedancia, límite resistivo, respuestas transitorias o timbre. El informe
mantiene `preflight_only=true` y `all_gates_passed=false` para distinguir
esta condición aprobada de una carga completa validada.

## Datos y reproducción

Se escribe sólo el informe pequeño en el repo y se usa la única tabla de
`local/`. Los ensayos anteriores conservan sus fuentes en bee4f9c y sus hashes
originales; no se les atribuyen las optimizaciones posteriores. No se generan
factores grandes, WAV o paquetes ni se dejan copias en SHARED.

Python/NumPy/SciPy, BLAS de un hilo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_streaming_air.py -v
python -m unittest discover -s tools/tests -p test_concert_wave_mass.py -v
python tools/audit-concert-grand-wave-mass.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --comparison-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/refinamiento32.json --case 128000 32 64 --case 128000 96 256 --case 64000 96 256 --fold-y --report local/concert-grand-physics/P6-angular-tasas-repeticion.json
```

## Siguiente

Construir la carga **96×256/128 kHz**, conservando la agrupación certificada
y el cambio de base de la tabla. Se comienza con 128 kHz para reducir el
error de discretización temporal; no se selecciona 64 kHz por su menor coste
sin medir su respuesta. Antes de reservar el Gram, usar la reciprocidad de
las parejas para evaluar un Gram espectral real y simétrico, con comparación
contra el complejo anterior y su certificado vivo.

Después verificar el completado resistivo, límites, transferencia continua,
semiplano derecho y transitorios. El paso siguiente sigue usando las mismas
masas/rigideces/pérdidas; no es otra identificación del piano. No se acopla
una onda incompleta al instrumento para escucha.

Actualización posterior: [Gram real y preflight completo](RESULTADOS-P6-Gram-real.md).
El completado finito y la pasividad pasan a 96×256/128 kHz. La precisión
todavía se rechaza a 6 kHz; esta aprobación de masa conserva su alcance DC.
