# Textura de unísono sobre el sostenido recuperado

## Contexto y decisión

El usuario compara `C4-124-Prueba-sin-carga.wav` con `C4-124-Steinway.wav`
de la auditoría de decaimiento: percibe más sostenido, pero falta cuerpo de
unísono/detune. La primera grabación es una ablación del drenaje, no un cambio
aprobado del plugin. Se conserva esa base para comparar exactamente su impresión
y también se comprueba cada hipótesis con la carga física activa.

El usuario **rechazó el piloto I**, B en el comparador, por un efecto parecido
a flanger. Se conserva como diagnóstico, no como candidato. La separación de
afinación y pérdida confirma cambios de balance entre parciales que el conteo
de mínimos tardíos no bastaba para descartar. Véase la
[auditoría factorial](../concert-grand-unison-flanger-2026-10-02/README.md).

Se prepararon audios del piloto de unísono **I** para escucha. No se aprueba una
calibración de fábrica ni se instala un paquete. En el control sin drenaje mejora
el error conjunto de cuatro notas fuertes, pero las agudas pierden nivel en parte
del sostenido. Con carga física la mejora media es pequeña y el peor caso empeora.
No se ha demostrado que esta textura explique por completo el cuerpo del Steinway.

## Qué encontramos en la base

El modelo conserva tres estados de cuerda en medios/agudos, pero
`UNISON_WIDTH=0` y `STRING_STIFFNESS_SPREAD=0`: las diferencias de afinación y
rigidez están apagadas. `POLARISATION_CENTS=0` también conserva igual afinación
entre polarizaciones. `COHERENT_UNISON=1` mantiene la ley de cada cuerda coherente
a lo largo de todos sus parciales; no es el parámetro que activa el detune.

Hay una diferencia de pérdida considerable: `UNISON_EXTRA_LOSS=2` añade tasas de
amplitud 2 y 3,4 por segundo a los miembros segundo y tercero. Sus factores
propios adicionales, tras un segundo, son `exp(-2)=0,1353` y `exp(-3,4)=0,0334`.
Esto no equivale directamente a sus amplitudes audibles: el puente mezcla las
configuraciones y actúan otras pérdidas. Sí justifica comprobar si la cola queda
demasiado dominada por un miembro. El ensayo anterior ya mostraba que quitar toda
esa pérdida podía empeorar el aftersound proyectado.

La teoría de [Weinreich sobre cuerdas ligeramente desafinadas](https://www.speech.kth.se/music/5_lectures/weinreic/mistuned.html)
describe que el acoplamiento puede modificar el aftersound sin producir una
sucesión permanente de batimientos. Eso depende de la admitancia y afinación;
su ejemplo de bloqueo no proporciona un umbral universal para nuestro modelo.
No se añade un chorus ni un LFO para simularlo.

## Ensayos

Renderer nativo congelado:
`a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831`.
No cambia el DSP de producción. Se usan únicamente knobs existentes.

Primera etapa: F#2, C3, C4, C5, C6 y F#6 a velocidades 28, 60, 92 y 124.
Cada nota se sostiene 2 segundos y se graba durante cuatro segundos.
Además, C4/C5/C6/F#6 a 124 se mantienen **12 segundos**, con dos de release.

| Perfil global | UNISON_WIDTH | Dispersión de rigidez | Pérdida extra |
| --- | ---: | ---: | ---: |
| Base | 0 | 0 | 2 |
| A | 0,04 | 0 | 2 |
| B | 0,10 | 0 | 2 |
| C | 0,10 | 0,003 | 2 |
| D | 0,20 | 0 | 2 |
| E | 0,10 | 0 | 0,5 |
| F | 0,20 | 0 | 0,5 |

`UNISON_WIDTH` es un multiplicador global, **no esos mismos valores en cents**.
La separación real sigue la ley del registro, el programa, la precisión por
tecla y el límite de batimientos del motor. Cada cuerda mantiene una afinación
fija y una rigidez propia en toda su escalera, conservando el centro geométrico.

Se ejecutan dos familias: `Carga-fisica` y `Control-sin-carga`, esta última con
`DIAGNOSTIC_STRING_DRAIN=0`. La reactividad sigue activa en ambos casos. El nombre
histórico «sin carga» significa aquí sin drenaje disipativo, no una cuerda
desconectada de toda interacción del puente. Es un control artificial.

Segunda etapa, piloto fuerte en C4/C5/C6/F#6:

- G: ancho 0,10 y separación adyacente supuesta de apoyos de 8 mm.
- H: lo anterior con pérdida extra 0,5.
- I: ancho 0,65, sin separación espacial, pérdida extra 1.

G/H usan la aproximación existente de gradiente local, compartida entre fuerza,
primer momento y carga. No identifican la geometría del Steinway ni son un
solver completo de apoyos separados. No pasan las comprobaciones conjuntas.
I se conserva sólo para escuchar una diferencia de unísono más perceptible.
Estos pilotos tienen cuatro notas a una intensidad y no validan todo el teclado.

## Resultado del piloto I

Score v2: combinación de errores acústicos en dB, no porcentaje de fidelidad.

| Cuatro notas/124 | Base | I |
| --- | ---: | ---: |
| Carga física, error medio | 7,136 | 7,030 |
| Carga física, peor error | 9,066 | 9,201 |
| Control sin drenaje, error medio | 7,081 | 6,003 |
| Control sin drenaje, peor error | 9,992 | 8,332 |

En C4/124 del control sin drenaje, el error de H1 relativo al Steinway cambia:

| Ventana sostenida | Base, dB | I, dB |
| --- | ---: | ---: |
| 50–200 ms | −1,12 | −0,54 |
| 200–500 ms | −0,27 | +1,35 |
| 500–1500 ms | −0,43 | +1,29 |

El error conjunto de C4 pasa 4,933 → 4,694. El cuerpo del impacto apenas cambia
(7,666 → 7,661): enriquecer el unísono no corrige por sí solo el balance inicial
de armónicos que sigue diferenciándonos del Steinway.

En la misma familia, C6 pierde 3,884 dB entre 0,5–1,5 s y 5,242 dB entre
1,5–3,5 s frente a su base. F#6 pierde 4,572 y 5,350 dB. Esta regresión impide
trasladar el piloto a todos los programas. El usuario también rechazó C4 por oído.

La guarda temporal media del control sin drenaje pasa de 1,974 a 2,032 dB de
error de forma; la subida inicial resuelta mejora de 3,874 a 3,811 dB. Falla
formalmente la guarda estricta de no empeorar la forma media. En perfiles muy
estrechos también falla por diferencias de sólo unas milésimas o diezmilésimas
de dB: esos fallos no se presentan como deterioros perceptivos demostrados.

## Batimientos y límites de la medición

Se demodula cada parcial en la tasa original antes de muestrear su envolvente.
La energía incluye ambas cápsulas, evitando confundir cancelación mono con
silencio. Se buscan mínimos que recuperan al menos 3 dB, separados por 120 ms,
en H1..H16 hasta 10 kHz. La selección de parciales depende únicamente de la
referencia. Se aplica el mismo piso absoluto de −90 dB en todos los perfiles.
La pérdida de nivel se mide aparte; silenciar una cola no cuenta como éxito.

En las cuatro notas largas del piloto I, el control sin drenaje no presenta
eventos detectados después de 1,5 s. Con carga física aparece uno en C6 entre
3,5 y 7,5 s. Esto es un resultado a velocidad 124 con esos umbrales, no garantía
de ausencia de batimientos audibles en otros ataques, intensidades o pedales.
Las referencias originales sí contienen modulaciones, y sus mínimos no se
interpretan automáticamente como defectos.

Las referencias largas se renderizan de nuevo con sfizz usando las grabaciones
originales y su mapeo; no se estiran ni repiten los WAV cortos. Se conservan
identidades de fuente, capa, SFZ, DLL, renderer y audio. Todo el corpus es el
ya conocido; no existe validación acústica independiente ni fase identificada
de cada cuerda real.

## Audición

Carpeta accesible: `C:\SHARED\Unisono-sutil-2026-10-02\ESCUCHAR`.

`C4-Control-sin-carga-A-B-Steinway.wav`:

- 0 s: A, control anterior que escuchó el usuario.
- 4,4 s: B, piloto I de unísono, sobre ese mismo control.
- 8,8 s: C, muestra Steinway.

Misma organización para C5, C6 y F#6. También están las comparaciones de la
familia con carga física, los archivos individuales y los sostenidos de 12 s.
Todos usan ganancia de exportación 0 dB, sin normalización por nota ni EQ.
Sólo se desvanece la frontera final de release durante 10 ms en el comparador;
ataque y tecla sostenida no se alteran.

Los 24 controles actuales de cuatro segundos coinciden muestra por muestra con
las capturas anteriores. Los ocho WAV sin drenaje de C4/C6 coinciden byte por
byte con los archivos de la auditoría que escuchó el usuario. Dos pruebas
Python nuevas detectan una pareja conocida con batimientos, conservan energía
ante cancelación mono y evitan aliasar un parcial agudo a modulación falsa.

No hay commit ni paquete nuevo. El paquete en SHARED conserva SHA256
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.

## Reproducción

```powershell
python tools/audit-concert-grand-subtle-unison.py --binary local/concert-grand-decay/build/renderer.exe --expected-sha256 a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831 --reference local/concert-grand-identified-body --work local/concert-grand-subtle-unison/renders --output C:/SHARED/Unisono-sutil-2026-10-02 --variants docs/concert-grand-subtle-unison-2026-10-02/extra-variants.json
python tools/audit-concert-grand-subtle-unison.py --binary local/concert-grand-decay/build/renderer.exe --expected-sha256 a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831 --reference local/concert-grand-identified-body --work local/concert-grand-subtle-unison/renders --output C:/SHARED/Unisono-sutil-2026-10-02/piloto-apoyos --variants docs/concert-grand-subtle-unison-2026-10-02/spatial-variants.json --extra-only --notes 60,72,84,90 --velocities 124
python tools/audit-concert-grand-subtle-unison.py --binary local/concert-grand-decay/build/renderer.exe --expected-sha256 a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831 --reference local/concert-grand-identified-body --work local/concert-grand-subtle-unison/renders --output C:/SHARED/Unisono-sutil-2026-10-02/piloto-visible --variants docs/concert-grand-subtle-unison-2026-10-02/visible-variant.json --extra-only --notes 60,72,84,90 --velocities 124
python tools/export-concert-grand-subtle-unison.py --input C:/SHARED/Unisono-sutil-2026-10-02/piloto-visible --output C:/SHARED/Unisono-sutil-2026-10-02/ESCUCHAR
python -m unittest discover -s tools/tests -p test_concert_subtle_unison.py -v
```
