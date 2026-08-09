# RackForge

RackForge convierte controladores MIDI y computadoras de propósito general o
embebidas en instrumentos autónomos, sin exigir un escritorio, monitor ni DAW.
El KeyLab Essential 61 mk3 con Raspberry Pi 4B es la primera implementación de
referencia, no el límite arquitectónico del proyecto.

La dirección multiplataforma, el runtime portable de plugins, el SDK y las
superficies futuras están definidos en el [roadmap técnico](ROADMAP.md).

El repositorio separa el producto portable de sus adaptaciones:

| Área | Responsabilidad |
|---|---|
| `crates/` | Core, APIs, SDK y runtime portable de plugins. |
| `apps/` | Ejecutables desktop y headless/web. |
| `platforms/` | Integraciones específicas, comenzando por Raspberry Pi. |
| `hardware/` | Drivers y paquetes para controladores MIDI conocidos. |
| `plugins/` | Fixtures mínimos de conformidad; los instrumentos viven en repositorios propios. |
| `web/` | SPA adaptable de RackForge. |
| `firmware/` | Investigación y firmware de dispositivos. |

## Flujo

```text
Teclas, pads y controles
           │
           ▼
Firmware KeyLab
  • detecta RackForge
  • envía intenciones
  • presenta menús/estado
           │ USB
           ▼
RackForge Core
  • fuente de verdad musical
  • plugins y motores
  • bancos y performances
  • mezcla y audio
           │
           ▼
      DAC USB / Scarlett
```

El host conserva el estado autoritativo de motores, bancos y performances. El
controlador envía eventos físicos y su driver renderiza el estado que recibe.
Si uno de los dos reinicia, un handshake reconstruye la superficie sin depender
de estado implícito.

## Desarrollo remoto

La Raspberry se accede mediante una clave dedicada y un alias local:

```powershell
ssh rackforge
```

No se guardan contraseñas en el repositorio. Las herramientas reproducibles de
conexión, sincronización y diagnóstico viven en
`platforms/raspberry-pi/dev/`.

## Builds automáticos

Cada push a `main` ejecuta `.github/workflows/build-main.yml` y publica tres
artefactos independientes de plugins:

- `RackForge.exe` para Windows x86-64;
- `RackForge-debug.apk` para Android ARM64;
- `RackForge-RaspberryPi-arm64.tar.gz` para Raspberry Pi OS ARM64.

Los plugins mantienen repositorios, versiones y pipelines propios. RackForge
solamente entrega los hosts capaces de instalarlos y ejecutarlos.

## Instalar en Raspberry Pi

La distribución para Raspberry Pi requiere una Raspberry Pi 4 o 5 con
Raspberry Pi OS Lite de 64 bits. El paquete no contiene instrumentos: los
`.rfplugin` se instalan por separado desde RackForge.

Hasta que se publique la primera GitHub Release, el paquete de prueba se puede
descargar desde la ejecución exitosa más reciente de **Build main artifacts**
en la pestaña Actions. Dentro del artefacto
`RackForge-RaspberryPi-arm64-<commit>` está
`RackForge-RaspberryPi-arm64.tar.gz`.

En la Raspberry, como el usuario que ejecutará RackForge:

```bash
mkdir -p "$HOME/rackforge/current"
tar -xzf RackForge-RaspberryPi-arm64.tar.gz \
  -C "$HOME/rackforge/current" --strip-components=1
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install.sh"
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh"
```

El instalador detecta el usuario y su directorio personal, instala el runtime,
la Web, los hosts de plataforma y controladores, y configura los servicios de
arranque. No depende de un nombre de usuario específico. Para una ubicación
personalizada se pueden definir `RACKFORGE_USER` y `RACKFORGE_ROOT`.

Después de la instalación, la interfaz queda disponible en el puerto `8787` de
la Raspberry. Se puede obtener su dirección con:

```bash
hostname -I
```

Desde otro equipo de la misma red se abre `http://DIRECCION_IP:8787`, se
instala un instrumento `.rfplugin` y se seleccionan los dispositivos MIDI y de
audio. La optimización reversible para uso como appliance se activa con:

```bash
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh" --optimize
sudo reboot
```

## Estado

- Raspberry Pi OS Lite / Debian 13 arm64, sin entorno gráfico.
- Toolchain Rust, C/C++, CMake, Ninja, ALSA y udev instalado.
- Comunicación de pantalla SysEx comprobada con el firmware Arturia actual.
- Entrada Note on/off del KeyLab comprobada directamente en la Raspberry.
- Nuked-SC55 compilado nativamente para ARM64; pendiente aportar ROMs propias.
- ABI de Sound Canvas VA 1.1.2 validado y bancos internos catalogados mediante
  herramientas Rust; no son ROMs SC-55 compatibles directamente con Nuked.
- Lector de Wave ROM y decodificador FCE-DPCM compilado y validado nativamente
  en ARM64 con salida idéntica a Windows.
- Resolvedor nativo de tonos, mapas y descriptores de muestra de SCVA 1.1.2;
  `Piano 1/C4` ya produce un preview reproducible por la Scarlett.
- Scaffold bare-metal seguro para el N32G455.
- Port de DOOM conservado como banco de pruebas y función futura.

La prioridad inmediata es completar la primera ruta
KeyLab→Nuked-SC55→Scarlett y luego integrarla en el daemon headless.
