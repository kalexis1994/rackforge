# P3/P5 — Contacto, cuerda y puente móvil con energía compartida

Fecha: 2026-10-03. **Piloto mecánico validado en el alcance indicado; sin
candidato audible nuevo, refit, instalación o commit.**

Datos, trazas, estados y resumen:
`C:\SHARED\Fisica-pendiente-2026-10-03\P3-puente-movil`.
La fábrica 0.172.0 conserva su paquete y sus rutas audibles anteriores.

## Qué se implementó

Se prolongó el núcleo SI de contacto no lineal con un extremo vertical móvil:

`v(x,t) = suma(q_n(t) sin(n pi x/L)) + (x/L) b(t)`.

El extremo izquierdo sigue fijo. El puente se mueve verticalmente; el
movimiento axial y la segunda dirección transversal tienen extremos fijos.
La cuerda conserva flexión Euler y el intercambio transversal/longitudinal del
potencial compartido. No es una cuerda Timoshenko ni un unísono completo.

Los modos de soporte B tienen masa intrínseca normalizada:
`b = phi · B`, con phi en kg^(-1/2). Mover el extremo transporta también
masa de cuerda. La matriz cinética en las coordenadas físicas z contiene:

- Bloque de cuerda `I`.
- Bloques cruzados `C = c phi^T`, con
  `c_n = mu L (-1)^(n+1)/(n pi sqrt(m_modal))` en la dirección vertical.
- Bloque de soporte `I + (mu L/3) phi phi^T`.

Una transformación por Cholesky normaliza **esa matriz completa**, sin
descartar la inercia cruzada. El potencial suma tensión, flexión, estiramiento,
fieltro y elasticidad intrínseca del soporte. El mismo desplazamiento de
contacto produce las fuerzas sobre martillo, cuerda y soporte. El soporte
devuelve movimiento a la cuerda durante toda la integración; no se inicializa
después del golpe con energía adicional.

La huella de contacto es una gaussiana positiva normalizada sobre [0,L]. Su
primer momento determina también el término del extremo móvil. Cerca de un
borde, truncarla cambia ese momento. Es una geometría supuesta de contacto.
Los controles rígidos usan **la misma huella nueva**: una comparación directa
contra el piloto anterior no aislaría sólo el puente.

## Controles y pruebas

Ocho pruebas independientes pasan:

1. Masa contra integrales espaciales de velocidad y transformación cinética.
2. Potencial contra integrales espaciales; gradiente/Hessiano por diferencias.
3. Convergencia espacial y reciprocidad de la huella de contacto.
4. Ecuación de fuerza del soporte y potencia conjugada `R * velocidad(b)`.
5. Proyección nula: recupera el contacto fijo de referencia, sin excitar soporte.
6. Pérdidas del fieltro y soporte: cada paso contabiliza el calor positivo.
7. Sistema lineal contra exponencial matricial independiente; reciprocidad,
   pasividad y refinamiento temporal de orden dos.
8. Soporte de varios modos, con proyecciones positivas/negativas: masa,
   derivadas, disipación y fuerza/potencia consistentes.

Las corridas de parámetros nativos emplean DOP853 independiente, 4 ms desde
reposo, rtol 1e-9, atol 1e-13, y paso máximo de un octavo del período de la
frecuencia estructural lineal más alta. El trabajo se integra como un estado
del solver; no se estima con las 401 filas de las trazas.

Error relativo máximo de energía + calor frente a energía entrante:
**1,423e-13**. Error máximo de energía del soporte + calor frente a su trabajo:
**3,581e-17** de la energía entrante. Residuo de fuerza máximo: **2,843e-14 N**.
Estos son controles del sistema discretizado, no exactitud respecto al piano.

## Soporte elemental: separar el mecanismo de la geometría

Primero se usa un oscilador supuesto de 0,25 kg y 180 Hz. Tres controles:
extremo rígido, soporte conservativo y soporte con pérdida modal 0,05
(`c = 0,05 omega`). C4 a velocidades 60/124 y C5 a 124: nueve corridas,
12 modos transversales y 24 axiales. Sirven para atribuir el efecto; esa
resolución pequeña no es el resultado final aceptado de C4 fuerte.

Se refinó C4 fuerte a 12/24/48/72 modos transversales y 2N axiales. Criterios:
energía de cuerda/soporte y trabajo <0,2%; tiempo de contacto <0,1%; RMS
de desplazamiento/velocidad de puente <1%, respecto al siguiente refinamiento.

| Refinamiento | Energía cuerda | Trabajo soporte | Tiempo contacto | RMS desplazamiento | RMS velocidad | Pasa |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 12→24 | 1,3251% | 0,6949% | 0,7274% | 1,3326% | 1,8366% | No |
| 24→48 | 0,2209% | 0,1022% | 0,1028% | 0,2157% | 0,2858% | No |
| 48→72 | 0,00433% | 0,00198% | 0,00202% | 0,00424% | 0,00560% | Sí |

Conservar energía a 12 modos no hacía converger su trayectoria. Los resultados
de resolución del extremo fijo no se transfirieron automáticamente al móvil.
La fuerza de interfaz sin filtrar cambia 0,01371% RMS en el último par, en
las 401 muestras de diagnóstico. No es una prueba de convergencia acústica
ni de toda la fuerza continua de alta frecuencia.

## Conectar la tabla física P2

Se conectaron los **35 modos estructurales bajo 800 Hz** del activo previo
`Panel-costillas-puente.npz`, sin complemento alto sintético. Se usa su masa
normalizada y la forma modal en x=0,642069 m, y=0,5 m para C4. Las pérdidas
modales son 0,046. Placa, nueve costillas y puente recto conservan geometría
supuesta; no son una identificación del Steinway.

Hash del activo:
`5469b9921b41ce4e3b752478e108c4a27be164306f5025317cf596bbb5f88fcd`.

En C4 fuerte, 12→48 no pasa por energía de cuerda, tiempo y movimiento del
puente. **48→72 pasa**: energía de cuerda 0,00429%, soporte 0,000301%,
trabajo 0,000802%, tiempo 0,00202%, desplazamiento 0,00436% y velocidad
0,00824% RMS. Esto refina la cuerda con ese banco bajo fijado; no prueba
convergencia del banco de tabla completo ni del resto del teclado.

Presupuesto de C4 fuerte a 72 modos, en 4 ms, con 0,046933719 J entrantes:

| Magnitud | Oscilador supuesto | Banco bajo P2 |
| --- | ---: | ---: |
| Energía cuerda | 0,028063436 J | 0,028153740 J |
| Energía soporte | 0,000236365 J | 0,000148060 J |
| Calor soporte | 0,000025684 J | 0,000023685 J |
| Trabajo neto recibido por soporte | 0,000262049 J | 0,000171745 J |
| Energía martillo saliente | 0,018608233 J | 0,018608233 J |
| Contacto hasta martillo retrocediendo | 1,816848 ms | 1,816848 ms |
| Desplazamiento máximo puente | 28,198 micrómetros | 9,750 micrómetros |

En este golpe el soporte recibe muy poca energía antes de separarse el
martillo (máximo del orden de 1e-7 J); gran parte del intercambio aparece
después. La reacción cambia mínimamente el contacto. No usar este caso
como evidencia de que el puente durante el contacto explica por sí solo
el cuerpo ausente que escucha el usuario.

## Ablación sin reacción: una fuente externa que debe contabilizarse

Se añadió un control deliberadamente distinto: cuerda de extremo fijo cuya
fuerza mueve un soporte adicional sin devolver movimiento. El trabajo sobre
ese soporte viene de un driver externo, y **no sale de la cuerda de control**.

En C4 fuerte a 12 modos, aporta 0,000282358 J adicionales: 0,60161% de la
energía inicial del martillo. Su presupuesto cierra al contar ese trabajo
externo. Es una ablación del piloto; **no una medición de ese error en la
fábrica**, ni un fallo del integrador. La versión cerrada elimina esa fuente
externa y verifica el trabajo entre subsistemas con fuerza y velocidad físicas.

## Alcance pendiente y siguiente ensayo

1. Construir un observador de presión a partir del movimiento SI de la tabla,
   con la misma superficie/formas modales y una ganancia común entre controles.
   No reutilizar directamente ganancias empíricas por parcial de las voces.
2. Completar el banco alto de la tabla y declarar el rango útil del observador.
   Escuchar ataque, cuerpo y sostenido con referencia en varios registros.
3. Conservar estado SI en golpes sucesivos y conectar unísono/polarizaciones;
   resolver captura del martillo, apagadores y pedal. Estos ensayos fueron desde
   reposo con una sola cuerda y soporte vertical.
4. Corregir la dependencia artificial de masa con velocidad en los graves y
   completar Timoshenko. No ampliar el diagnóstico a F#2 sin esa corrección.
5. Medir coste y estabilidad de la ruta completa en tiempo real/WASM antes
   de llevarla a fábrica o volver a hacer refit.

## Reproducción y conservación

Fuentes nuevas: `tools/concert_grand_moving_bridge.py`,
`tools/audit-concert-grand-moving-bridge.py`,
`tools/summarize-concert-grand-moving-bridge.py` y su prueba Python.
El resumen JSON incluye hashes de fuentes, entradas, activo P2 y reportes,
versiones de Python/NumPy/SciPy, parámetros y criterios.

El archivo `local/concert-grand-moving-bridge/build/source-audit.zip`
incluye todas las dependencias Python usadas, las entradas CSV y el activo
P2. SHA256:
`874198a0ab37c4492cd34e3ab3638f6cca1548b501a4a74b98577d4bab91c432`.
El log de las ocho pruebas queda en la misma carpeta. No se recompiló ni
cambió el motor nativo en esta etapa; los controles nativos congelados
anteriores siguen conservados en `local/concert-grand-resolved-restrike/build`.

Para repetir, usar un entorno con NumPy/SciPy, un solo hilo BLAS y
`PYTHONDONTWRITEBYTECODE=1`. Ejecutar:

```powershell
$inputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input'
$outputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-puente-movil'
$assetPath = 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz'
python -m unittest discover -s tools/tests -p test_concert_moving_bridge.py -v
python tools/audit-concert-grand-moving-bridge.py --input $inputDir --output "$outputDir/scalar" --phase scalar
python tools/audit-concert-grand-moving-bridge.py --input $inputDir --output "$outputDir/resolution" --phase resolution
python tools/audit-concert-grand-moving-bridge.py --input $inputDir --output "$outputDir/resolution-fine" --phase resolution --resolution-modes 72
python tools/audit-concert-grand-moving-bridge.py --input $inputDir --output "$outputDir/p2" --phase p2 --p2-asset $assetPath
python tools/audit-concert-grand-moving-bridge.py --input $inputDir --output "$outputDir/p2-fine" --phase p2 --p2-asset $assetPath --transverse 48
python tools/audit-concert-grand-moving-bridge.py --input $inputDir --output "$outputDir/p2-finest" --phase p2 --p2-asset $assetPath --transverse 72
python tools/summarize-concert-grand-moving-bridge.py --root $outputDir --input $inputDir --build local/concert-grand-moving-bridge/build --p2-asset $assetPath
```

Paquete preservado SHA256:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
