# P6: tabla compacta con flexibilidad alta y pérdidas proyectadas

2026-10-05. El avance anterior de sostenido, release y banda estructural
queda en **c6f3b1a8**, por pedido del usuario; pasan las 35 pruebas dirigidas
de esas herramientas. Se continúa con una reducción de la misma tabla P2
simétrica convergida. Sin audios nuevos en SHARED; datos únicos en `local/`.

## Método y conservación mecánica

El sistema padre tiene 7392 coordenadas modales normalizadas por masa,
con `M = I`, `K = diag(omega²)` y `C = diag(0.046*omega)`. La pérdida sigue
siendo supuesta. Se conservan exactamente los 296 modos hasta 6 kHz.
En el sector superior se construye una base real de desplazamientos forzados:

```text
x(s) = (K + s*C + s²*M)^-1 * B
s = i*2*pi*f,   f = 0, 1500, 3000, 4500, 6000 Hz
```

Se normalizan las columnas real/imaginaria antes de SVD y se descartan
dependencias con tolerancia relativa 1e−11. La base `Q` se usa por congruencia:
`Mr = Qᵀ M Q`, `Kr = Qᵀ K Q`, **`Cr = Qᵀ C Q`**, `Br = Qᵀ B`.
La misma `Q` proyecta las formas Ritz y el observador retardado de superficie.
Por tanto, se conservan matrices positivas, almacenamiento dinámico y
reciprocidad fuerza/velocidad. La velocidad de un puerto y su fuerza
generalizada usan mapas transpuestos. No se reemplaza la cola por un
feedthrough estático ni se ajusta un EQ contra las muestras.

Se diagonaliza la rigidez respecto de la masa real del subespacio superior;
**su matriz de pérdidas permanece completa**. Tener modos de rigidez no
permite suponer que las pérdidas comparten sus autovectores. El marco de
reducción estructural para sistemas mecánicos disipativos se describe en
[Beddig y colaboradores](https://arxiv.org/abs/2010.06331).
Esta realización y sus coeficientes son nuestros ensayos de la geometría P2
supuesta; el artículo no identifica un Steinway ni garantiza su timbre.

## Controles reservados

Los puertos iniciales son C4/E4/G4/C5 (MIDI 60/64/67/72). Se reservan
**36/48/53/65/69/84/96** para medir, sin incluirlos en la construcción.
También se usa una grilla independiente de 901 frecuencias desplazadas
respecto de los 1616 puntos anteriores. La construcción sólo usa los cinco
desplazamientos de frecuencia explícitos; no usa los errores para mover sus
frecuencias o pérdidas. Se refina la cobertura espacial al fallar el control.

El umbral de 1% se aplica a cada banda de **cada puerto de entrada**,
integrando diferencias complejas de admitancia entre todos los puertos
observados y de presión entre ambas cápsulas. Es más exigente que sumar
todas las notas y aceptar sólo la norma global. Sigue sin ser un límite
por frecuencia ni una certificación de las 88 teclas o del sonido cargado.

[Control inicial](medidas-P6-proyeccion-pasiva/estructura.json):

| Construcción | Coordenadas | Máximo nota/banda, grilla original | Grilla independiente |
| --- | ---: | ---: | ---: |
| Cuatro puertos | 332 | 13,7299% | 13,7173% |
| 18 puertos distribuidos | 458 | 2,6486% | 2,6489% |

Ambas pasan flexibilidad estática de sus propios puntos, positividad,
normalización y reciprocidad, pero **fallan las transferencias reservadas**.
La primera es muy precisa en sus cuatro notas y mala al extrapolar: no se
selecciona por eso. La segunda tampoco se selecciona.

## Cobertura espacial que pasa

[Refinamiento espacial](medidas-P6-proyeccion-pasiva/espacial.json).
Se agregan fuerzas virtuales sobre posiciones del puente que equivalen a
21,5; 24,5; …; 105,5 unidades de nota, además de los extremos 21/108 y
los cuatro puertos iniciales. Son **35 posiciones mecánicas**, no notas
grabadas/transpuestas ni muestras de entrenamiento. Ninguno de los siete
puertos reservados coincide con ellas.

El sistema queda en **606 coordenadas: 296 modos retenidos y 310 de cola**,
frente a 7392 del padre. Reducción de coordenadas de 91,80%; no es una
medición de aceleración del renderer ni de coste en WASM.

| Máximo por puerto en la grilla independiente | 60–1200 Hz | 1200–3000 Hz | 3000–6000 Hz |
| --- | ---: | ---: | ---: |
| Admitancia | 0,002523% | 0,039032% | 0,138867% |
| Presión/fuerza | 0,000476% | 0,014811% | 0,128625% |

El máximo de la grilla original es 0,138826%. Pasan los seis controles:
ambas grillas, flexibilidad estática de construcción (5,325e−16), masa
(1,504e−15), disipación positiva y reciprocidad (1,554e−16). La matriz de
masa, rigidez y pérdidas de la cola admite Cholesky; no se recortan
autovalores negativos. La ejecución espacial completa tarda 26,343 s,
incluidos construcción, mediciones, guardar y una ablación de pérdidas.

Activo único:
`local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz`
(59.460.727 bytes), SHA256
`eb976d08d13ab086aa32d61ee6d38307666c95f726046addfd01e7289fef266e`.
Incluye mapa modal, formas Ritz, matrices de cola y kernel de micrófonos;
todavía **no** contiene una carga acústica nueva ni un instrumento instalable.

La ablación que elimina términos no diagonales de `Cr` cambia la transferencia
reservada hasta 0,00503% respecto del sistema con pérdidas completas. En esta
banda el efecto medido es pequeño. No se selecciona la ablación ni se afirma
que explique una diferencia perceptual: se conserva la congruencia completa.

## Movimiento completo detrás de las transferencias

[Control de estado](medidas-P6-proyeccion-pasiva/estado.json) sobre las once
notas y la grilla independiente. Se devuelve el desplazamiento reducido a
las 7392 coordenadas del padre y se compara todo su movimiento. Las formas
Ritz guardadas coinciden exactamente con `U * Q` en la comprobación.

| Norma de error de estado integrado | Máximo nota/banda |
| --- | ---: |
| Velocidad ponderada por masa | 0,195859% |
| Desplazamiento ponderado por rigidez | 1,364417% |
| Velocidad ponderada por pérdida viscosa | 0,443436% |

La primera tiene límite previo de 1% y pasa. Las otras son diagnósticos
separados, sin umbral de selección asignado en este ensayo. Son raíces de
cocientes de formas cuadráticas del **error de estado**, no porcentajes de
energía creada ni errores de balance temporal. La norma por masa tampoco
equivale a error relativo de velocidad en cada punto de superficie.
No se oculta el 1,36% elástico ni se acredita con esto la energía del futuro
sistema cargado/contacto: deberá medirse al integrarlo.

## Pruebas y reproducción

Cuatro pruebas nuevas pasan: congruencia contra matrices completas,
flexibilidad estática/estados forzados en las frecuencias de construcción,
pasividad/reciprocidad y balance de potencia armónica contra calor viscoso,
y entradas sin sector superior válido. El fixture conserva pérdidas no
diagonales y compara el solve separado con el solve matricial completo.

Desde el repo, con Python/NumPy/SciPy y BLAS de un hilo, usando rutas nuevas:

```powershell
python -m unittest tools/tests/test_concert_compliant_projection.py
python tools/audit-concert-grand-compliant-projection.py --parent-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-alta/simetria.json --output local/concert-grand-physics/P6-proyeccion-repeticion --report local/concert-grand-physics/P6-proyeccion-repeticion.json
python tools/audit-concert-grand-spatial-projection.py --mechanical-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-alta/simetria.json --parent-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-proyeccion-pasiva/estructura.json --output local/concert-grand-physics/P6-espacial-repeticion --report local/concert-grand-physics/P6-espacial-repeticion.json
python tools/audit-concert-grand-projected-state.py --mechanical-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-banda-alta/simetria.json --projection-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-proyeccion-pasiva/espacial.json --report local/concert-grand-physics/P6-estado-repeticion.json
```

Los informes padre refieren sus datos originales; la segunda y tercera
reproducción comprueban sus hashes y no duplican esas referencias. El control
completo de once puertos es único en
`local/concert-grand-physics/P6-proyeccion-pasiva-2026-10-05/Transfer-completo-control.npz`.
Los JSON de esta etapa registran criterios, hashes y errores por nota/banda.

## Próximo paso

Construir y verificar la carga recíproca usando **estas mismas formas** y
comparar respuestas cargadas al refinar base/superficie/ángulos. La precisión
seca para fuerzas del puente no garantiza precisión para fuerzas distribuidas
que devuelva el aire. No reutilizar los factores de 59/90 como si fueran esta
tabla. Preservar las pérdidas completas al extender el integrador: actualmente
las herramientas de contacto/coast no aceptan aún esta nueva matriz de cola.

Antes de guardar un factor angular grande, medir memoria/disco y evaluar por
bloques; el usuario pidió no llenar SHARED ni duplicar datos. Después verificar
energía y convergencia temporal en contacto, sostenido y apagado de tecla.
La siguiente escucha sigue pendiente. No se atribuye una mejora de timbre a
esta reducción, ni se cambia el paquete 0.172.0. La geometría sigue supuesta,
un sector vertical simétrico; identificación, unísono, ambas caras/borde,
P4, acción/pedal y ejecución en tiempo real permanecen abiertos.
