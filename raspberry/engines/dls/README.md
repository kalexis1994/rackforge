# RackForge DLS engine (prueba)

Motor genérico para bancos Downloadable Sounds Level 1/2. El repositorio no
incluye bancos DLS ni contenido extraído de ellos.

La primera etapa soporta:

- colecciones RIFF `DLS `;
- tabla `ptbl` y pool `wvpl`;
- instrumentos, regiones de nota/velocidad y enlaces de onda;
- ondas mono PCM16;
- afinación, atenuación y loops `wsmp`;
- envolvente EG1 básica desde `art1`/`art2`;
- render offline a 48 kHz;
- reproducción MIDI de baja latencia hacia ALSA en Linux ARM64.

```text
cargo run --release -- inspect /ruta/banco.dls
cargo run --release -- render /ruta/banco.dls 0 0 60 piano-c4.wav
rackforge-dls-live --bank 0 --program 0 /ruta/banco.dls
```

Los bancos son recursos aportados por el usuario y deben contar con una
licencia que permita su uso en el dispositivo de destino.
