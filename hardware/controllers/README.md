# RackForge controller packages

Los controladores físicos no forman parte de Core ni de los plugins de audio.
Cada integración se distribuye como un directorio autocontenido con extensión
`.rfcontroller`, un `rackforge-controller.toml` validado y artefactos por
plataforma.

El store instalado conserva versiones inmutables:

```text
controllers/
├── active/
│   └── org.rackforge.example.json
└── packages/
    └── org.rackforge.example/
        └── 1.0.0/
            ├── rackforge-controller.toml
            └── bin/
```

`rackforge-controller-host` verifica compatibilidad de API, rutas, permisos,
artefactos e integridad antes de instalar. El registro `active` decide la
versión y el nivel de confianza sin aceptar esa afirmación desde el propio
manifest.

Instalar una actualización no elimina la versión anterior. Si una versión
nueva falla, el administrador puede reactivar atómicamente una versión
instalada y reiniciar el host:

```bash
rackforge-controller-host activate org.rackforge.arturia-keylab-essential-mk3 0.1.0
sudo systemctl restart rackforge-controller-host
```

La activación conserva el nivel de confianza asignado por el administrador;
un paquete no puede elevarlo desde su manifest.

## Responsabilidades

Un paquete de controlador puede:

- identificar dispositivos y clasificar endpoints;
- traducir inputs físicos;
- implementar handshake, display, LEDs, heartbeat y restauración;
- declarar layouts explícitos como `little@1`.
- declarar controles físicos reservados para funciones globales tipadas del
  host, como el nivel maestro.
- declarar acciones momentáneas reservadas, con valores explícitos de presión
  y liberación, como el acceso rápido a las partes del teclado.

No puede definir instrumentos, programas, bancos ni lógica propia de un plugin.
Los layouts y eventos lógicos son la frontera entre el hardware y RackForge.

## Runtimes

El schema v1 reserva dos runtimes:

- `process-v1`: ejecutable separado por plataforma y runtime de transición. Se
  admite para paquetes oficiales, certificados y desarrollo local. El proceso
  abre MIDI directamente dentro del sandbox del servicio, por lo que todavía
  no existe aislamiento fino por capability. Los paquetes `community`
  requieren consentimiento explícito al ejecutarlos.
- `wasm-v1`: identificador reservado para el runtime portable y aislado. Un
  host que todavía no lo implemente rechaza el paquete de forma segura. En este
  runtime el host será dueño de MIDI/USB y expondrá únicamente las funciones
  autorizadas al módulo.

Los permisos peligrosos (`raw_usb`, escritura de firmware, red y filesystem)
se rechazan en el schema v1. En `process-v1` esa declaración se refuerza con
trust y sandbox del sistema operativo; la garantía completa llegará con WASM.

Todo driver `process-v1` debe implementar dos comandos sin acceder al hardware:

```text
driver-info
self-test
```

`driver-info` devuelve un único objeto JSON con `protocol_version`, `id`,
`controller_api`, `layouts`, `host_controls` y `host_actions`. El host contrasta
esa identidad completa con el manifest y evita que un binario suplante otro
paquete o reserve un CC no declarado. `self-test` valida codecs, mappings y
mensajes conocidos usando fixtures locales. El comando `conformance` ejecuta ambos antes de
habilitar el servicio:

```bash
rackforge-controller-host conformance org.rackforge.example
```

## Flujo comunitario previsto

1. Crear el paquete con el SDK y fixtures de tráfico MIDI.
2. Ejecutar el conformance suite sin hardware.
3. Probar `probe`, adquisición, health check y restore en el dispositivo.
4. Publicar el `.rfcontroller` como `community` o enviar un PR al repositorio
   de controller packs para certificación.

La implementación Arturia de este repositorio es el paquete de referencia.
