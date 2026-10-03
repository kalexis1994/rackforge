# P3 — contacto acoplado en coordenadas físicas

2026-10-03. **Paso de inicialización verificado en un piloto aislado.** No se
selecciona una voz nueva ni se instala otro paquete. El instrumento audible
continúa en 0.172.0; ambas huellas de fábrica y el SHA del paquete se conservan.

## Qué se hizo

El martillo comienza en el primer contacto con su velocidad incidente; la
cuerda comienza en reposo. Martillo y cuerda comparten la compresión del
fieltro y una sola energía potencial. El movimiento axial se genera durante
el golpe por la misma reacción que actúa sobre el movimiento transversal.
Al separarse, se conservan los desplazamientos y velocidades, sin normalizar
el pico ni asignar un estado axial nuevo.

Este ensayo resuelve la inicialización que quedó pendiente en
[P3 contacto/referencia](RESULTADOS-P3-contacto.md). Importar las amplitudes
audibles actuales no resuelve ese problema: contienen normalizaciones sin
una escala física única.

`tools/concert_grand_coupled_contact.py` utiliza el núcleo SI anterior y añade
el grado de libertad del martillo. Para cada coordenada modal,
`Q=sqrt(m_modal)*q`, `P=sqrt(m_modal)*v`; para el martillo se utiliza su masa.
La compresión es `delta=a·Q`, donde la misma distribución de contacto aparece
en el desplazamiento sentido por el fieltro y en la fuerza sobre la cuerda:

```
shape_n = sin(pi*n*x0/L) * exp(-1.2*(n*width_fraction)^2)
delta = y_hammer - sum(shape_n*q_vertical_n)
U_contact = K*max(delta,0)^(p+1)/(p+1)
U_string = T/2*integral(v_x²+w_x²) + EA/2*integral((u_x+(v_x²+w_x²)/2)²)
E = kinetic_string + kinetic_hammer + U_string + U_contact
```

La integración AVF usa el trabajo exacto del potencial escalar del fieltro
y la cuadratura exacta del gradiente cúbico del potencial discretizado de la
cuerda. El control lineal desactiva sólo el intercambio no lineal T/L; conserva
material, masa de martillo, fieltro, distribución y discretización.

También se comprueba una pérdida de contacto supuesta:
`F=F_elastic*exp(d*delta_dot)`, con `d=0,1 s/m`. El trabajo perdido es
`(F-F_elastic)*delta_dot >= 0`; se contabiliza como calor. **No es la memoria
Stulov de fábrica ni una identificación del fieltro real.** Su control sirve
para probar disipación, no para elegir un fieltro por oído.

## Condiciones y alcance

- C4 y C5, velocidades MIDI 60/124: geometría y parámetros de contacto
  exportados del inspector de fábrica antes de la normalización audible.
- Una cuerda representativa y masa de martillo por cuerda; no se duplica su
  energía para representar un unísono completo. Contacto vertical desde reposo.
- Longitud, densidad lineal y tensión constantes al cambiar velocidad,
  comprobadas en estas dos notas. F#2 entorchada queda fuera de este ensayo.
- Acero macizo supuesto: E=200 GPa, densidad=7850 kg/m³. No propiedades medidas
  del Steinway. Extremos fijos, sin flexión ni Timoshenko, puente o micrófonos.
- Base: 12 modos por dirección transversal y 2 axiales; 6 ms desde primer
  contacto. Se estudian separadamente 24 transversales y 4/8 axiales.
- Referencia independiente DOP853, rtol=1e−11, atol=1e−14 y paso máximo
  limitado por la mayor frecuencia axial. AVF: tasas nominales 176400,
  352800 y 705600 Hz; el paso exacto figura en cada reporte.

Los parámetros de contacto exportados incluyen K, p, ancho y punto de golpe;
la ley de memoria y la dispersión del contacto nativo no están incorporadas.
Por eso comparar el nivel del contacto nativo contra este piloto no permite
atribuir toda la diferencia únicamente al intercambio T/L. El control lineal
emparejado sí permite aislarlo dentro de las condiciones anteriores.

## Resultados

Estado final comparado con DOP853: norma ponderada por la energía modal lineal
de la cuerda y el momento del martillo. Se registra aparte el error de posición
del martillo, normalizado por la compresión máxima; una posición libre no
aporta energía potencial y no queda cubierta por la primera norma.

| Caso | Contacto (ms) | Energía de cuerda al separarse (J) | Error de estado a 705600 Hz | Cambio de energía frente al control lineal |
| --- | ---: | ---: | ---: | ---: |
| C4, 60 | 2,314609 | 0,001677714 | 0,00703% | −0,2082% |
| C4, 124 | 1,792972 | 0,028347103 | 0,06952% | −1,3799% |
| C5, 60 | 2,131884 | 0,000581382 | 0,00274% | −0,2648% |
| C5, 124 | 1,468414 | 0,013892814 | 0,06483% | −2,7118% |

Los errores caen aproximadamente cuatro veces al dividir el paso temporal
por dos: cocientes entre 3,943 y 4,003. En C4 fuerte pasan por 1,11327%,
0,27809% y 0,06952%. El máximo error relativo del balance conservativo
en estos refinamientos es 6,37e−12.

Al copiar los estados al núcleo libre, el salto de energía es como máximo
3,47e−18 J. Un paso libre coincide exactamente con el paso del sistema
acoplado ya separado. El tiempo de contacto mostrado interpola el cruce;
el traspaso se comprueba en el primer paso con compresión no positiva.

El ensayo disipativo pierde entre 5,00% y 13,88% de la energía incidente,
según nota/intensidad, y contabiliza esa pérdida. Su mayor error de estado
frente a la referencia es 0,05923%. Los controles adicionales de trabajo de
fieltro, derivada del potencial común y balance con amortiguación positiva
dan errores relativos 2,34e−11, 9,01e−11 y 3,59e−13, respectivamente.

### Qué evita el nuevo inicio

Como diagnóstico, se toman sólo los primeros 12 modos físicos verticales del
contacto nativo, con axial inicial cero, y se evalúa su energía bajo los dos
potenciales. Añadir el potencial de estiramiento al final sin pagar su trabajo
crearía energía: en C4 fuerte, 0,00021733 J, equivalente a **0,90383%** de su
energía lineal truncada. En C4 suave es 0,05962%; C5 suave/fuerte,
0,01258%/0,00129%. Depende del estado y la fase al separarse.

Esto es una transferencia hipotética, **no una medición de energía creada por
el plugin instalado**, ni prueba de que esa cantidad explique el sonido flaco.
El contacto conjunto evita asignar ese coste después del golpe: todo el
estiramiento y movimiento axial proceden del trabajo del martillo desde el inicio.

### Resolución modal todavía abierta

Con 24 transversales, la energía transferida cambia entre −0,838% y −2,038%
respecto a 12. Con 12 transversales, aumentar los axiales de 2 a 8 cambia la
energía final de cuerda de C4/C5 fuertes **+1,256% / +2,255%**. El incremento
de 4 a 8 es +0,132% / +0,316%, pero la energía axial lineal de C4 a 6 ms
cambia aproximadamente +30,76%. No está demostrada la convergencia de todo
el campo por observar que la energía total cambia poco.

La componente `axial_linear_j` es un diagnóstico; el término de interacción
puede ser negativo. No sumar porcentajes de componentes como si fueran
depósitos independientes positivos, ni interpretar esa componente como presión
sonora o un objetivo de cuerpo del Steinway.

## Artefactos y reproducción

En `C:\SHARED\Fisica-pendiente-2026-10-03`:

- `P3-contacto-acoplado/input/`: transferencia SI y parámetros de contacto.
- `P3-contacto-acoplado/physical/`: reporte, trazas CSV y estados finales NPZ
  de los controles conservativos, lineales y disipativos.
- `P3-contacto-acoplado/resolution/resolution.json`: sensibilidad axial y
  diagnóstico del coste de inicialización.
- `resumen-P3-contacto-acoplado.json`: resultados compactos y hashes.

El inspector congelado y las fuentes están en
`local/concert-grand-coupled-contact/build/`. `source-audit.zip` tiene SHA256
`634070adba7e6a78ec522beee3112e230a7b12f3e6e037bf56322d351bd5d800`;
incluye núcleo anterior, contacto nuevo, auditorías y fuentes Rust. Los
binarios P3 del 2026-10-02 no se sobrescribieron.

```
cargo +1.98.0-x86_64-pc-windows-msvc test -p rackforge-concert-grand --lib --release nonlinear_wires::tests -- --test-threads=1
# Exportación con RUST_MIN_STACK=33554432 y CG_P3_TRANSFER_DIR=<input>
inspector.exe nonlinear_wires::tests::export_contact_state_transfer --exact --ignored --nocapture --test-threads=1
python tools/audit-concert-grand-coupled-contact.py --input <input> --output <physical>
python tools/audit-concert-grand-contact-resolution.py --input <input> --output <resolution>
```

Python necesita numpy/scipy. Usar un solo hilo BLAS para estos sistemas pequeños.
Seis pruebas Rust pasan, tres exportadores permanecen ignorados por defecto;
el exportador de contacto se ejecutó explícitamente. Dos huellas pasan:
`232b085ed4248398` / `4371ab4adc1542ab`. Paquete preservado con SHA256
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

## Siguiente trabajo

1. Cerrar resolución modal y precisión con dispersión incluida, cubriendo
   registros altos, pérdidas y reiteración desde una cuerda ya en movimiento.
2. Mantener masa y frecuencia de referencia independientes de velocidad al
   construir la ruta SI. Resolver la subida dibujada en graves antes de combinarla.
3. Separar el movimiento SI de su conversión de salida. Proyectar fuerza y
   velocidad conjugadas al puente, con la carga y radiación consistentes;
   no reconstruir estiramiento desde amplitudes normalizadas.
4. Comparación audible con control congelado y ganancia común, sin ajuste de
   nivel por nota. Después, decidir si vale integrarlo y volver al refit.

El piloto es costoso y offline. No hay presupuesto de callback/WASM verificado
ni evidencia nueva de mayor fidelidad audible. P2/P4/P5 siguen abiertos.
