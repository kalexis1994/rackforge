# P3 — Contacto SI y referencia de tensión acumulada

Fecha: 2026-10-02. Continúa [P3](RESULTADOS-P3.md). **Sin candidato seleccionado**.
No se hizo commit, paquete ni instalación. Fábrica 0.172.0 preservada.

## Qué se implementó

1. Referencia absoluta de ángulo y radio del polo en `f64`, sólo en los ensayos
   nativos. Se reconstruye el polo a partir de la frecuencia inicial y el
   desplazamiento relativo solicitado. No se aumenta la ganancia de tensión.
2. Inspector del contacto antes de la normalización y de los estados verticales
   inmediatamente después de construirlos. Diez casos y 40 controles de polos,
   cada uno en un proceso nuevo, para evitar contaminación de parámetros.
3. Conversión explícita a coordenadas de masa y energía, observador de incremento
   de longitud y proyección de fuerza de tensión al extremo, fuera del plugin.
4. Comparación sonora con fábrica y geometría por cuerda congeladas.

## Referencia absoluta de polos

`TENSION_REFERENCE=1` es un interruptor de investigación nativa, sin parámetro
público ni cambio WASM. Para cada polo se conserva `theta0` y `radius`, y se
calcula `theta = theta0*(1+offset)`. La conversión a `rc/rs` sigue siendo `f32`:
un desplazamiento total menor que su resolución puede continuar sin modificar
el polo. Lo que se elimina es perder sucesivamente incrementos pequeños cuyo
total sí sería representable. También se sustituye la aproximación heredada
proporcional a `sin(theta)` por una frecuencia relativa explícita.

El radio se conserva en doble precisión entre actualizaciones para no acumular
errores de renormalización. Si un apagador cambia el radio externamente, se
adopta ese radio. Retirar o reinstalar una componente borra su referencia.
La variante presupone que otros editores no cambian la fase del polo durante
el ensayo: cambios de geometría/voicing en vivo no están verificados. Mantiene
el offset inicial de afinación que ya tenía la fábrica.

Prueba prescrita: 20 000 incrementos de 1e−9 para cuatro ángulos, y vuelta a
cero. El esquema incremental anterior no cambia ningún coeficiente; la
referencia alcanza el desplazamiento acumulado 2e−5 con el error final de
cuantización esperado y regresa exactamente al polo inicial. Otra prueba
verifica pérdida externa de apagador y retiro sin resurrección. Seis pruebas
algebraicas pasan en total; tres inspectores offline permanecen ignorados en
el test normal y se invocan expresamente.

### Magnitud en el instrumento actual

Velocidad 124, primeros ≈120 ms, estadística heredada igual en ambos casos.
Máximo cambio relativo al polo de inicio de cualquier parcial vertical físico:

| Nota | Incremental heredado, cents | Referencia absoluta, cents |
| --- | --- | --- |
| F#2 | 0,015268 | 0,015173 |
| C4 | 0,0004343 | 0,0011742 |
| C5 | 0 | 0,0002386 |
| C6 | 0 | 0,0000398 |
| F#6 | 0 | 0 |

La referencia permite representar parte del cambio perdido, pero las cifras
siguen siendo diminutas. Esta tabla mide polos almacenados, no el pitch del
audio completo ni la contribución reactiva del puente. El CSV también incluye
las variantes geométricas 1/2/3. No demuestra que la tensión física real tenga
una variación tan pequeña.

## Contacto → estados: dónde se pierde la escala física

El contacto entrega `q_n` y `v_n/omega_n` en metros, con masa modal `mu*L/2`.
Después se calcula magnitud proporcional a `h*sqrt(q²+(v/omega)²)`, se iguala
el pico de **cada golpe** al de la receta, se aplican pisos/continuaciones
espectrales y fases de ataque, y se normaliza nuevamente la suma de amplitudes
para imponer la curva MIDI. Luego se divide entre las cuerdas y se aplican
ganancias/proyecciones. Estos estados ya no son desplazamientos mecánicos.

La amplitud de fuerza de tensión ideal sería `T*pi*h/L` veces la amplitud en
metros, con signo alternado en el extremo. Esa proyección, utilizada aquí
como diagnóstico, excluye fuerza de cizallamiento por flexión y un puente real
móvil. No se identifica la sensibilidad del micrófono a partir de ella.

Factor observado del fundamental de la cuerda vertical 0, unidades nativas
por metro de magnitud modal al terminar el contacto:

| Nota | Velocidad 60 | Velocidad 124 | Cociente fuerte/suave |
| --- | --- | --- | --- |
| F#2 | 3,177 | 4,079 | 1,284 |
| C4 | 32,437 | 36,677 | 1,131 |
| C5 | 73,009 | 68,994 | 0,945 |
| C6 | 789,242 | 184,823 | 0,234 |
| F#6 | 6407,056 | 442,631 | 0,069 |

El factor es especialmente dependiente de la intensidad en los agudos. Un
único escalar para mapear fuerzas de tensión de los diez casos a magnitudes
nativas deja **43,02% de error RMS relativo**. Es un control de coordenadas,
no un refit de audio ni evidencia de que una constante arregle la acústica.
La variación entre notas puede incluir proyección/calibración; la dependencia
con velocidad confirma que no es sólo una conversión fija de unidades.

Se comprobó la terminación del contacto: todos se separan antes del límite,
con duraciones de 1,332 a 3,216 ms. No se exportó un estado cortado por el cap
de 20 ms. En F#6 la energía lineal de cuerda entregada pasa de 1,117e−7 J a
4,781e−4 J; el martillo devuelve gran parte de su energía en el golpe suave.
Esto no identifica el contacto real del Steinway ni demuestra que esa enorme
relación dinámica sea correcta. Es la salida del contacto actual antes de
su normalización. Falta validar convergencia del contacto y su constitución
al usarlo como fuente de energía para otro modelo.

### Subida inicial de afinación de graves aún activa

Hay dos rutas diferentes. `start_voice_unit` todavía eleva el `f0` inicial
mediante una ley de velocidad y registro cuando la velocidad supera 0,6.
Los campos `glide_rate/glide_steps` se inicializan a cero: esa ruta no se
deshace mediante el antiguo descenso de 28 pasos. `tension_step` es la segunda
ruta, dependiente del movimiento y auditada arriba.

En F#2, pasar de velocidad 60 a 124 eleva la frecuencia inicial **3,40223
cents**. Como `string_mass` se calcula usando ese `f0` elevado, la masa modal
cambia de 0,009044085 a 0,009008608 kg, **−0,3923%**. A igual tensión y longitud,
la densidad física de una cuerda no debe cambiar con la tecla. Se documenta
para separar esa ley de la escala de referencia al integrar SI; no se cambió
el sonido de fábrica para resolverlo aisladamente.

## Coordenadas físicas verificadas

`tools/concert_grand_physical_coordinates.py` proporciona, para una cuerda con
basis seno de extremos fijos:

```
Q_n = sqrt(mu*L/2) * q_n
P_n = sqrt(mu*L/2) * v_n
E_lineal = sum(P_n² + omega_n²*Q_n²)/2
g_n = (-1)^n * T*pi*n/L / sqrt(mu*L/2)
F_tension = sum(g_n*Q_n)
strain_media = sum((pi*n/L)²*(q_n²+(v_n/omega_n)²))/8
```

Para ambas direcciones transversales se suman sus contribuciones de longitud,
sin sumar cuerdas antes de elevar al cuadrado. El factor 1/8 corresponde al
promedio de fase de la amplitud máxima: 1/2 geométrico, 1/2 por ortogonalidad
espacial y 1/2 por promedio temporal. Para desplazamientos instantáneos, el
factor es 1/4. Confundir ambos introduce otro factor dos en la tensión.

Controles independientes por cuadratura espacial, promedio temporal y trabajo
virtual de la proyección del extremo: errores relativos 1,01e−14, 3,76e−15 y
5,64e−15. La comprobación de energía modal también coincide con la energía
registrada por el contacto dentro del redondeo de `f32`.

Como observador supuesto, con E=200 GPa y densidad=7850 kg/m³ de acero macizo,
se calcula `EA = E*mu/rho`, `DeltaT_media=EA*strain_media` y la frecuencia
ideal cuasiestática. Para C4/C5/C6/F#6 fuertes da respectivamente **4,797 /
2,640 / 0,813 / 0,203 cents**. Estas cifras indican la distancia entre una
estimación en coordenadas físicas y la ley empírica actual; **no son objetivos
medidos del Steinway** ni una ganancia lista para instalar. El cálculo ignora
relajación axial dinámica, dispersión en la sensibilidad a tensión, interacción
no lineal durante contacto y carga de puente. Para F#2 entorchada no se puede
inferir el área del alma a partir de la densidad total: su cifra del reporte
no identifica un módulo axial válido.

El estado físico del contacto es una cuerda representativa, calculada con
masa de martillo asignada por cuerda; no un contacto tridimensional de tres
cuerdas ya identificado. La horizontal agregada, el bloom y los estados
auxiliares no tienen una conversión SI validada aquí. Tampoco se debe copiar
ese estado tres veces y afirmar conservación de la energía de todo el golpe.

## Comparación sonora

Diez casos F#2/C4/C5/C6/F#6 a velocidades 60/124, dos segundos pulsados y dos
de release, 44,1 kHz/256. Sin ajuste de volumen individual, contacto ni tabla.
Veinte controles apagados (fábrica y geometría media) son idénticos por bytes
a los P3 congelados. Referencias Steinway y archivos fuente verificados por SHA.

| Variante | Score medio |
| --- | --- |
| Fábrica anterior | 6,51305535 |
| Referencia absoluta con estadística heredada | 6,51315762 |
| Geometría media por cuerda anterior | 6,51313750 |
| Geometría media por cuerda + referencia absoluta | 6,51312245 |

Menor score es mejor. Las diferencias no justifican selección. Mayor cambio
de forma de onda en F#2 fuerte: −45,46 dB RMS relativo con referencia de fábrica,
−60,85 dB con geometría por cuerda respecto a sus controles. Pedal, medio pedal
y reiteraciones resultan finitos, pico <0,392. No se verificó presupuesto
WASM, callbacks, headroom interno completo ni conmutación en vivo.

**La corrección numérica sola no recuperó cuerpo ni demostró mayor fidelidad.**
No se promueve todavía. Sus datos quedan como control del siguiente paso SI.

## Artefactos y reproducción

En `C:\SHARED\Fisica-pendiente-2026-10-02`:

- `P3-contacto-unidades/`: transfer.csv, contact-end.csv y report.json.
- `P3-tension-referencia/precision.csv`: 40 controles de polos.
- `P3-tension-audio/`: diez intensidades, dos reiteraciones, LEEME y report.json.
- `resumen-P3-contacto.json`: resultados compactos, procedencia y hashes.

`local/concert-grand-tension-reference/build/renderer.exe` produjo los audios;
SHA `1b1c6dd2b18cce8b91a55e45122160f02d838573371276be3c5b7f62aed1f46e`.
`source-audio.zip` conserva esa fuente. `inspector.exe` añade la traza final
de contacto, SHA `ed0854b5633ec908088643a26f50f851345826d41a956a9355e74db4c47f7a03`;
fuente en `source-inspector.zip`. No sobrescribirlos con el build siguiente.

```
cargo +1.98.0-x86_64-pc-windows-msvc test -p rackforge-concert-grand --lib --release nonlinear_wires::tests -- --test-threads=1
python tools/audit-concert-grand-contact-units.py --binary <inspector.exe> --expected-sha256 <hash-inspector> --work <carpeta> --transfer <transfer.csv> --precision <precision.csv> --output <carpeta>
python tools/audit-concert-grand-tension-reference.py --binary <renderer.exe> --expected-sha256 <hash-renderer> --work <carpeta> --output <carpeta>
```

Usar numpy/scipy/soundfile/mido, `RUST_MIN_STACK=33554432` y fuentes/referencias
del workspace. Las huellas de fábrica y pedal del inspector siguen siendo
`232b085ed4248398` / `4371ab4adc1542ab`. Los crudos de controles apagados
comparten un directorio temporal; sus veinte hashes verificados quedan en el
reporte y sus originales completos siguen en P3-geometría.

## Próximo paso dentro de P3

Conservar `q,v` físicos antes de la normalización audible; establecer escala
de referencia y masa independientes de velocidad; separar la conversión al
micrófono del movimiento que usa la no linealidad. El contacto actual usa
modos lineales: al pasarlos a un Hamiltoniano no lineal, la energía inicial de
estiramiento y el estado axial deben corresponder al trabajo del contacto.
Añadirlos arbitrariamente podría introducir otro golpe o producir energía.
Validar esa inicialización y la reacción bidireccional en el núcleo SI antes
de trasladar otra ganancia o recalibrar el instrumento completo. P2/P4/P5
continúan abiertos, especialmente carga conjugada y coste de integración.
