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
| `plugins/` | Plugins de referencia y pruebas de conformidad. |
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
