# Baseline de la Raspberry

Estado comprobado el 29 de julio de 2026:

```text
Hardware: Raspberry Pi 4B, 8 GiB
Arquitectura: aarch64
Sistema: Debian 13 (trixie)
Kernel: Raspberry Pi PREEMPT
Hostname: artupy
Usuario: kalex
Root filesystem: ext4 en microSD de 64 GB
Acceso: SSH mediante clave dedicada
```

Paquetes de desarrollo instalados por `../dev/bootstrap.sh`:

- Git;
- CMake y Ninja;
- ALSA y udev development headers;
- pkg-config;
- Rustup y Rust estable;
- tmux, jq, curl y certificados.

El servicio `artupy.service` es solamente una plantilla y aún no está instalado
ni habilitado. El software se despliega en `/home/kalex/artupy/current`.

## Periféricos comprobados

Con el KeyLab y la interfaz conectados simultáneamente:

```text
Arturia KeyLab Essential 61 mk3: USB 1c75:028c, MIDI ALSA
Focusrite Scarlett Solo 3rd Gen: USB 1235:8211, audio ALSA
Estado de alimentación: throttled=0x0
```

La Scarlett funciona a USB high speed y expone reproducción/captura estéreo
S32_LE de 44.1 a 192 kHz. El perfil reproducible está en `../audio/`.
