# Audio

La salida principal comprobada es una Focusrite Scarlett Solo 3rd Gen
conectada por USB a la Raspberry Pi.

## Inventario comprobado

```text
USB VID:PID: 1235:8211
Producto: Focusrite Scarlett Solo USB
Driver Linux: snd-usb-audio
Velocidad USB: high speed (480 Mbit/s)
Reproducción: 2 canales, 24 bits transportados como S32_LE
Captura: 2 canales, 24 bits transportados como S32_LE
Tasas: 44100, 48000, 88200, 96000, 176400 y 192000 Hz
```

El número de tarjeta (`card 4` durante el inventario) es efímero y no debe
guardarse. El perfil inicial usa `hw:CARD=USB,DEV=0`; antes de abrirlo, el
daemon debe confirmar que existe el dispositivo USB `1235:8211` y que el
producto ALSA es `Scarlett Solo USB`.

La ruta `/dev/snd/by-id/usb-Focusrite_Scarlett_Solo_USB_*-00` puede usarse
para resolver el número de tarjeta cuando sea necesario, sin guardar el
número de serie del hardware en Git.

## Perfil inicial

`rackforge-audio.toml` fija 48 kHz, estéreo y `S32_LE`. Un stream de silencio se
mantuvo abierto con 128 frames por período y 384 frames de buffer sin errores
ni subtensión. Es el punto de partida de baja latencia; el daemon deberá
detectar xruns y poder degradar a 256/768 frames.

## Diagnóstico

Después de desplegar `raspberry/`:

```bash
cd /home/kalex/rackforge/current
bash ./audio/probe.sh
```

El script solamente enumera hardware y controles: no modifica el mezclador ni
reproduce audio.
