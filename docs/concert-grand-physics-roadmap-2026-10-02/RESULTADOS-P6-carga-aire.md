# P6 — Puerto acústico recíproco: almacenamiento y radiación

Fecha: 2026-10-03. La observación SI anterior quedó en el commit **aaa520d**
por pedido del usuario. Esta etapa valida una carga acústica recíproca con
una geometría analítica. No se seleccionó ni instaló un nuevo piano.

Resultados y trazas:
`C:\SHARED\Fisica-pendiente-2026-10-03\P6-carga-aire`.

## Geometría de control y unidades

Se eligió una **esfera pulsante** para disponer de una impedancia exacta.
No representa la tabla del Steinway ni sustituye el banco P2. El control
permite verificar resistencia, parte reactiva, signos, fuerzas recíprocas y
energía antes de aproximar una geometría de tabla más compleja.

Para radio a, área A=4 pi a², rho=1,2 kg/m³ y c=343 m/s, la impedancia
mecánica entre fuerza y velocidad radial es:

`Z(s) = R * s tau / (1 + s tau)`
`R = rho c A`, `M = rho A a`, `tau = M/R = a/c`.

Se contrastó con la solución saliente de potencial esférico y con la
[documentación oficial de impedancia de esfera pulsante](https://www.mathworks.com/help/audio/ref/sphericalsourceradiationimpedance.html).
Esa fuente expresa presión/caudal: sus valores rho c/A y rho a/A se
multiplican por A² para pasar a fuerza/velocidad. No se confundieron ambas
convenciones. El modelo tiene un polo estable en −1/tau, comportamiento
inercial de baja frecuencia y resistencia de alta frecuencia.

Una realización pasiva exacta para esta geometría es:

`F = R (v-u)`, `M u' = F`.

u es la velocidad del elemento inercial equivalente. Su energía almacenada
es `E_air = 0,5 M u²`, y la potencia en el canal de onda saliente es
`P_out = R (v-u)²`. En cada instante:

`F v = E_air' + P_out`.

La potencia de puerto Fv puede ser negativa cuando el elemento devuelve
energía almacenada. P_out permanece positiva. No se recorta la devolución
de energía ni se interpreta radiación como calor del material.

La presión exacta fuera de la esfera es la presión de frontera F/A,
multiplicada por a/r y retardada (r−a)/c. Se verifican fase y unidades;
no se introduce una ganancia para recuperar el cuerpo del piano.

## Acoplamiento con coordenadas SI

Una proyección constante G convierte la velocidad mecánica normalizada P
en velocidad de frontera v=G P. La reacción vuelve por **G^T F**. El estado
inercial se representa como W=sqrt(M) u, con energía 0,5 W^T W.
El operador disipativo conjunto se construye a partir de la velocidad
relativa `[G, -diag(1/sqrt(M))]`; es simétrico y semidefinido positivo.

El integrador del contacto conserva su potencial de martillo/fieltro/cuerda
y añade el elemento inercial sin energía elástica. El paso centrado contabiliza
por separado calor del material/fieltro, almacenamiento acústico y onda saliente.
El trabajo de puerto paga exactamente almacenamiento + salida en cada paso.

Para el control de contacto, la esfera se conecta al desplazamiento b del
soporte elemental anterior. G incluye la transformación de masa mixta de
la cuerda y soporte. No se descarta la inercia cruzada ni se conecta presión
directamente al martillo. El soporte tiene masa intrínseca 0,25 kg, 180 Hz,
y pérdida 0,05 que **en este ensayo se define como pérdida de material**.
Esa definición no identifica ni modifica las pérdidas de fábrica/P2.

## Ocho pruebas pasan

1. Impedancia y presión contra la solución saliente del potencial esférico,
   incluyendo límites inercial/resistivo y propagación.
2. Trabajo de fuerzas conjugadas y presupuesto instantáneo de puerto,
   con proyecciones de varios puertos y signos distintos.
3. Impedancia positiva real en el semiplano derecho, reciprocidad y simetría
   de frecuencias negativas.
4. Generador lineal contra exponencial matricial; identidad de energía y
   refinamiento de orden dos con el paso centrado.
5. Aire previamente excitado: devolución de energía al soporte con radiación
   positiva. El modelo no crea esa energía.
6. Contacto no lineal: energía total y trabajo de puerto contabilizados en
   cada paso AVF.
7. Proyección cero: recupera el contacto mecánico sin carga, con aire en cero.
8. Contacto no lineal con aire contra DOP853 independiente: el error de estado
   ponderado por energía cae aproximadamente cuatro veces al refinar y pasa
   el umbral de 0,1% en el último paso probado. Se usa un núcleo pequeño para
   verificar el integrador; la resolución de C4 se evalúa aparte.

## Control lineal: cargar y amortiguar producen respuestas diferentes

El radio supuesto de 0,18 m determina:

- Resistencia de alta frecuencia: 167,583108 kg/s.
- Masa acústica de baja frecuencia: 0,08794449 kg.
- Tiempo de memoria: 0,524781 ms.

Con igual estructura/material y excitación armónica de 1 N:

| Carga | Pico de respuesta de velocidad | Máxima velocidad por N |
| --- | ---: | ---: |
| Sin aire | 180,0 Hz | 0,0707355 m/s/N |
| Resistencia constante igual a Re Z(180 Hz) | 180,0 Hz | 0,0173034 m/s/N |
| Carga esférica completa | 158,2 Hz | 0,0199267 m/s/N |

El control resistivo es pasivo, pero tiene 43,6548 kg/s constantes y no
almacenamiento acústico. La carga completa tiene resistencia y reactancia
dependientes de frecuencia. El desplazamiento del pico refleja su respuesta
conjunta; no se atribuye toda esa diferencia sólo a la reactancia. No se
cambia la afinación de una nota del piano a partir de este control.

En un transitorio de 90 ms con 0,00005 J iniciales:

- Onda saliente: 0,00003830733 J.
- Calor del material: 0,00001169265 J.
- Queda una pequeña energía mecánica/acústica residual.
- Error máximo del presupuesto total: 8,810e-15 relativo.
- Error del presupuesto de puerto: 8,945e-15 relativo.
- Estado final contra exponencial matricial: 3,820e-17 ponderado por energía.
- 664/1801 muestras devuelven potencia desde el elemento inercial al soporte.

La energía almacenada pertenece a la realización pasiva equivalente del
puerto; no es una discretización espacial de todo el aire. El canal de salida
contabiliza energía transportada fuera del puerto. Tapa, borde, reflexión y
campo acústico de la tabla continúan sin modelar en este control.

## Contacto C4/124: conservar la fuente y pagar la carga

Se usaron los parámetros SI congelados y 4 ms de contacto/evolución, con
flexión Euler, extremos axial/horizontal fijos y una cuerda. A 48 modos
transversales y 96 axiales se midieron cuatro variantes:

| Variante | Energía soporte a 4 ms | Energía almacenada en aire | Onda saliente | Trabajo de puerto |
| --- | ---: | ---: | ---: | ---: |
| Sin aire | 0,000236360 J | 0 J | 0 J | 0 J |
| Esfera recíproca | 0,000116583 J | 0,000017728 J | 0,000094031 J | 0,000111760 J |
| Resistencia constante | 0,000184150 J | 0 J | 0,000070513 J | 0,000070513 J |
| Esfera sin reacción sobre la mecánica | 0,000236360 J | 0,000031756 J | 0,000145387 J | 0,000177143 J |

Sin aire, el estado final coincide **exactamente** con el archivo mecánico
congelado anterior. En la última variante el trabajo procede de un driver
externo: se mantiene la vibración sin pagar su emisión. Su presupuesto cierra
al contar ese trabajo externo; no es un fallo del integrador ni una medición
del error de fábrica.

La esfera recíproca sí toma su energía de la mecánica. Durante esa corrida
hay 35/401 muestras con potencia de puerto negativa, permitida por el
almacenamiento previo. La radiación no se hace negativa.

El ensayo recíproco se repitió a 72 transversales y 144 axiales:

| Magnitud | Diferencia 48→72 |
| --- | ---: |
| Energía cuerda | 0,00432% |
| Energía soporte | 0,00386% |
| Energía almacenada en aire | 0,00362% |
| Energía de onda saliente | 0,000717% |
| Trabajo de puerto | 0,00118% |
| Tiempo hasta martillo retrocediendo | 0,00202% |
| RMS desplazamiento/velocidad del soporte | 0,00404% / 0,00597% |

Pasa los criterios previos de 0,2% para energía/trabajo, 0,1% para tiempo y
1% para desplazamiento/velocidad. La corrida a 72 modos cierra el presupuesto
total a 8,280e-15 relativo, y el de puerto a 6,931e-18 de la energía entrante.
Su contacto dura 1,816848 ms: esta carga altera poco el contacto inicial y
afecta principalmente la respuesta posterior del soporte en la ventana.

## Lo que falta antes de cargar P2 y escuchar

1. Calcular la **impedancia compleja modal de la tabla** con la misma superficie
   que se observa. Verificar su parte real contra la resistencia angular
   anterior, y resolver la autointeracción singular de cada región de superficie
   para obtener correctamente la parte reactiva.
2. Construir una realización causal/pasiva de esa impedancia, incluyendo
   términos entre modos. La familia resistencia/inertancia probada tiene
   respuesta resistiva monótona por puerto; no basta para representar toda
   la interferencia y dinámica de una placa. No asignar una esfera por nota
   o modo como una identificación automática del Steinway.
3. Separar pérdidas estructurales y radiativas. Si frecuencias/pérdidas modales
   proceden de mediciones con aire, pueden incluir ya carga acústica; distinguir
   estructura seca y respuesta cargada antes de sumar otra masa o amortiguación.
4. Mantener el estado acústico global junto con tabla y cuerda en reiteraciones
   y nuevas voces; no reiniciarlo en cada golpe. Ampliar luego duración/banda,
   banco alto, unísono y pedal antes de producir un candidato audible.
5. Conservar los pendientes de geometría de ambas caras/borde, Timoshenko,
   masa de graves, backcheck/apagadores y tiempo real/WASM. La fábrica no cambió.

## Procedencia y reproducción

Nuevas fuentes: `tools/concert_grand_air_load.py`,
`tools/audit-concert-grand-air-load.py` y `tools/tests/test_concert_air_load.py`.
Dependencias mecánicas conservadas en el commit aaa520d y en los archivos de
fuentes previos. El resumen guarda hashes de fuentes/entradas/paquete,
versiones del entorno, opciones del solver, criterios y todos los resultados.

`local/concert-grand-air-load/build/source-air-load.zip` congela las tres
fuentes nuevas. SHA256:
`6561addd28699aaf44c2f75e4e6fd5b65ecfb734c3a9696915a85432d8fbc944`.
El log de las ocho pruebas está en la misma carpeta y copiado a SHARED.

Con NumPy/SciPy y un hilo BLAS:

```powershell
$inputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input'
$outputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-carga-aire'
$bridgeDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P3-puente-movil'
python -m unittest discover -s tools/tests -p test_concert_air_load.py -v
python tools/audit-concert-grand-air-load.py --phase linear --input $inputDir --frozen-root $bridgeDir --output "$outputDir/linear"
python tools/audit-concert-grand-air-load.py --phase contact --input $inputDir --frozen-root $bridgeDir --output "$outputDir/contact48" --transverse 48
python tools/audit-concert-grand-air-load.py --phase contact --input $inputDir --frozen-root $bridgeDir --output "$outputDir/contact72" --transverse 72 --profiles Sphere
python tools/audit-concert-grand-air-load.py --phase summary --input $inputDir --frozen-root $bridgeDir --output $outputDir
```

Paquete 0.172.0 preservado:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
