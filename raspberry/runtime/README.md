# RackForge Runtime

Este workspace contiene el host headless y el contrato de plugins nativos de
RackForge. Está aislado de los motores experimentales actuales para poder
estabilizar la API sin interrumpir el instrumento que ya funciona.

## Componentes

| Directorio | Responsabilidad |
|---|---|
| `rackforge-control-api/` | Protocolo local versionado entre Core y superficies de control. |
| `rackforge-session-api/` | Estado, instancias, comandos, eventos y revisiones independientes de plataforma. |
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

El formato distribuible de un plugin usa la extensión `.rfplugin`:

```text
rf-dls-0.1.0.rfplugin
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
del paquete. El host no ejecuta bibliotecas directamente desde `.rfplugin`;
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

El host conserva compatibilidad binaria con plugins API 1.0–1.3. Los plugins
de referencia se compilan contra la API vigente para probar el contrato
completo.

## Datos privados de plugins

API 1.2 agrega una raíz de datos privada por plugin:

```text
<data-root>/
└── plugins/
    └── org.rackforge.roland-scva/
```

RackForge crea, valida y aísla únicamente esa raíz. No crea carpetas internas ni
impone nombres como `programs`, `resources` o `banks`. Cada plugin decide toda
su estructura interna y puede cambiarla mediante sus propias migraciones.

El host ofrece operaciones de rutas relativas y escritura atómica: rechaza
`..`, rutas absolutas y enlaces simbólicos que escapen del namespace. La
biblioteca nativa recibe la ruta mediante `get_plugin_data_path`; sigue siendo
código confiable dentro del proceso hasta que exista aislamiento opcional.

API 1.4 consolida el vocabulario público de `plugin`. El slot binario del
callback de datos no cambió, por lo que los binarios 1.0–1.3 continúan siendo
aceptados. Las nuevas compilaciones usan `get_plugin_data_path`.

Al abrir por primera vez un plugin, Core migra de forma atómica la antigua raíz
`<data-root>/addons` a `<data-root>/plugins`. Si ambas existen, mueve solamente
namespaces que no colisionan y se niega a sobrescribir datos cuando encuentra
contenido en ambos lados.

Los recursos declarados y los datos del plugin son conceptos distintos:

- un recurso es una dependencia externa seleccionada por RackForge, como una ROM
  o un banco renderizado;
- la raíz privada contiene todo lo que el plugin decida crear o guardar.

## Catálogos dinámicos

API 1.3 permite que un plugin publique su catálogo después de crear la instancia
y abrir sus recursos. Esto resuelve bancos externos cuyo contenido no se conoce
al compilar el plugin. La publicación usa JSON validado por Core y una extensión
compatible de `HostApiV1`; la tabla exportada por plugins 1.0–1.2 no cambia.

RF-DLS usa esta capacidad para convertir cada instrumento encontrado en el DLS
en un preset dinámico con ID opaco y estable, nombre y detalle. Core conserva la
selección activa y las superficies de control nunca leen ni interpretan DLS.

El proceso LIVE crea una sesión con un `session_id`, instancias identificadas
por `instance_id` y una revisión monotónica. El socket Unix local expone
snapshots, historial acotado de eventos y comandos tipados:

```text
plugin → HostApi 1.4 → SessionState → rackforge-control-api → superficies
```

El esquema de sesión v7 publica `plugin_id`, `plugin_name`,
`PluginInstanceState`, el modo superior activo (`LIVE` o `PLAY`), el
`master_level`, el `master_pan`, el contexto `RACK`/`SONG`/`SETLIST` y el árbol declarativo del editor
dentro del draft activo. El lector conserva aliases de vocabulario para campos
de instancia v1 y acepta snapshots sin estos campos como `LIVE`, nivel unidad y
pan centrado, pero todo estado nuevo se serializa únicamente con el vocabulario
`plugin`.

Core persiste el contexto estable de la sesión LIVE en
`data/sessions/live.main.json`: modo activo, contexto LIVE, nivel y pan maestros,
y sonido seleccionado por instancia. La escritura usa un archivo temporal, `fsync` y
reemplazo atómico. Audition, drafts y revisiones son deliberadamente
transitorios. Al reiniciar, un sonido persistido válido tiene precedencia sobre
el preset de arranque; si ya no está en el catálogo, Core informa el problema y
usa el fallback configurado.

La ganancia maestra pertenece al host y se aplica después del procesamiento del
plugin. Usa una curva de fader de audio, nivel unidad como compatibilidad para
sesiones antiguas y una rampa corta para evitar clicks. Los paquetes de
controlador pueden declarar `host_controls` reservados para `master_level` y
`master_pan`; Core los filtra antes de entregar MIDI al plugin, por lo que esos
controles físicos no pueden reutilizarse como CC del instrumento. El balance
maestro mantiene ambos canales a unidad en el centro, atenúa sólo el lado
opuesto y también usa una rampa corta para evitar discontinuidades.

LITTLE ya utiliza `SessionCommand::SetActiveMode`, `SetLiveBrowseMode`,
`ActivateLiveTarget`, `SelectSound`, `BeginAudition`, `KeepAuditionAlive` y
`EndAudition`. Core valida la instancia y el catálogo,
envía la operación mediante una cola acotada y publica un `SessionEvent` solo
después de que el motor la aplica. La selección ocurre al comienzo de un bloque
de audio. El callback no toca el store de sesión ni realiza E/S.

Los contratos de performance viven en `rackforge-performance-api`. `Rack` es el
objeto reproducible; `Song` ordena Parts que referencian Racks y `Setlist` ordena
Songs. LIVE permite entrar directamente por `RACK`, `SONG` o `SETLIST`. Core
guarda los documentos en `data/performance/{racks,songs,setlists}` y rechaza la
biblioteca completa ante IDs duplicados o referencias colgantes. La activación
publica `SoundSelected` y `LiveTargetActivated` sólo después de la confirmación
del hilo de audio.

La edición declarativa usa `EditProgramDraftField`. La superficie entrega un
`field_id` opaco, un valor tipado y el indicador `preview`; Core nunca interpreta
ni modifica el payload. El plugin aplica la mutación, valida el documento
completo y devuelve el documento y el árbol de editor canónicos. Un preview no
cambia el snapshot, no avanza la revisión y no marca el draft como dirty.
`RestoreProgramDraftPreview` restaura el último documento confirmado al
cancelar con `BACK`. `ReplaceProgramDraft` se conserva para tooling y edición
del sobre común, como el nombre del programa.

Los clientes pueden declarar `expected_revision` para rechazar ediciones
obsoletas. Si pierden eventos, solicitan un snapshot completo y reconstruyen su
vista sin depender de estado implícito.

Para crear la raíz o guardar un documento desde tooling:

```bash
rackforge-core plugin-init /home/kalex/rackforge/data org.rackforge.roland-scva
rackforge-core program-save /home/kalex/rackforge/data \
  programs/factory/piano-1.json \
  plugins/roland-scva/programs/factory.piano-1.json
```

La ruta `programs/factory/piano-1.json` es una decisión del plugin Roland, no
una convención obligatoria de RackForge.

## Modelo de programas

RackForge define sólo el sobre común de un programa: identidad, nombre, plugin
propietario, versiones, categoría, tags y un `payload` JSON. El plugin posee,
valida y migra el contenido del payload. Así Core puede catalogar un programa
sin asumir que todos los instrumentos tienen capas, osciladores o FX iguales.

RF-DLS define un payload versionado de una o dos capas. Cada capa referencia un
instrumento DLS y posee rangos de tecla/velocidad y overrides de nivel, tuning,
pitch bend, modulación, envolventes y LFO. Los campos opcionales ausentes
heredan los articuladores del DLS. El payload v3 agregó una cadena FX compartida
después del mix de capas con chorus estéreo; el payload v4 agrega una reverb
ROOM después del chorus; el payload v5 agrega un exciter antes del chorus. Los
payloads v1 se migran a una sola capa `A`; v1 y v2 heredan todos los FX
desactivados, v3 hereda reverb y exciter desactivados y v4 hereda el exciter
desactivado. El payload v6 reemplaza el primer exciter por el modelo clásico de
blend, emphatic point, EQ low/high y dry/wet; v5 se migra automáticamente y
todos se escriben como v6 al siguiente guardado.

La extensión binaria de programas 1.1 agrega un callback opcional de preview.
La versión 1.2 agrega `editor_view` y `apply_edit`: el plugin publica páginas y
campos tipados y conserva la propiedad exclusiva de las mutaciones del payload.
Core sigue cargando 1.0 y 1.1 por sus prefijos de estructura; sólo los plugins
1.2 pueden abrir el editor declarativo. RF-DLS publica 1.2 y preescucha el
documento validado completo sin instalarlo ni persistirlo.

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
  --resource dls-bank=/home/kalex/rackforge/data/plugins/rf-dls/banks/gm.dls \
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

Core retiene por fuente y canal el último estado de los controladores MIDI
continuos (`CC 0..119`), pitch bend y channel pressure. Después de seleccionar
un sonido o transferir/restaurar el foco de audition, reinyecta ese estado al
instrumento al comienzo del siguiente bloque. Así una rueda de modulación o
pedal conserva su posición lógica aunque el plugin haya sido reseteado, sin
mezclar dos controladores que usen el mismo canal. `CC 121` sigue siendo la
única orden que borra explícitamente el estado retenido de ese canal y fuente.

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

## Perfiles de plataforma y arranque

`rackforge-platform-host detect` separa la plataforma binaria, el perfil de
hardware y las capacidades disponibles. En la Raspberry de desarrollo resuelve
`linux-aarch64`, `org.rackforge.hardware.raspberry-pi-4` y los proveedores de
Wi-Fi, telemetría y arranque de audio.

`scripts/install-appliance.sh` instala el host privilegiado con socket Unix
acotado y `rackforge-audio.service`. Core se inicia mediante:

```bash
rackforge-core resume /home/kalex/rackforge/config/audio.toml
```

El documento de inicio es versionado y conserva el paquete, recursos, raíz de
datos y un perfil tipado `[audio.output]`. El schema 2 selecciona la interfaz por
ID estable o identidad USB, nunca por el índice ALSA efímero. El schema 1 se
migra explícitamente al perfil Scarlett anterior para mantener instalaciones
existentes. El servicio no depende de Internet ni de `network-online.target`;
si el teclado o la interfaz de audio todavía no están disponibles, systemd
reintenta sin bloquear el controlador ni la web.

El inventario puede inspeccionarse, con el servicio de audio detenido, mediante:

```bash
rackforge-core audio-list
```

Core valida capacidades, aplica todos los parámetros ALSA juntos y comprueba el
perfil realmente negociado antes de publicar `AUDIO_READY`. El renderer inicial
acepta `S32_LE` estéreo y permite configurar sample rate, period y buffer. La API
puede describir capture, pero capture y controles específicos de fabricante se
mantienen fuera del flujo activo por ahora.

Sobre una instalación limpia de Raspberry Pi OS Lite, la transición opcional a
appliance se ejecuta con `scripts/install-appliance.sh --optimize`, o por
separado con `scripts/optimize-appliance.sh audit|apply|rollback`. El perfil no
desactiva cloud-init hasta comprobar que el primer aprovisionamiento, SSH y al
menos una conexión persistente terminaron correctamente. Todos los cambios del
OS tienen rollback local.

La superficie LITTLE muestra CONFIG > SYSTEM > WI-FI sólo cuando el perfil
publica `system.network.wifi.manage.v1`. Separa redes conocidas y descubiertas,
permite conectar, desconectar u olvidar perfiles, y admite credenciales nuevas
mediante un editor secreto. La clave se entrega al host de plataforma por el
socket local y nunca aparece en argumentos ni logs.

`rackforge-ui::Spinner` es un componente reutilizable e independiente del
transporte. Su animación ASCII `| / - \\` es dirigida por el host mientras una
operación asíncrona sigue activa; Wi-Fi es su primer consumidor, pero el mismo
componente sirve para carga de plugins, bancos o espera de hardware.

## Foco temporal de audition

El protocolo LIVE permite adquirir, renovar y devolver una lease exclusiva para
un editor de plugin. Core captura el preset activo antes de concederla y ejecuta
`reset` al transferir o devolver el foco, evitando notas colgadas. Las
selecciones hechas durante la lease son audibles, pero al liberarla se restaura
el preset capturado.

La lease vence después de 15 segundos sin heartbeat. Un watchdog del plano de
control ordena la restauración mediante la misma cola acotada y publica
`AuditionEnded`; el hilo de audio no consulta relojes, mutexes ni estado de
interfaz. Esto cubre cierres, desconexiones y crashes del editor sin dejar
permanentemente activo un sonido de preview.

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
- raíz de datos privada por plugin, sin estructura interna impuesta;
- documento común de programas con payload versionado por el plugin.
- catálogos de presets dinámicos dependientes de recursos externos;
- sesión versionada con instancias estables, comandos y eventos monotónicos;
- historial acotado para sincronización de superficies;
- lease exclusiva y recuperable para audition durante la edición;
- colas MIDI y de control acotadas hacia el hilo de audio.

Quedan para extensiones compatibles:

- eventos SysEx de tamaño variable;
- buses laterales y configuraciones multibus;
- notificaciones plugin→host;
- aislamiento opcional de plugins en procesos separados.
