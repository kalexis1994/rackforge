# ArtuPy Runtime

Este workspace contiene el host headless y el contrato de plugins nativos de
ArtuPy. Está aislado de los motores experimentales actuales para poder
estabilizar la API sin interrumpir el instrumento que ya funciona.

## Componentes

| Directorio | Responsabilidad |
|---|---|
| `artupy-plugin-api/` | ABI C versionada, manifiestos y esquema declarativo de parámetros. |
| `artupy-core/` | Descubrimiento, validación, carga dinámica e instancias. |
| `plugins/gain/` | Plugin de referencia que prueba audio, parámetros y estado. |
| `plugins/roland-scva/` | Primer instrumento real, alimentado por un banco privado externo. |

## Principios del contrato

- Ningún `String`, `Vec`, trait object ni asignador de Rust cruza el límite
  binario.
- `artupy-plugin-api/include/artupy_plugin.h` permite implementar plugins en
  C o C++ sin depender de Rust.
- La biblioteca exporta solamente `artupy_plugin_entry_v1`.
- Las estructuras ABI incluyen `struct_size` y `api_version`.
- El host acepta plugins con el mismo major y un minor no mayor al suyo.
- Los metadatos variables viajan como JSON UTF-8 hacia buffers propiedad del
  host.
- El manifiesto, el descriptor de runtime y el esquema de parámetros tienen
  versiones independientes.
- El hilo de audio no realiza asignaciones, E/S, logs ni bloqueos.
- El estado pertenece al plugin; ArtuPy lo guarda como bytes opacos.

Un paquete instalado tiene esta forma:

```text
plugin.artupy/
├── artupy-plugin.toml
├── lib/
│   └── libplugin.so
├── presets/
└── assets/
```

Las rutas declaradas en el manifiesto deben ser relativas y no pueden escapar
del paquete.

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

## Parámetros y pantalla

Cada plugin expone páginas y parámetros. El host genera su interfaz sin
conocer nombres propios del motor:

```json
{
  "index": 0,
  "id": "gain",
  "name": "Gain",
  "page": "level",
  "kind": {
    "type": "float",
    "minimum": 0.0,
    "maximum": 2.0,
    "default": 1.0,
    "step": 0.01,
    "unit": "x"
  },
  "suggested_control": "knob"
}
```

`index` es la dirección numérica eficiente usada en tiempo real. `id` es la
identidad estable usada por estados, mappings y migraciones.

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
cargo build -p artupy-gain
cargo run -p artupy-core -- `
  smoke plugins/gain/package `
  --library target/debug/artupy_gain.dll
```

Resultado esperado:

```text
PLUGIN_LOADED id=org.artupy.gain parameters=2 pages=1 presets=3
PRESET_LOADED id=factory.unity name="Unity"
PARAMETER_ROUNDTRIP id=gain value=0.500000
PLUGIN_SMOKE_OK peak=0.125000 state_bytes=9
```

En Linux ARM64 se usa `target/debug/libartupy_gain.so`.

## Alcance de API v1.1

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

Quedan para extensiones compatibles:

- eventos SysEx de tamaño variable;
- buses laterales y configuraciones multibus;
- notificaciones plugin→host;
- aislamiento opcional de plugins en procesos separados.
