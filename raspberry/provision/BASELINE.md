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
