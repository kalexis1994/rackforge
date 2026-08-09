# Plataforma Raspberry Pi

Este directorio contiene únicamente la integración de RackForge con Raspberry
Pi OS Lite. Core, las APIs y el runtime portable viven en `crates/`.

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
.\platforms\raspberry-pi\dev\health.ps1
.\platforms\raspberry-pi\dev\sync.ps1
.\platforms\raspberry-pi\dev\connect.ps1
```

## Artefacto ARM64

`tools/build-raspberry-pi.sh` compila nativamente en ARM64 y produce
`dist/raspberry-pi/RackForge-RaspberryPi-arm64.tar.gz`. El paquete contiene los
hosts, la Web y la integración de Raspberry Pi, pero ningún instrumento: cada
`.rfplugin` se publica desde su propio pipeline.

Los paquetes `.rfcontroller` y el driver Arturia de referencia viven en
`hardware/`. `rackforge-controller-host` los descubre y supervisa sin conocer
marcas o modelos.

`audio/` contiene el perfil ALSA inicial de la Scarlett Solo y un diagnóstico
de hardware que no reproduce sonido ni modifica el mezclador.

## Tiempo real

La ruta de audio necesita dos mitades independientes, y ninguna sirve sola: la
plataforma debe **conceder** los límites y el host debe **pedirlos**.

| Mitad | Dónde vive |
|---|---|
| Concesión | `LimitRTPRIO` y `LimitMEMLOCK` en `systemd/rackforge-audio.service`; `etc/security/limits.d/rackforge-audio.conf` para ejecuciones manuales. |
| Solicitud | `rackforge_core::realtime::engage`, invocado sobre el hilo que corre el bucle de audio. |

`sbin/rackforge-cpu-performance.sh` fija el governor antes de que arranque el
audio y guarda el anterior para poder restaurarlo. `optimize-appliance.sh apply`
instala ambas mitades, desactiva swap y deja todo revertible con `rollback`.

Un arranque sin privilegios de tiempo real **no es un error**: el host sigue
sonando, pero queda expuesto a dropouts bajo carga. Como esa diferencia es
inaudible hasta el peor momento posible, el host publica su estado en el
arranque y la auditoría lo expone:

```bash
sudo platforms/raspberry-pi/scripts/optimize-appliance.sh audit
```

Las líneas `realtime_status`, `cpu_governor`, `swap_active` y
`xruns_since_boot` describen la postura real de la máquina, no la configurada.

RF-DLS y su motor DLS viven en el repositorio independiente
`rackforge-plugin-rf-dls`. Los bancos `.dls` aportados por el usuario continúan
en `data/plugins/rf-dls`, fuera de Git y de cualquier paquete distribuible.

`crates/` contiene `rackforge-core`, la API versionada para plugins y los
esquemas declarativos con los que cada plugin aporta sus páginas sin introducir
pantallas específicas en el host.

`engines/nuked-sc55/` integra el emulador Roland Sound Canvas, sus herramientas
de compilación ARM64 y su launcher headless. Las ROM permanecen fuera de Git.

`engines/scva-arm64/` contiene el lector Rust nativo para los bancos extraídos
de Sound Canvas VA. Sus datos propietarios viven en `share/scva`, fuera del
despliegue y del repositorio.

La plantilla `systemd/rackforge.service` todavía no se instala: se habilitará
cuando exista un binario de daemon con comportamiento seguro ante fallos.
