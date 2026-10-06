# P6 — Observación del campo con cotas de lo omitido

Fecha: 2026-10-06. Etapa numérica del prototipo SI; no cambia el plugin,
sus programas ni el paquete de SHARED. Control congelado:
[prefijos completos anteriores](RESULTADOS-P6-contacto-orden2-prefijos.md).

## Qué se acelera

El contacto mantiene las **606 coordenadas de tabla**, todas sus pérdidas
mutuas y las 787.194 coordenadas acústicas. Conserva la misma cuerda C4
supuesta, fieltro, velocidad incidente, carga, tasas y estados iniciales.
Las fuerzas siguen usando la correlación completa R autenticada.

La nueva base Q sólo observa el campo para medir almacenamiento y flujo.
Se construye con QR y SVD de las memorias ponderadas de los extremos y
las velocidades completas del puerto. No se modifica la trayectoria ni
se utiliza esa base para producir fuerzas. Su tolerancia de selección
(1e−12 de norma de los datos) **no equivale a un error energético aprobado**.
Se calcula y comprueba aparte una cota del campo omitido.

Para cada bloque angular se regenera H completo: mismos taps, direcciones,
pesos y salidas. Se evalúan H Q, sus colas por convolución y sus campos
instantáneos. La radiación se mide con su norma positiva; no se deduce
del trabajo de puerto ni del resto del balance.

## Cotas independientes

Sea A² = trace(R[0]) = suma de todos los cuadrados de H. Cada pasada verifica
esa igualdad con los factores regenerados a error relativo < 1e−10.
Sean N los taps, f la tasa del factor y X₁/X₂ las memorias de cada extremo.
Los residuos son Xᵢ − (Xᵢ Q) Qᵀ. Young y Cauchy–Schwarz dan:

```text
b_almacenamiento ≤ sqrt(N A² / (2 f))
                   sqrt(144 ||residuo X₁||²_F + 12 ||residuo X₂||²_F)
```

Se reconstruyen una vez todos los inputs de las 606 coordenadas desde
las velocidades capturadas. En paralelo se reconstruyen los inputs de
la base Q. Su diferencia completa rᵢ da:

```text
b_radiación ≤ sqrt(A² suma_i(dt_i ||r_i||²_F) / 2)
```

La reconstrucción completa debe reproducir el extremo registrado a < 1e−11.
Para cualquiera de las dos energías observadas Ê, el intervalo es
`[max(sqrt(Ê) − b, 0)², (sqrt(Ê) + b)²]`. El límite inferior cero procede
de la desigualdad triangular; no recorta autovalores ni repara energía física.

Las cotas de proyección son analíticas en aritmética exacta. El cálculo
añade a cada norma residual `gamma` veces la norma proyectada, con
`gamma = N × 606 × epsilon_float64`, y ensancha cada extremo energético
en `1e−11 × max(límite superior, 1e−30 J)`. Son salvaguardas numéricas
reproducibles desde las fuentes registradas en el informe, pero **no es
una demostración formal con aritmética de intervalos del error flotante**.
La comparación con un certificado independiente completo sigue siendo
necesaria. Tampoco se reduce el coste de integrar las fuerzas por paso.

## Criterios declarados antes de medir

- Se mantienen los límites anteriores de energía, interfaz y refinamiento.
- Error máximo de observación < 1e−4 relativo al mayor entre energía medida
  y el piso absoluto 1e−14 J. Ese piso no permite afirmar precisión relativa
  de una radiación casi nula.
- Balance total y del aire < 1e−9 incluso tomando los extremos adversos
  de los intervalos independientes de almacenamiento/radiación.
- Al repetir los prefijos, las 18 energías del certificado completo anterior
  deben quedar dentro de los intervalos nuevos, sin ajustar los límites.
- La puerta general permanece falsa: estos prefijos no cubren nota completa,
  repetición natural, apagador, presión temporal, timbre ni coste del plugin.

## Pruebas del observador

Tres pruebas dirigidas pasan contra un factor completo almacenado:

1. Una base de rango 1 omite dos direcciones con memoria viva y radiación
   no nula: las energías completas quedan dentro de las cotas.
2. Una base completa obtenida por QR/SVD reproduce almacenamiento por
   convolución y flujo independiente a tolerancia 1e−11.
3. Se rechazan una base no ortonormal y un extremo incompatible con la
   historia de puerto.

Se usa un banco pequeño independiente para estas pruebas. Los resultados
del contraste de 606 coordenadas se registran separadamente abajo.

## Contraste completo de los prefijos

[Informe único](medidas-P6-observador-acotado/prefijos125us.json).
Se repiten los mismos seis prefijos, 125 µs frío + 125 µs continuado,
a 384/768/1536 kHz. Q elige **102 columnas**; la cola relativa de los datos
es 9,437300e−13. La dinámica conserva los 606 modos. Cambios absolutos en
66 diagnósticos de trayectoria frente al informe anterior: **cero**
(11 por caso: energías mecánica/relajación/soporte, norma de memoria, tres
pérdidas, trabajo, impulso, máximo AVF y compresión final).

| Control | Resultado | Límite previo |
| --- | ---: | ---: |
| Reconstrucción completa de seis extremos | 0 | < 1e−11 relativo |
| Norma de H regenerado frente a trace(R[0]) | 0 | < 1e−10 relativo |
| 18 energías anteriores dentro de los intervalos | 18 de 18 | Todas |
| Máxima diferencia central frente al certificado anterior | 9,564276e−25 J | Diagnóstico |
| Mayor cota relativa de observación, con piso 1e−14 J | 2,923294e−7 | < 1e−4 |
| Balance total con extremos adversos | 3,258294e−13 | < 1e−9 |
| Balance del aire con extremos adversos | 5,490971e−16 | < 1e−9 |

Pasan los **12 controles del prefijo**, incluyendo los originales de
interfaz, fuerza, trayectoria y refinamiento de segundo orden. La mayor
cota absoluta de error de almacenamiento es 1,343314e−17 J. La mayor cota
de error de flujo es 2,430158e−26 J: comparable al flujo casi nulo de este
prefijo, y sus intervalos pueden incluir cero. No se afirma precisión
relativa de una cola radiada apreciable. Ningún caso frío separa todavía
el martillo; ambos controles de duración permanecen falsos.

Coste medido: **1516,607 s** total, **1390,795 s** de certificado y pico
propio **3,700088 GiB**. El certificado completo anterior registró
1844,820/1706,065 s y 6,588562 GiB. Es una comparación de ejecuciones:
la anterior convivió con otros controles, la nueva no; no permite atribuir
toda la diferencia de tiempo al algoritmo ni es un benchmark de render.
Las medianas por paso mecánico son 0,121506–0,126731 s y siguen siendo
demasiado costosas para tiempo real. Las historias permanecen en 3.257.856
bytes. No se generó otro activo acústico grande, WAV ni paquete.

Los SHA256 del auditor, observador y dependencias se registran al inicio
en el informe; coinciden con las fuentes de esta etapa. Los padres y la
R autenticada siguen congelados. Esta mejora es una observación acotada
validada en estos prefijos, no un modelo físico reducido aprobado.

## Siguiente etapa

1. Alargar frío/continuado hasta observar separación del martillo y flujo
   con límite inferior > 1e−14 J. Refinar tasas y medir allí las cotas y
   coste: Q puede necesitar más columnas al crecer la duración.
2. Segunda pulsación natural y apagador/release con memoria viva.
3. Presión temporal en ambos micrófonos, resolución y cobertura antes de
   escuchar otra nota o melodía. Continúan pendientes identificación real,
   unísono, caras/borde y demás mecanismos del plan.

`all_gates_passed=false`: el ensayo completo y el plugin no están aprobados
por este resultado.

## Reproducción

Python/NumPy/SciPy, BLAS de un hilo; nueva ruta para cada informe:

```powershell
python -m unittest discover -s tools/tests -p test_concert_bounded_observer.py -v
python tools/audit-concert-grand-pade-contact-full.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --compact-certificate --bounded-observer --compare-prefix-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-orden2/prefijos125us-repetido.json --report NUEVO_INFORME.json
```

Se reutiliza una sola R de 1,91 GB, autenticada con SHA256 y firma del
proveedor físico. No se guardan factores duplicados ni estados/historias
de contacto en disco; los informes únicos permanecen en el repositorio.
