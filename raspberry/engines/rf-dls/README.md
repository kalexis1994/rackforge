# RF-DLS engine (prueba)

Motor para el subconjunto inicial de Downloadable Sounds Level 1/2 que necesita
RackForge. DLS estandariza el contenedor y el comportamiento del sintetizador,
pero no obliga a que dos colecciones tengan los mismos instrumentos, muestras,
regiones, loops o articulaciones. Tampoco garantiza un mapa General MIDI salvo
que el banco se declare compatible con GM.

El repositorio no incluye bancos DLS ni contenido extraído de ellos.

La primera etapa soporta:

- colecciones RIFF `DLS `;
- tabla `ptbl` y pool `wvpl`;
- instrumentos, regiones de nota/velocidad y enlaces de onda;
- ondas mono PCM16;
- afinación `wsmp` con corrección fina signed, atenuación y loops;
- envolvente EG1 en centibeles desde `art1`/`art2`;
- envolvente de pitch EG2, incluida la profundidad `EG2 → Pitch`;
- LFO DLS con frecuencia, delay y profundidad de pitch/atenuación controlada por `CC1`;
- render offline a 48 kHz;
- reproducción MIDI de baja latencia hacia ALSA en Linux ARM64.

Todavía no se interpretan todos los destinos de articulación de DLS-2, filtros,
matrices de modulación, formatos de onda distintos de PCM16 mono ni chunks
propietarios. Un banco que use esas capacidades puede ser DLS válido y aun así
quedar fuera de la compatibilidad actual de RF-DLS.

```text
cargo run --release -- inspect /ruta/banco.dls
cargo run --release -- render /ruta/banco.dls 0 0 60 piano-c4.wav
rf-dls-live --bank 0 --program 0 /ruta/banco.dls
```

Los bancos son recursos aportados por el usuario y deben contar con una
licencia que permita su uso en el dispositivo de destino.
