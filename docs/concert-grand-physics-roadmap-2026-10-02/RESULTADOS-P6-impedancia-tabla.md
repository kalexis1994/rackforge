# P6 — Impedancia compleja de la superficie P2

Fecha: 2026-10-03. Base congelada: **3bbb170**, commit autorizado del control
recíproco de carga esférica. Este ensayo continúa con la tabla P2 existente.
No modifica la fábrica ni produce un nuevo audio/plugin.

## Qué se calculó

Se conservan las formas físicas normalizadas por masa de los 35 modos de P2
bajo 800 Hz, con sus signos y todos sus términos mutuos. Para velocidad modal
V en sqrt(kg)·m/s, Z actúa en s⁻¹; la fuerza del aire sobre la madera es −Z·V.
Con la convención exp(+iωt):

```text
Z_jl(s) = ρs/(2π) ∬ φ_j(x) φ_l(y) exp(−s|x−y|/c)/|x−y| dSx dSy
Potencia media saliente = 0,5 V* Re[Z(iω)] V
M_aire,estática = lim(s→0) Z(s)/s
```

Resistencia y reactancia provienen de la misma integral saliente. No se
asignan esferas a modos ni coeficientes por nota. Se usa una cara plana en
un baffle rígido infinito, aire ρ=1,2 kg/m³ y c=343 m/s. La geometría P2 es
supuesta; no se ha identificado la tabla del Steinway.

La presión y la impedancia de radiadores rectangulares pueden obtenerse
desde su perfil de velocidad y el Green de medio espacio. El perfil importa
especialmente para la reactancia; aquí se conserva el banco Ritz completo
seleccionado, en lugar de sustituirlo por un perfil fundamental genérico.
[Khorassany et al., 2024, sección 4](https://arxiv.org/pdf/2407.00265).

## Integración singular y resolución

Cada celda rectangular recibe el promedio **analítico** de las funciones Ritz
originales. Es una aproximación de Galerkin constante por celda que se refina
hacia esas mismas formas continuas. La doble integral de dos celdas se reduce
a una integral de desplazamientos con peso de solapamiento triangular.

La singularidad 1/r es integrable. Una transformación de Duffy en los
cuadrantes locales y de celdas que se tocan cancela el denominador con el
jacobiano. No se omite la diagonal ni se inventa un radio de autointeracción.
La convolución Toeplitz se aplica con FFT y relleno suficiente; no se arma
una matriz densa entre todas las celdas. No se recortan autovalores.

Frecuencias declaradas: **60; 80; 261,6255653; 523,2511306; 784,8766959;
800 Hz**. Estos seis puntos son controles, no una garantía entre ellos ni
fuera de banda. Criterios establecidos en la herramienta antes de la corrida:
1% para cambio complejo global; 0,5% para R/X por frecuencia y masa estática;
1e−5 para cuadratura; 1e−8 para el contraste angular en la misma superficie;
0,5% por frecuencia frente a la superficie continua; tolerancia relativa
1e−8 para negativos de redondeo y 1e−10 para balance de potencia.

| Control | Resultado |
| --- | ---: |
| 192×144→256×192, impedancia compleja global | 0,07993% |
| Mismo refinamiento, resistencia, máximo por frecuencia | 0,05718% |
| Mismo refinamiento, reactancia, máximo por frecuencia | 0,11058% |
| Mismo refinamiento, masa estática global | 0,04497% |
| Cuadratura 8→16, impedancia compleja global | 1,219e−12 relativo |
| Re[Z] vs integral angular independiente, mismas celdas | 1,470e−15 relativo |
| Malla fina vs resistencia angular de formas Ritz continuas, máximo | 0,07351% |
| Asimetría de Z en malla fina | 4,582e−16 relativo |
| Autovalor mínimo relativo de Re[Z], malla fina | −2,680e−17, redondeo |
| Balance armónico de potencia, máximo | 6,093e−16 relativo |

Todas las puertas pasan. La reactancia es positiva en los puntos examinados.
La masa estática normalizada tiene autovalores entre **0,00944018 y 0,12442553**:
es adimensional respecto a la masa modal intrínseca, no una masa en kg.
Tres puntos de Laplace en el semiplano derecho también dan parte hermítica
positiva. Esto no sustituye una prueba de pasividad global de una realización
temporal reducida futura.

## Pruebas independientes

**Siete pruebas pasan**:

1. Autointeracción de un cuadrado: masa agregada contra la integral cerrada
   `4 a³ [asinh(1) + (1−sqrt(2))/3]`, a 13 decimales relativos.
2. Pistón circular: resistencia **y reactancia** contra Bessel J₁ y Struve H₁
   a 80/400/800 Hz, cada componente dentro de 0,2%. El borde circular se
   aproxima con fracciones de celdas. La solución analítica es un control
   distinto de la placa. [Anderson, ecuación 15](https://physics.byu.edu/docs/publication/3436).
3. Dividir un radiador uniforme en celdas conserva su impedancia compleja:
   error dentro de 3e−7. Comprueba interacciones locales y vecinas.
4. Resistencia contra potencia angular independiente, reciprocidad y signos.
5. Proyección modal lineal y parte real positiva en el semiplano derecho.
6. Promedios de funciones Ritz contra integración Gauss independiente.
7. Rechazo de geometrías/entradas inválidas.

## Qué indica para el siguiente modelo

Se resolvió una excitación armónica de fuerza 1 N en el punto C4 del puente,
con los mismos modos y pérdidas **supuestas materiales** 0,046·ω_j. Hay
controles sin aire, sólo resistencia, carga completa y sólo diagonal.
Entrada de potencia = calor material + radiación en cada solución.

En estos casos, quitar la reactancia cambia el vector de velocidad entre
14,63% y 33,15% respecto al control sólo resistivo. Eliminar términos mutuos
cambia la velocidad hasta 5,55%. Son sensibilidades de esta geometría y
excitación; no prueban mejor sonido, más cuerpo ni fidelidad al Steinway.
El CSV permite revisar movilidad y potencia sin normalización de resultados.

**Siguiente etapa**: realizar la matriz como una carga causal y pasiva con
estados de aire, comprobar una banda más densa y la energía en el tiempo, y
conservar esos estados durante nuevas notas/reiteraciones. No basta ajustar
amortiguación o una masa estática a partir de estas seis frecuencias.

Antes de sumarla al modelo, separar pérdidas estructurales de radiación y
frecuencias secas de cargadas para evitar doble conteo. Permanecen pendientes
segunda cara/borde, geometría real, banco alto, unísono, Timoshenko, masa de
graves, apagadores/backcheck/pedal y coste en tiempo real. El ensayo siguiente
debe ampliar duración/banda antes de generar un candidato audible.

## Procedencia y reproducción

Datos en `C:\SHARED\Fisica-pendiente-2026-10-03\P6-impedancia-tabla`:
cinco matrices NPZ, `harmonic.csv`, `resumen.json`, log de pruebas y fuentes.
El resumen guarda criterios, resultados, entorno y hashes de entradas,
dependencias congeladas y paquete. Fuente estructural P2:

`5469b9921b41ce4e3b752478e108c4a27be164306f5025317cf596bbb5f88fcd`.

Archivo `source-surface-impedance.zip`, con las tres nuevas fuentes:

`6f12d4f865fcb9ee7302c483961ecdd1d6294b3adfcec4068fc08e8518d3dec7`.

Con NumPy/SciPy y un hilo BLAS:

```powershell
python -m unittest discover -s tools/tests -p test_concert_surface_impedance.py -v
python tools/audit-concert-grand-surface-impedance.py --asset 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz' --output 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-impedancia-tabla' --commit 3bbb170
```

Paquete 0.172.0 preservado:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
