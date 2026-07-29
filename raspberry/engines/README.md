# Motores de sonido

Los motores se integran detrás del futuro daemon ArtuPy. El daemon será dueño
de la selección de dispositivo, ciclo de vida, presets y estado mostrado en el
KeyLab; cada motor sólo recibirá MIDI y producirá audio.

## Prioridad actual

1. `nuked-sc55`: emulación fiel del Roland SC-55; motor Roland principal.
2. `scva-arm64`: lector y futuro motor nativo de los bancos SC-88/SC-8820
   encontrados en Sound Canvas VA.
3. Backend de prueba nativo: permitirá comprobar el host de plugins sin ROMs.
4. Otros instrumentos y efectos ARM64.

[OpenAudio](https://github.com/webprofusion/OpenAudio) se usa como catálogo de
proyectos, no como dependencia. Cada candidato requiere revisar por separado:

- soporte Linux ARM64 desde código fuente;
- ejecución headless o API de procesamiento separada de la interfaz;
- formato standalone, CLAP, LV2 o biblioteca integrable;
- licencia del código y de los bancos de sonido;
- coste de CPU, memoria y latencia en la Raspberry Pi 4B.

`3HSPlug` es un candidato futuro interesante porque es GPL y ofrece síntesis
GM/GS multitimbral. No sustituye a Nuked-SC55: usa un chip de fantasía y no
emula las ROM ni el circuito Roland.
