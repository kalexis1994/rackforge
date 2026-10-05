# P6: pérdidas completas en contacto, sostenido y apagador

2026-10-05. La validación refinada de carga continua queda en **40e3547**,
por pedido del usuario. Esta etapa prepara la mecánica temporal para la
tabla proyectada de 606 coordenadas. No identifica coeficientes nuevos.

## Problema resuelto

La proyección conserva masa, rigidez y pérdidas por la misma congruencia.
Aunque se diagonalice la rigidez respecto de la masa, las pérdidas de la
cola no suelen ser diagonales en esa base. `MovingBridge` sólo recibía un
vector de pérdidas. Los integradores admitían una matriz, pero varios
diagnósticos de fuerza y calor también asumían ese vector.

`Support` admite ahora una matriz real simétrica de amortiguamiento. Se
rechazan matrices activas, complejas, asimétricas o la combinación ambigua
de matriz completa con pérdidas diagonales adicionales. No se recortan
autovalores. El margen para semidefinitud es sólo de redondeo relativo 1e−12.
Los soportes diagonales anteriores siguen usando su construcción original.

Todos los consumidores físicos usan ahora:

```text
F_material = C_board * v_board
calor por segundo = v_board.T * C_board * v_board
C_normalizada = T.T * C_board * T
```

Esto incluye componentes de energía, interfaz de `BridgeHammer`,
`BoundedContact` y el control anterior `BathContact`. El coast exponencial
y el apagador ya reciben la matriz normalizada completa: no se elimina
su acoplamiento durante el paso disipativo. `core.viscous` queda sólo como
diagonal de compatibilidad para fixtures históricos; no gobierna fuerzas
ni calor.

## Adaptador de coordenadas

`ModalSupportMap` resuelve el problema generalizado con la **masa proyectada
real**:

```text
K * V = M * V * diag(omega²)
V.T * M * V = I
q_original = V * eta
f_modal = V.T * f_original
C_modal = V.T * C_original * V
```

No se reduce el número de coordenadas y no se diagonaliza ni reajusta
`C_modal`. El extremo móvil de la cuerda conserva después sus términos de
inercia mixta y su normalización anterior.

La futura carga acústica y los micrófonos deben usar **el mismo V**:
`Z_modal = V.T * Z_original * V`, y `P_modal = P_original * V`.
No se puede cambiar sólo el soporte y conectarle un factor acústico en otra
base. El adaptador no suministra ni reutiliza una realización acústica vieja.

## Pruebas

Cinco pruebas nuevas verifican:

- Invariancia de estado, puertos, energía y calor con masa no identidad,
  rigidez no diagonal y pérdidas con términos mutuos importantes.
- Rechazo de pérdidas inválidas o ambiguas.
- Fuerza y calor completos y cierre de ambas interfaces de aire existentes.
- Contacto contra una ODE independiente en las coordenadas físicas, integrada
  con DOP853; refinamientos 512/1024/2048 kHz dan convergencia de segundo
  orden y error final < 0,05% en el estado de ese fixture.
- Balance vivo de contacto, coast y apagador: error relativo < 1e−9,
  incluyendo calor material y trabajo/radiación del aire.

Pasan además tres regresiones seleccionadas de la mecánica anterior:
inercia/derivadas/potencia multimodal, calor de soporte y fieltro, y
recuperación del contacto de cuerda fija cuando el puente no se mueve.
**Ocho pruebas pasan.** Los controles acústicos de estos fixtures son pequeños
y sintéticos; no validan un factor causal nuevo para las 606 coordenadas.

## Medición con la tabla de 606 coordenadas

[Informe único](medidas-P6-perdidas-proyectadas/soporte.json). Se reutiliza
`Tabla-proyectada-pasiva.npz`, sin copiarlo. Se contrasta el cambio de base en
los 83 puntos armónicos y once puertos de la corrida anterior. Esta vez se
mide **equivalencia de coordenadas de la mecánica seca**, no otro refit ni
otra aproximación de la carga del aire. Los errores de estado/puertos son
normas Frobenius relativas de las matrices de los once puertos; el calor
se comprueba por puerto.

Además se ejecuta un contacto mecánico de C4 con las 606 coordenadas:
192 pasos a 1,536 MHz, 0,125 ms, martillo incidente de 5 m/s. Cuerda y
fieltro son supuestos explícitos del fixture anterior, no una identificación
del Steinway. Es un control inicial de conexión y energía, no una nota
completa ni evidencia de fidelidad de ataque, sostenido o release.

**Pasan los siete controles de la medición.** Los límites se fijan
en el auditor antes de ejecutar: congruencia 1e−11, equivalencia armónica y
calor 1e−10, energía temporal 1e−9, residual de interfaz 1e−9 N/W.

| Control | Máximo medido |
| --- | ---: |
| Congruencia de masa | 1,483e−15 |
| Congruencia de rigidez | 1,592e−15 |
| Congruencia de pérdidas | 3,463e−17 |
| Estado armónico original frente a reconstruido | 1,068e−11 |
| Respuesta de puertos | 2,251e−12 |
| Calor armónico por puerto | 2,796e−12 |
| Energía del contacto inicial más calor acumulado | 5,843e−14 |
| Residual de fuerza de interfaz | 9,020e−15 N |
| Residual de potencia de interfaz | 6,649e−20 W |

La norma de los términos fuera de la diagonal es el **12,17%** de la norma
de `C_modal`: no resulta razonable sustituirla por su diagonal. Esto no
implica que el cambio de timbre sea de 12,17%; es una medida matricial.
El menor autovalor de pérdidas es positivo (20,411 en coordenadas de masa
normalizada). En el contacto inicial hay 0,025 J; quedan 0,0246289312 J de
energía mecánica y se contabilizan 0,0003710688 J de calor total.
La medición tarda 38,812 s. Se verificaron hashes de las fuentes registradas,
informe padre y tabla; no se escribió un segundo factor ni archivos en SHARED.

## Reproducción

Python/NumPy/SciPy, BLAS con un hilo, desde el repo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_projected_support.py -v
python tools/audit-concert-grand-projected-support.py --loaded-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-carga-directa/refinada.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --report local/concert-grand-physics/P6-soporte-repeticion.json
```

## Siguiente

Construir una realización causal pasiva y acotada en memoria de la carga de
aire sobre esta tabla, usando la referencia continua ya validada. Luego
validar transitorios acoplados, contacto completo, sostenido y release antes
de escuchar una nota o melodía nueva. Siguen pendientes ambas caras/borde,
identificación de la tabla real, unísono y acción/pedal completos. No se
genera audio ni se cambia el paquete instalado en esta etapa.
