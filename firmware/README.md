# Firmware Arturia

Este directorio contiene la investigación y el firmware del KeyLab Essential
61 mk3 para RackForge.

## Responsabilidades

- escanear teclas, pads, botones, encoders y faders;
- detectar el daemon de RackForge mediante handshake y heartbeat;
- enviar eventos e intenciones, no decisiones sobre motores;
- presentar bancos, presets, parámetros, splits y estado en la pantalla;
- conservar un modo degradado claro cuando la Raspberry no esté disponible;
- permitir recuperación al firmware oficial.

## Estado compartido

La Raspberry es la fuente de verdad para motores, bancos y performances. El
firmware es la fuente de verdad para eventos físicos instantáneos. Los mensajes
de estado llevan número de secuencia; después de una reconexión la Raspberry
envía un snapshot completo.

## Etapas

1. Firmware Arturia original + puente SysEx para validar toda la experiencia.
2. Protocolo compañero estable y documentado.
3. Firmware propio ejecutado temporalmente mediante una vía recuperable.
4. Reemplazo instalable solamente después de comprobar backup y recuperación.

## Contenido

- `keylab/`: scaffold Rust bare-metal para el N32G455.
- `analysis/`: mapa offline del firmware y del hardware inferido.
- `recovery/`: firmware oficial y procedimiento conservador.
- `tools/host/`: prueba SysEx desde Windows.
- `vendor/`: fuentes upstream usadas por el experimento DOOM.

No se genera ni instala firmware personalizado mientras la recuperación física
y el formato completo del actualizador no estén validados.
