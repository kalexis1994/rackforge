# Software Raspberry

Este directorio contiene el cerebro headless de RackForge.

## Responsabilidades

- detectar el KeyLab y la salida de audio;
- mantener la conexión y el handshake con el firmware;
- administrar motores, plugins, bancos, presets, splits y layers;
- mezclar audio y enviarlo directamente mediante ALSA;
- persistir performances y restaurar la última sesión;
- supervisar fallos y recuperarse sin intervención gráfica.

## Máquina objetivo

```text
Raspberry Pi 4B — 8 GiB
Debian 13 (trixie) arm64
Kernel Raspberry Pi PREEMPT
Hostname: rackforge
Usuario de servicio/desarrollo: kalex
```

## Estructura remota

```text
/home/kalex/rackforge/
├── current/       software desplegado
├── banks/         bancos de sonidos
├── performances/  configuración musical
├── state/         estado persistente
└── logs/          logs acotados
```

## Desarrollo

Desde Windows:

```powershell
.\raspberry\dev\health.ps1
.\raspberry\dev\sync.ps1
.\raspberry\dev\connect.ps1
```

`keylab-bridge/` es el puente SysEx actual. Será absorbido progresivamente por
el daemon principal cuando el protocolo compañero esté estabilizado.

`audio/` contiene el perfil ALSA inicial de la Scarlett Solo y un diagnóstico
de hardware que no reproduce sonido ni modifica el mezclador.

`runtime/` contiene `rackforge-core`, la API binaria versionada para plugins y los
esquemas declarativos con los que cada plugin aporta sus propias páginas de
configuración sin introducir pantallas específicas en el host.

`engines/nuked-sc55/` integra el emulador Roland Sound Canvas, sus herramientas
de compilación ARM64 y su launcher headless. Las ROM permanecen fuera de Git.

`engines/scva-arm64/` contiene el lector Rust nativo para los bancos extraídos
de Sound Canvas VA. Sus datos propietarios viven en `share/scva`, fuera del
despliegue y del repositorio.

La plantilla `systemd/rackforge.service` todavía no se instala: se habilitará
cuando exista un binario de daemon con comportamiento seguro ante fallos.
