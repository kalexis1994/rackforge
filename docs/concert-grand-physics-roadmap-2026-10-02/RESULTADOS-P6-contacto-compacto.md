# P6: contacto SI con aire compacto persistente

Fecha: 2026-10-04. Base del aire: `919a3c5`. Investigación offline; el plugin
0.172.0 no cambia. Esta etapa conecta el martillo, una cuerda vertical y el
puente móvil al candidato de aire de 64 kHz / polar 96 / azimut 96.

## Construcción y controles

`BoundedContact` conserva las coordenadas mecánicas de masa mixta. Una matriz
C proyecta su velocidad a los 35 modos físicos P2 bajo 800 Hz. La fuerza del
aire vuelve por Cᵀ. No se añade masa de aire instantánea: el puerto contiene
sus estados de ondas y relajación, 11445 en total.

El paso usa trabajo AVF para fieltro/cuerda y punto medio para el aire, con
la misma velocidad centrada. Se comprueban por separado las pérdidas de
fieltro, material y radiación, el trabajo hacia el aire y hacia la tabla.
El relanzamiento sólo recoloca el martillo ya separado e impone su velocidad;
preserva exactamente posiciones/velocidades de cuerda/tabla y ambos estados
acústicos. El cambio energético del martillo se registra como trabajo externo.
Es un control de contacto reiterado, todavía no una mecánica de tecla/pedal.

Cinco tests verifican convergencia frente a una ODE independiente en
coordenadas físicas con masa no unitaria, balances con estados iniciales
vivos, desacoplamiento, relanzamiento y equivalencia del observador agrupado.
Este último conserva estados/fuerzas idénticos en el fixture; sólo cambia
el orden de suma del mismo factor angular para medir la radiación.

## Control escalar ya medido

C4/124, 72 modos transversales, 144 axiales, 4 ms a 384 kHz. Los parámetros
SI y coeficientes permanecen congelados. Medidas y procedencia únicas en
`medidas-P6-contacto-compacto/control-escalar-384000.json` y
`provenance-escalar.json`; trayectoria en
`C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto/T72-384000-Cold.npz`.

| Magnitud | Resultado |
| --- | ---: |
| Energía incidente | 0,0469337190 J |
| Tiempo de contacto | 1,8168373 ms |
| Fuerza máxima | 25,554674 N |
| Impulso | 0,023868586 N·s |
| Energía final de cuerda | 0,0281532483 J |
| Energía final de tabla | 0,0001396683 J |
| Energía final almacenada en aire | 0,00000969126 J |
| Radiación acumulada | 0,000000123356 J |
| Máximo error relativo del balance global | 2,22e−15 |

El almacenamiento acústico se evalúa independientemente cada 0,5 ms, nueve
puntos; trabajo y pérdidas, cada paso. El fixture verifica además la identidad
en cada paso. Un balance correcto no basta para certificar la precisión de
una trayectoria; los refinamientos y el segundo contacto se verifican abajo.

## Resultados completos desde a526d50

La ejecución se retomó después de la limpieza del disco y completó cuatro
casos: primer golpe durante 0–4 ms y relanzamiento durante 4–8 ms, a 384 y
768 kHz. No se reinicia el aire ni la tabla al relanzar. El fieltro conserva
su parámetro de pérdida de tasa cero: su disipación se verifica en el fixture
separado, no se identifica ni ajusta en este C4.

El primer caso agrupado coincide **bit a bit** con el control escalar en q,
v, ambos estados acústicos finales, posiciones/velocidades de tabla y todas
las columnas del registro salvo radiación. Su error RMS de radiación es
8,24e−16; el de radiación final, 2,15e−16. Se conserva el mismo factor angular
completo; no se descartan canales ni se aproximan fuerzas para ahorrar tiempo.
El coste medido baja de 210,94 a 120,89 s para este caso en el mismo equipo.
Es coste de laboratorio offline, todavía no una implementación de audio.

Los umbrales siguen fijados antes de ejecutar: balance 1e−9, energía/trabajo
e impulso/fuerza 0,2%, tiempo de contacto 0,1%, trayectorias RMS de puente/tabla
1%. Todos pasan; no se amplían los límites para aceptar la repetición.

| Cambio 384 → 768 kHz | Primer golpe | Segundo golpe | Límite |
| --- | ---: | ---: | ---: |
| Velocidad RMS del puente | 0,011121% | 0,028135% | 1% |
| Posición RMS del puente | 0,002075% | 0,005676% | 1% |
| Velocidad RMS del banco de tabla | 0,007543% | 0,013821% | 1% |
| Posición RMS del banco de tabla | 0,001848% | 0,004465% | 1% |
| Mayor cambio de energía/trabajo | 0,043529% | 0,005334% | 0,2% |
| Duración del contacto | 0,000458% | 0,000415% | 0,1% |
| Fuerza máxima | 0,002756% | 0,023213% | 0,2% |
| Impulso | 0,000076% | 0,001167% | 0,2% |

La duración fina es 1,816846 ms para el primer golpe y 2,249334 ms para el
segundo, medido desde el relanzamiento. El trabajo externo de éste es
0,0283254612 J; su balance de continuidad relativo es 9,22e−17.
El presupuesto global acumulado de los dos contactos da 3,41e−15 a 384 kHz
y 5,14e−13 a 768 kHz. Máximos en todos los puntos de los cuatro casos:
global 5,79e−13, aire 1,50e−16, tabla 3,68e−16. Los controles evaluados cada
0,5 ms conservan su alcance: no son una evaluación independiente del
almacenamiento en todos los pasos del banco grande.

El contraste con el baño finito anterior cambia la energía final de tabla
−0,005167% y su trabajo +0,001251%; fuerza/duración/impulso iniciales coinciden
hasta redondeo. Son aproximaciones acústicas distintas: ese baño no es una
verdad exacta de banda ancha ni se usa como gate para promover sonido.

`resumen-P6-contacto-compacto.json` contiene medidas, comparaciones, presupuesto
de las dos secuencias, equivalencia del observador y SHA-256 de fuentes y
trayectorias. Los cinco gates pasan. Pasaron también los 16 tests dirigidos.

Informes, procedencia y resumen se guardan sólo en el repositorio. `SHARED`
contiene las trayectorias NPZ; no se generan CSV duplicados ni ZIP de fuentes.

```powershell
python tools/audit-concert-grand-bounded-contact.py --input C:/SHARED/Fisica-pendiente-2026-10-03/P3-contacto-acoplado/input --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --coefficients C:/SHARED/Fisica-pendiente-2026-10-04/P6-resolucion-aire/Polar96/Wave64000-Cells128x96-Angle96x96-Sparse.npz --mass C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos/spectrum.npz --output C:/SHARED/Fisica-pendiente-2026-10-04/P6-contacto-compacto/batched --transverse 72 --rates 384000 768000 --relaunch --reference C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire
```

Para repetir, usar un destino y `--reports` nuevos: el banco rechaza
sobrescribir casos medidos. `--summary` permite escoger otro resumen en el repo.
Quedan abiertos modos altos de tabla, banco de notas/velocidades, pérdidas
identificadas, ambas caras/borde, unísono, P4, graves y acción/pedal. Este
ensayo no es una demostración audible de cuerpo ni se promueve a fábrica.

## Siguiente paso concreto

Ampliar la base estructural con controles por etapas, antes de ajustar el
sonido: el asset P2 tiene 256 modos exportados hasta 2965,08 Hz, mientras
este ensayo sólo usa 35 bajo 800 Hz. Un primer escalón a 1200 Hz incorpora
59 modos; luego 1500 Hz, 90, y 2000 Hz, 145. Recalcular para cada base masa,
resistencia y factor de radiación; no reutilizar matrices de 35 modos como
si describieran las formas nuevas. Mantener pasividad, controles geométricos,
refinamiento temporal y trabajo de contacto. Después añadir observación SI
y comparar notas/velocidades con una ganancia común para escuchar el cuerpo.
La transferencia del aire hasta 6 kHz no sustituye esta ampliación estructural.
