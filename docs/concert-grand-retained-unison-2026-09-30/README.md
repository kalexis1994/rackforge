# Unísono 12: diferencias coherentes entre cuerdas

## Candidato provisional

C conserva tres estados de cuerda en medios/agudos con idéntica afinación.
El motor además llevaba el ancho a cero desde C7. Se preparó U10-resistivo,
llamado Unisono-12, para recuperar diferencias pequeñas entre esas cuerdas.

- Una afinación fija por cuerda, consistente para todos sus parciales y con
  centro geométrico del trío conservado: `COHERENT_UNISON=1`, ancho global `.10`.
- `UNISON_TREBLE_WIDTH_RATIO=1` conserva esa ley de ancho en los agudos.
- Misma inarmonicidad efectiva entre cuerdas, sin dispersión independiente.
- Acoplamiento disipativo al puente físico anterior, sin reactividad experimental.
- Emisión mecánica, contacto, micrófono y pérdidas intrínsecas conservados.

El control nuevo es experimental, limitado a 0..1, exportado en tuning y vale
cero por defecto. No ocupa índices públicos ni cambia el estado persistente.
No se ha actualizado el plugin principal ni identificado nuevos coeficientes
contra las grabaciones. La aceptación auditiva sigue pendiente.

## Evidencia

C4, F4, G4, C5, G5, C6, G6 y C7 a velocidades 60, 84 y 124. Además, cada nota
se mantiene 12 segundos a velocidad 124, con dos segundos de release. Las
variantes más anchas se descartaron por recuperar oscilaciones y perder cuerpo.

En los 24 casos del candidato, el ataque de 50..200 ms cambia como máximo
0.423 dB; máxima reducción 0.456 dB entre .5 y 1.5 s y 0.969 dB entre 1.5 y
3.5 s. Las colas agudas aumentan, a menudo desde niveles muy bajos: no interpretar
los grandes deltas cerca del silencio como mejoras perceptivas de ese tamaño.

La auditoría mezcla cada portadora antes de remuestrear, incluyendo parciales
hasta 10 kHz. Usa hasta 16 parciales y la misma máscara por nota entre perfiles.
Los mínimos con recuperación de al menos 3 dB son indicadores de envolvente,
no defectos audibles independientes:

| Perfil | .1-.5 s | .5-1.5 s | 1.5-3.5 s | 3.5-7.5 s | 7.5-11.5 s |
|---|---:|---:|---:|---:|---:|
| C | 7 | 2 | 0 | 0 | 0 |
| .10 reactivo | 8 | 10 | 2 | 0 | 0 |
| .10 resistivo, candidato | 6 | 7 | 2 | 0 | 0 |
| .15 resistivo | 6 | 7 | 6 | 1 | 0 |

Los dos mínimos tardíos del candidato son G4: parcial 5 a 2.04 s (3.13 dB) y
parcial 6 a 1.69 s (6.97 dB). No se declara eliminado todo batimiento. El FFT
busca regiones alrededor del armónico nominal; no identifica cada cuerda real.

104 pruebas nativas pasan; 74 especiales se ignoran en la ejecución normal.
Las dos huellas del audio legado pasan aparte. Ocho WAV físicos C coinciden
muestra por muestra con el ejecutable anterior. No se ha verificado ASIO.

## Audio y procedencia

`C:\SHARED\Unisono-12` contiene intensidades de medios/agudos y agudos sostenidos,
para candidato y C anterior. Son mono izquierdo con ganancia común fija, sin
normalización por nota. C anterior es nuestro piano, no una muestra Steinway.
Las piezas completas están en `C:\SHARED` con prefijo `Unisono-12`, en estéreo
44.1 kHz/24 bit. Tempos y eventos MIDI conservados; 10 segundos de cola.
Cambio de ganancia respecto a las piezas C anteriores: +0.0202 dB.

Modelo congelado en `local/concert-grand-coherent-unison/frozen-unison-12`.
`verification.json` registra hashes y controles. `research.patch` es acumulativo
frente a `plugins/concert-grand/src` del worktree histórico `concert-grand-refit`;
no aplicar suponiendo que main tiene esa revisión. Scripts reproducibles:
`tools/render-concert-grand-unison-audition.py` y
`tools/audit-concert-grand-retained-unison.py`.

## Alcance físico

Las afinaciones son parámetros efectivos, no medidas de tensión, longitud o
diámetro por cuerda. No hay LFO ni afinación que cambie con el tiempo. El puente
sigue usando una aproximación modal de acoplamiento débil: no se ha resuelto
recíprocamente el contacto del martillo con tres cuerdas y toda la memoria del
puente. Es una prueba de textura para escuchar, no una demostración de fidelidad.

Fuentes consultadas:

- [Weinreich: modos colectivos](https://www.speech.kth.se/music/5_lectures/weinreic/strings.html).
- [Weinreich: cuerdas desafinadas](https://www.speech.kth.se/music/5_lectures/weinreic/mistuned.html).
- [Woodhouse: varias cuerdas y doble decaimiento](https://euphonics.org/7-3-multiple-strings-and-double-decays/).
