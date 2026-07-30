# RackForge Runtime

Este workspace contiene el host headless y el contrato de plugins nativos de
RackForge. Está aislado de los motores experimentales actuales para poder
estabilizar la API sin interrumpir el instrumento que ya funciona.

## Componentes

| Directorio | Responsabilidad |
|---|---|
| `rackforge-control-api/` | Protocolo local versionado entre Core y superficies de control. |
| `rackforge-plugin-api/` | ABI C versionada, manifiestos y esquema declarativo de parámetros. |
| `rackforge-core/` | Descubrimiento, validación, carga dinámica e instancias. |
| `rackforge-ui/` | Componentes, foco, layout y estilos independientes del hardware. |
| `plugins/gain/` | Plugin de referencia que prueba audio, parámetros y estado. |
| `plugins/rf-dls/` | Instrumento DLS nativo que usa bancos externos aportados por el usuario. |
| `plugins/roland-scva/` | Primer instrumento real, alimentado por un banco privado externo. |

## Principios del contrato

- Ningún `String`, `Vec`, trait object ni asignador de Rust cruza el límite
  binario.
- `rackforge-plugin-api/include/rackforge_plugin.h` permite implementar plugins en
  C o C++ sin depender de Rust.
- La biblioteca exporta solamente `rackforge_plugin_entry_v1`.
- Las estructuras ABI incluyen `struct_size` y `api_version`.
- El host acepta plugins con el mismo major y un minor no mayor al suyo.
- Los metadatos variables viajan como JSON UTF-8 hacia buffers propiedad del
  host.
- El manifiesto, el descriptor de runtime y el esquema de parámetros tienen
  versiones independientes.
- El hilo de audio no realiza asignaciones, E/S, logs ni bloqueos.
- El estado pertenece al plugin; RackForge lo guarda como bytes opacos.
- Los plugins no dependen de módulos privados de `rackforge-core`; esta frontera
  permite moverlos a repositorios independientes cuando la ABI se estabilice.

El formato distribuible de un addon usa la extensión `.rfaddon`:

```text
rf-dls-0.1.0.rfaddon
└── plugin/
    ├── rackforge-plugin.toml
    ├── lib/
    │   └── libplugin.so
    ├── presets/
    └── assets/
```

RackForge valida y expande ese archivo mediante una instalación atómica. La
forma materializada interna conserva la misma estructura, pero no es el
artefacto que se distribuye:

```text
plugins/rf-dls/
├── rackforge-plugin.toml
├── lib/
│   └── libplugin.so
├── presets/
└── assets/
```

Las rutas declaradas en el manifiesto deben ser relativas y no pueden escapar
del paquete. El host no ejecuta bibliotecas directamente desde `.rfaddon`;
primero las materializa para que el cargador nativo pueda abrirlas y para
permitir verificación, actualización y rollback.

## Recursos externos

API 1.1 permite declarar archivos y directorios que no forman parte del
paquete:

```toml
[[resources]]
id = "rendered-bank"
name = "Rendered SCVA Bank"
kind = "directory"
required = true
```

El host resuelve y valida cada recurso antes de cargar la biblioteca. El plugin
solo solicita su ruta mediante `get_resource_path`; no conoce ubicaciones
específicas de la Raspberry. Esto mantiene bancos, ROMs y muestras fuera de
Git y permite que la misma biblioteca funcione en distintas instalaciones.

La extensión es compatible hacia atrás: el plugin Gain continúa declarando
API 1.0 y se carga sin utilizar el callback agregado en 1.1.

## Datos privados de addons

API 1.2 agrega una raíz de datos privada por addon:

```text
<data-root>/
└── addons/
    └── org.rackforge.roland-scva/
```

RackForge crea, valida y aísla únicamente esa raíz. No crea carpetas internas ni
impone nombres como `programs`, `resources` o `banks`. Cada addon decide toda
su estructura interna y puede cambiarla mediante sus propias migraciones.

El host ofrece operaciones de rutas relativas y escritura atómica: rechaza
`..`, rutas absolutas y enlaces simbólicos que escapen del namespace. La
biblioteca nativa recibe la ruta mediante `get_addon_data_path`; sigue siendo
código confiable dentro del proceso hasta que exista aislamiento opcional.

Los recursos declarados y los datos del addon son conceptos distintos:

- un recurso es una dependencia externa seleccionada por RackForge, como una ROM
  o un banco renderizado;
- la raíz privada contiene todo lo que el addon decida crear o guardar.

## Catálogos dinámicos

API 1.3 permite que un addon publique su catálogo después de crear la instancia
y abrir sus recursos. Esto resuelve bancos externos cuyo contenido no se conoce
al compilar el plugin. La publicación usa JSON validado por Core y una extensión
compatible de `HostApiV1`; la tabla exportada por plugins 1.0–1.2 no cambia.

RF-DLS usa esta capacidad para convertir cada instrumento encontrado en el DLS
en un preset dinámico con ID opaco y estable, nombre y detalle. Core conserva la
selección activa y las superficies de control nunca leen ni interpretan DLS.

El proceso LIVE expone una foto del catálogo y órdenes `SelectSound` mediante
un socket Unix local versionado:

```text
addon → HostApi 1.3 → Core LIVE → rackforge-control-api → KeyLab bridge
```

La selección se aplica al comienzo de un bloque de audio. El servidor de
control usa mensajes JSON acotados, valida IDs contra el catálogo publicado y
no realiza E/S dentro del callback de procesamiento del plugin.

Para crear la raíz o guardar un documento desde tooling:

```bash
rackforge-core addon-init /home/kalex/rackforge/data org.rackforge.roland-scva
rackforge-core program-save /home/kalex/rackforge/data \
  programs/factory/piano-1.json \
  plugins/roland-scva/programs/factory.piano-1.json
```

La ruta `programs/factory/piano-1.json` es una decisión del addon Roland, no
una convención obligatoria de RackForge.

## Modelo de programas

RackForge define sólo el sobre común de un programa: identidad, nombre, plugin
propietario, versiones, categoría, tags y un `payload` JSON. El plugin posee,
valida y migra el contenido del payload. Así Core puede catalogar un programa
sin asumir que todos los instrumentos tienen capas, osciladores o FX iguales.

Roland define inicialmente un payload de una o dos capas. Cada capa referencia
un `sound_id` y posee gain, pan, octava, transposición, afinación fina, rangos
MIDI y ADSR. El programa de fábrica Piano 1 comienza con una sola capa `A`.

## Parámetros y pantalla

Cada plugin expone páginas y parámetros. El host genera su interfaz sin
conocer nombres propios del motor:

```json
{
  "schema_version": 1,
  "pages": [
    {
      "id": "envelope",
      "name": "Envelope",
      "order": 0,
      "header": "ROLAND SCVA"
    }
  ],
  "parameters": [
    {
      "index": 1,
      "id": "envelope.attack",
      "name": "Attack",
      "page": "envelope",
      "kind": {
        "type": "float",
        "minimum": 0.0,
        "maximum": 5.0,
        "default": 0.0,
        "step": 0.01,
        "unit": "s"
      },
      "suggested_control": "knob"
    }
  ]
}
```

`index` es la dirección numérica eficiente usada en tiempo real. `id` es la
identidad estable usada por estados, mappings y migraciones.

`header` es opcional por página. Si el plugin lo declara, el backend puede usar
la región superior disponible; si lo omite, el layout conserva la libertad de
ocultarla y utilizar más espacio. Es una preferencia declarativa: ningún plugin
conoce los bytes SysEx ni las dimensiones del KeyLab.

Los tipos iniciales son:

- `float`
- `integer`
- `boolean`
- `enum`
- `trigger`
- `meter`

## Ciclo de vida

```text
descubrir paquete
  → validar manifiesto
  → cargar biblioteca
  → validar descriptor y parámetros
  → crear instancia
  → activar
  → procesar bloques
  → desactivar
  → destruir
```

Un plugin puede tener muchas instancias dentro de distintos racks. La
biblioteca permanece cargada mientras exista cualquiera de ellas.

## Prueba local

En Windows con la toolchain GNU instalada:

```powershell
$env:Path = "C:\msys64\ucrt64\bin;$env:Path"
cd raspberry\runtime
cargo test --workspace
cargo build -p rackforge-gain
cargo run -p rackforge-core -- `
  smoke plugins/gain/package `
  --library target/debug/rackforge_gain.dll
```

Resultado esperado:

```text
PLUGIN_LOADED id=org.rackforge.gain parameters=2 pages=1 presets=3
PRESET_LOADED id=factory.unity name="Unity"
PARAMETER_ROUNDTRIP id=gain value=0.500000
PLUGIN_SMOKE_OK peak=0.125000 state_bytes=9
```

En Linux ARM64 se usa `target/debug/librackforge_gain.so`.

## Runtime LIVE

En Linux, `rackforge-core live` conecta un plugin de instrumento al puerto MIDI
principal del KeyLab y a la Scarlett. RF-DLS usa el `.dls` como recurso externo:

```bash
rackforge-core live /home/kalex/rackforge/plugins/rf-dls \
  --resource dls-bank=/home/kalex/rackforge/data/addons/rf-dls/banks/gm.dls \
  --preset gm.piano-1
```

El plugin anterior de bancos renderizados continúa disponible durante la
migración:

```bash
rackforge-core live /home/kalex/rackforge/plugins/roland-scva \
  --resource rendered-bank=/home/kalex/rackforge/share/rendered-piano-v1 \
  --preset scva.piano-1
```

El host permanece genérico: selección de muestras, voces, sustain y
parámetros pertenecen al plugin. El host solo administra carga, MIDI, bloques
de audio y salida ALSA.

Las entradas musicales y las superficies UI son conceptos independientes.
Core conecta los endpoints MIDI normales —incluidos controladores todavía
desconocidos— y excluye puertos auxiliares/DAW conocidos. Esto no concede
permiso de display: solamente un driver registrado y una negociación exacta de
layout pueden abrir una salida SysEx.

`scripts/install.sh` instala binario y plugins mediante reemplazos atómicos.
`scripts/select-live-engine.sh rf-dls-plugin` activa RF-DLS mediante Plugin API;
`rf-dls` conserva temporalmente el daemon provisional como rollback. El selector
verifica cada PID antes de detenerlo y restaura el motor anterior si el nuevo no
alcanza `READY_TO_PLAY`.

## Foco temporal de audition

El protocolo LIVE permite adquirir, renovar y devolver una lease exclusiva para
un editor de addon. Core captura el preset activo antes de concederla y ejecuta
`reset` al transferir o devolver el foco, evitando notas colgadas. Las
selecciones hechas durante la lease son audibles, pero al liberarla se restaura
el preset capturado.

La lease vence después de 15 segundos sin heartbeat. Esto cubre cierres,
desconexiones y crashes del editor sin dejar permanentemente activo un sonido
de preview.

## Alcance de API v1.3

Incluye:

- plugins de instrumento, efecto y procesamiento MIDI;
- audio intercalado `f32`;
- MIDI corto con posición dentro del bloque;
- automatización sample-accurate;
- parámetros declarativos;
- bancos y presets con identificadores estables;
- estado opaco versionado por el plugin;
- carga dinámica nativa.
- recursos externos declarados por el plugin.
- raíz de datos privada por addon, sin estructura interna impuesta;
- documento común de programas con payload versionado por el plugin.
- catálogos de presets dinámicos dependientes de recursos externos;
- protocolo local genérico para consultar LIVE y seleccionar sonidos.
- lease exclusiva y recuperable para audition durante la edición.

Quedan para extensiones compatibles:

- eventos SysEx de tamaño variable;
- buses laterales y configuraciones multibus;
- notificaciones plugin→host;
- aislamiento opcional de plugins en procesos separados.
