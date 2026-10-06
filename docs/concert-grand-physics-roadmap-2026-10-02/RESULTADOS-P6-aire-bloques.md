# P6: construcción causal por bloques de direcciones

2026-10-05. El soporte temporal con pérdidas completas queda en **7257bc4**,
por pedido del usuario. Se continúa con las 606 coordenadas de la misma
tabla y con el cambio de base modal de esa etapa.

## Construcción y memoria

La realización anterior guarda `H[tap, dirección, modo]`. En una tabla de
606 coordenadas ese factor crece demasiado al refinar la cuadratura angular.
`SurfaceFactorBlocks` regenera las parejas de direcciones en bloques de ocho,
con las mismas distribuciones de retraso de cada celda, derivada hacia atrás
y promedio de taps adyacentes. No omite direcciones, taps ni coordenadas.

`StreamingRationalAir` acumula el Gram espectral de esos bloques y transforma
a correlaciones `R[k]`. Se conserva la realización allpass/Cayley anterior,
con su certificado de almacenamiento positivo y radiación. La FFT inversa
se hace también por bloques de filas para evitar otro arreglo modal completo.
Sólo se simetriza redondeo aritmético; no se recortan autovalores.

La fuerza usa las correlaciones acumuladas. La energía almacenada y la
radiación regeneran los bloques del factor para evaluar el **mismo** certificado:
no se sustituyen por un balance calculado a posteriori ni por pérdida numérica.
`StreamingBoundedAir` sólo añade la relajación de masa/resistencia si ambos
residuales son ya positivos, igual que la realización anterior.

Antes de reservar el Gram se verifica un presupuesto explícito de arreglos
(8 GiB en esta medición). Es una estimación de arreglos, no un límite absoluto
de working set: se mide aparte el pico de proceso. Gram y correlaciones
dependen de taps/modos, mientras que el factor temporal de trabajo depende
del tamaño del bloque, no del total de direcciones.

El coste modal denso de `R` y el coste de regenerar el certificado siguen
presentes. Esto resuelve el almacenamiento de `H`; no acredita ejecución
en tiempo real ni el rendimiento del render de una pieza.

## Verificación de equivalencia

Tres pruebas nuevas pasan en una superficie pequeña con formas arbitrarias:

- Factor por bloques frente al factor completo anterior, correlaciones,
  impedancia en eje imaginario/semiplano derecho y cambio del tamaño de bloque.
- Energía con estados arbitrarios activos, paso midpoint, potencia radiada y
  evaluación por lotes frente al certificado completo. Balance relativo < 1e−11.
- Completado de límites frente a `BoundedAir` y rechazo de residual activo.

Esto verifica equivalencia de construcción y certificado. No demuestra que
una cuadratura angular poco densa sea precisa para las 606 formas de la tabla.

## Preflights con las 606 coordenadas

Se usa superficie 256×192, frecuencia del factor 128 kHz y el mismo cambio
de base `V` para formas de superficie, puertos, masa, rigidez y pérdidas.
La masa estática de referencia se evalúa con Rayleigh continuo y sus términos
singulares locales, en bloques de 16 coordenadas. El límite resistivo es
`rho*c*area*Phi.T*Phi` de las mismas celdas.

Se declaran antes de ejecutar:

- Completado pasivo de ambos límites, sin recortar ni reducir ganancia.
- Error complejo/resistencia/reactancia de impedancia: 2%.
- Error complejo por puerto de admitancia y presión: 1%.
- Disipación relativa mínima ≥ −1e−10, margen de redondeo.

Puntos armónicos: 60; 1881,960712; 6000 Hz. Son **preflights**, sin aceptación
de banda completa, refinamiento espacial, semiplano derecho ni transitorios
acoplados de esta tabla. Si se rechaza el completado, las transferencias
restantes son diagnósticos de la onda incompleta, nunca del candidato final.

### Cuadratura 16×32

[Medición](medidas-P6-aire-bloques/preflight.json). **Rechazada.** La onda es
recíproca, tiene DC nulo y conserva pasividad dentro de redondeo. Pero el
residual de masa tiene autovalores negativos: mínimo −0,0105585182, máximo
0,0102497259. No se puede completar con la relajación pasiva existente.

Los diagnósticos de onda incompleta tampoco cumplen precisión. La diferencia
de presión llega a 32,57% en los tres puntos y once puertos. No corresponde
presentarlos como sonido nuevo aprobado.

Tiempo 98,796 s; working set máximo 6.541.541.376 bytes (6,09 GiB).
Factor completo que se evitó: 1.603.485.696 bytes; factor de trabajo por
bloque: 50.108.928 bytes. El pico global incluye Gram y correlaciones.

### Cuadratura 32×64

[Refinamiento](medidas-P6-aire-bloques/refinamiento32.json). Se repite con
cuatro veces más direcciones, igual tasa, celdas y tamaño de bloque.
**También se rechaza**: residual mínimo −0,0044861946, máximo 0,0057726914.
El completado pasivo sigue siendo imposible. En los diagnósticos de onda
incompleta, el máximo de presión baja a 14,19%; no alcanza el límite de 1%.
Tiempo 309,859 s; working set máximo 6.546.014.208 bytes (6,10 GiB).

El factor completo ocuparía 6.423.871.488 bytes frente a 50.186.496 bytes
por bloque; el pico observado de construcción casi no cambia. Los dos
ensayos conservan sus umbrales. La mejora al refinar es compatible con error
angular importante; no demuestra que sea el único error de la aproximación.

### Masa estática antes de construir correlaciones

Se implementa una identidad del mismo factor discreto, con DC nulo:

```text
G_j = sum(H_l, l <= j)
M_onda = sum(G_j.T * G_j, j < último tap) / rate
       = -sum(k * R_k, k > 0) / rate
```

No es otro ajuste de masa ni una aproximación nueva del objetivo continuo.
Evita construir Gram espectral y correlaciones sólo para descubrir que el
residual de masa no sirve. Tres pruebas adicionales contrastan la identidad
con las correlaciones anteriores para factores aleatorios y de superficie,
cambios de bloque y rechazo de factores con DC no nulo. Pasan las seis pruebas
nuevas de esta etapa.

[Control acumulado y refinamiento angular](medidas-P6-aire-bloques/masa-acumulada.json).
Se contrasta primero 32×64 con el ensayo de correlaciones completo y después
se evalúa 64×128 sin reservar esos dos arreglos grandes.

| Cuadratura | Residual mínimo | Residual máximo | Tiempo de masa |
| --- | ---: | ---: | ---: |
| 32×64, acumulado | −0,0044861946 | 0,0057726914 | 68,516 s |
| 64×128, acumulado | −0,0016222293 | 0,0029174356 | 272,156 s |

La comparación de extremos propios de 32×64 con el ensayo de correlaciones
da diferencia relativa **1,938e−14**: pasa el umbral 1e−10. Ese ensayo
completo tardó 309,859 s, pero incluía además límites y transferencias; no
son dos benchmarks aislados del mismo trabajo. La corrida acumulada de
ambas cuadraturas tarda 346,922 s en total, con working set máximo
920.170.496 bytes (**0,857 GiB**). En cada caso se evita reservar ~5,72 GB
de Gram y correlaciones.

**64×128 también se rechaza como completado de masa**. Se reduce el defecto
negativo al refinar, pero sigue siendo real y excede ampliamente el redondeo.
No se construye su correlación completa ni se escucha una onda incompleta.
Esta puerta decide si merece la pena construir correlaciones; por sí sola
no acredita el resto de la realización causal. Se verificaron hashes de
fuentes, padres y tabla de las tres mediciones finales.

## Datos y reproducción

Se escriben sólo informes pequeños en el repo. Los factores de trabajo no
se guardan; se reutiliza la única tabla proyectada de `local/`. No se deja
otro WAV, paquete o informe duplicado en SHARED.

```powershell
python -m unittest discover -s tools/tests -p test_concert_streaming_air.py -v
python tools/audit-concert-grand-streaming-air.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --report local/concert-grand-physics/P6-aire-bloques-repeticion.json
```

Agregar `--polar 32 --azimuth 64` reproduce el segundo preflight. Cada
medición conserva hashes de fuentes, padres y tabla, así como presupuesto,
límites, errores y consumo observado.

```powershell
python -m unittest discover -s tools/tests -p test_concert_wave_mass.py -v
python tools/audit-concert-grand-wave-mass.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --support-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-perdidas-proyectadas/soporte.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --comparison-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/refinamiento32.json --report local/concert-grand-physics/P6-masa-acumulada-repeticion.json
```

## Qué sigue

La onda causal ya puede construirse conservando todas las direcciones por
bloques. Falta cerrar su precisión y el completado pasivo para esta tabla:
primero separar error de cuadratura angular del de histogramas/tasa. Después
comparar límites, semiplano derecho y los 83 controles cargados de la referencia,
y recién entonces contacto, sostenido y apagador acoplados en el tiempo.
No se cambia el programa de fábrica a partir de un preflight rechazado.

Control próximo concreto: masa acumulada 96×256 antes de reservar Gram,
manteniendo celdas/tasa; después contraste de tasa para distinguir error
angular del temporal. Si el residual no cierra, revisar la integración de
direcciones y considerar promedios sobre regiones angulares con certificado
de energía, en lugar de reparar la matriz residual. Cualquier cambio de
representación debe conservar el puerto físico y pasar transferencia.

Continuación del 2026-10-06: [integración angular y contraste temporal](RESULTADOS-P6-integracion-angular.md).
La masa residual 96×256 pasa a ser positiva a 128 y 64 kHz. Se conserva el
rechazo de las cuadraturas anteriores. Esto permite intentar el completado;
la impedancia y los transitorios del candidato completo siguen pendientes.
