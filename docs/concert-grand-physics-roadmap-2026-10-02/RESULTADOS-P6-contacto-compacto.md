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
una trayectoria: faltan los refinamientos y el segundo contacto de este banco.

## Ensayo que se retoma

La ejecución agrupada se interrumpió durante la limpieza del disco. No tiene
todavía una trayectoria completa ni se declara validada a escala C4.
Primero debe coincidir con el control escalar guardado; después se comparan
384/768 kHz para contacto frío y relanzamiento a los 4 ms, conservando los
estados vivos. Umbrales previos: balance 1e−9, energía/trabajo e impulso/fuerza
0,2%, tiempo de contacto 0,1%, trayectorias RMS de puente/tabla 1%.

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
