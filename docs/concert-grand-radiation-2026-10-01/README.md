# Radiación de la tabla bajo excitación forzada

## Qué se verificó

Se mantiene el renderer nativo congelado
`57a6c88d490e527f63ee9d9cfcdbb44a8bcd6b94f148a1c04a1fe7ca8fcb12e9`,
Concert Grand, 44,1 kHz, bloques de 256 y preparación de 200 ms. Los 167 modos
de tabla tienen las mismas masas positivas, pérdidas, proyecciones del puente
y formas de superficie en ambas observaciones. No se cambia fieltro, contacto,
detune, ataques mecánicos ni ganancias por nota.

`bench::export_physical_transfer` exporta los coeficientes reales del banco.
`tools/audit-concert-grand-radiation-transfer.py` reconstruye su admisión digital
y compara cuatro observaciones de esa misma velocidad modal:

- Dos muestras de velocidad, con los coeficientes actuales.
- La misma aproximación, retirando la fase del trayecto común altura/c.
- Integral de Rayleigh dependiente de la frecuencia de excitación.
- Integral causal discreta, con aceleración y retardos fraccionarios lineales.

La segunda es únicamente una ablación analítica, no un ajuste incorporado.
Para las dos últimas se retira el mismo trayecto común de todos los modos
de tabla. No se retiran retardos individuales por modo para igualar fases.

## Verificación numérica

- Error relativo global al reproducir los coeficientes nativos en sus propias
  frecuencias: **2,23 × 10⁻⁵**. Comprueba geometría, escala y signo de fase.
- Error relativo máximo de la admisión analítica frente a integración q/v
  sinusoidal independiente: **2,28 × 10⁻¹³**.
- Malla 80 × 48 frente a 40 × 24: cambio máximo **0,116 dB** en los niveles
  evaluados. Esta convergencia corresponde a la geometría congelada a 2,64 m;
  no garantiza resolución suficiente para cualquier micrófono muy cercano.
- Integral causal discreta frente a continua: máximo **0,834 dB** en los
  niveles evaluados. La aproximación incluye la diferencia de aceleración y
  la interpolación lineal de los retardos; no se declara equivalencia exacta.

Se evalúan H1, H2 y H3 de F#2/C3/C4/C5/C6/F#6/C7 con una fuerza sinusoidal
unitaria en el puente. Las frecuencias son temperadas; las notas reales del
motor tienen estiramiento e inarmonicidad. El ensayo no incluye cuerdas,
envolventes, rim, fuentes auxiliares ni sala. Sus niveles no son SPL calibrado
ni predicen por sí solos la calidad del instrumento.

En C6, la transferencia H2/H1 resulta −6,44 dB en la aproximación actual y
+6,46 dB en la integral causal discreta: aproximadamente **12,9 dB de cambio**.
En C4 ocurre un cambio de signo contrario. Por eso no es defendible corregir
C6 con una ganancia aislada ni activar el nuevo observador sin recalibración.
La aproximación de dos muestras sólo coincide en la frecuencia propia de cada
modo; el golpe y sus armónicos también fuerzan movimientos fuera de ella.

## Ensayo del sonido completo

Doce pares: F#2/C3/C4/C5/C6/F#6 a 60 y 124. Las raíces Steinway son exactamente
las grabadas, sin transposición, con el sampler Stage/Lid ORTF habitual. Dos
segundos sostenidos y dos de release. Exportación estéreo PCM24 a ganancia
unidad para todos; ninguna normalización por nota, EQ ni ajuste posterior.

Se activa únicamente `CAUSAL_RADIATION_FIR=1`. La ruta de tabla tiene el
trayecto común retirado; las fuentes auxiliares y la sala conservan sus rutas.
La comparación completa incluye esa nueva relación de fases y no se presenta
como una sustitución acústica completa de todos los caminos del instrumento.

| C6/124, 20–100 ms | Actual | Radiación causal | Steinway |
| --- | ---: | ---: | ---: |
| H1 | −20,43 dB | −33,45 dB | −17,07 dB |
| H2/H1 | −27,96 dB | −8,88 dB | −15,84 dB |

La relación mejora hasta sobrepasar la referencia, pero parte de ese cambio
proviene de debilitar H1. H2 sube alrededor de 6 dB mientras H1 cae 13 dB.
Esto **no resuelve el cuerpo**: una relación correcta necesita niveles y
envolventes correctos, no sólo una fundamental más débil.

| Error score v2 sobre doce pares; menor es mejor | Actual | Causal |
| --- | ---: | ---: |
| Promedio | 6,061 | 6,335 |
| Peor par | 9,066 | 11,210 |
| Grave | 3,844 | 3,562 |
| Medio | 6,406 | 5,754 |
| Agudo | 7,935 | 9,690 |

La prueba falla promedio, peor caso, todos los registros, fundamental tardía,
balance, bandas de ataque/cuerpo y regresión máxima por par. La regresión
máxima es 4,02 dB de score. No se selecciona, empaqueta ni instala. Estos doce
pares son un diagnóstico acotado; no sustituyen holdouts ni una prueba de MIDI.

## Resultado y trabajo siguiente

Hay evidencia de una limitación de la observación de radiación bajo fuerza
broadband. El filtro actual reproduce lo solicitado en la frecuencia modal;
no es un error de signo en ese punto. Aumentar la cuadratura tampoco explica
por sí solo las grandes diferencias medidas. No se demuestra que toda la
falta de cuerpo provenga de esta única limitación.

La próxima corrección debe validar juntos la representación espacial de la
tabla, su carga de cuerdas y su observación causal, con una geometría coherente
con la grabación. Una recalibración debe conservar las mismas masas para
movimiento y admisión, evaluar niveles y envolventes de H1/H2/H3, y pasar
registros/velocidades reservados antes de ofrecer otro candidato. El FIR actual
es nativo y diagnóstico; su costo y portabilidad deben resolverse antes de
usarlo en el plugin. La ganancia o ruido grave por nota no son una solución
derivada de este ensayo.

## Archivos y reproducción

`C:/SHARED/Radiacion-tabla-2026-10-01` contiene secuencias separadas del actual,
de la prueba causal y del Steinway, extractos C6/124, mediciones y agenda.
En las secuencias cada raíz se toca suave/fuerte antes de pasar a la siguiente;
C6/124 comienza a 27 s. Es la misma nota del caso de 57 s, ahora en una agenda
de sólo dos intensidades. No se modifica la entrega anterior.

Los informes se conservan aquí; CSV, binario y raws en
`local/concert-grand-radiation-audit/`.

```powershell
$env:CG_TUNING=(Resolve-Path local/concert-grand-radiation-audit/tuning.txt).Path
$env:CG_PHYSICAL_TRANSFER_CSV=Join-Path (Resolve-Path local/concert-grand-radiation-audit).Path 'bank.csv'
$env:RUST_MIN_STACK='33554432'
& local/concert-grand-current-fit/regional-build/renderer.exe bench::export_physical_transfer --exact --ignored --nocapture --test-threads=1
python tools/audit-concert-grand-radiation-transfer.py --bank local/concert-grand-radiation-audit/bank.csv --output local/concert-grand-radiation-audit
python tools/fit-concert-grand-current-physics.py --binary local/concert-grand-current-fit/regional-build/renderer.exe --expected-sha256 57a6c88d490e527f63ee9d9cfcdbb44a8bcd6b94f148a1c04a1fe7ca8fcb12e9 --work local/concert-grand-radiation-audit/render --output local/concert-grand-radiation-audit/audition --profiles local/concert-grand-radiation-audit/profiles.json --notes 42,48,60,72,84,90 --velocities 60,124 --export
```

`profiles.json` contiene
`{"radiacion-causal":{"tuning":{"CAUSAL_RADIATION_FIR":1},"params":{}}}`.
El entorno Python necesita numpy/scipy/soundfile y el sampler local ya usado.
No hay cambios de DSP ni calibración de fábrica derivados de este ensayo.
