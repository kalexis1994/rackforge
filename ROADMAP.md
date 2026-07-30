# Roadmap de RackForge

## Visión

RackForge debe convertirse en un runtime musical autónomo y multiplataforma.
El mismo plugin debe poder ejecutarse en Linux ARM64, Windows, macOS y Android
sin conocer ALSA, WASAPI, CoreAudio, AAudio, USB, rutas del sistema ni la
arquitectura del procesador.

La Raspberry Pi es la primera plataforma de producción, no una restricción del
diseño. RackForge será para los plugins lo que una máquina virtual es para una
aplicación portable:

```text
Plugin universal
      │
      ▼
RackForge Runtime + API estable
      │
      ├── Linux ARM64
      ├── Windows x86-64/ARM64
      ├── macOS ARM64/x86-64
      └── Android ARM64
```

No se construirá una máquina virtual desde cero. El objetivo es usar
WebAssembly como formato ejecutable portable y construir encima el runtime,
las capacidades musicales, el SDK y el formato de distribución de RackForge.

## Principios no negociables

1. Un plugin consume solamente la API pública de RackForge.
2. Ningún plugin depende directamente de APIs de una plataforma.
3. El host es dueño del audio, MIDI, almacenamiento, red, controladores y
   superficies.
4. El hilo de audio no asigna memoria, no realiza E/S, no bloquea y no ejecuta
   lógica de interfaz.
5. Los plugins reciben capacidades explícitas, no acceso general al sistema.
6. Toda API pública y todo formato persistente son versionados.
7. Las instancias, no solamente los tipos de plugin, poseen estado.
8. LITTLE, MEDIUM y WEB son proyecciones del mismo estado y utilizan los
   mismos comandos.
9. Un controlador MIDI desconocido puede tocar, pero nunca recibe SysEx ni
   control de pantalla sin un driver registrado.
10. Los layouts nunca se infieren por tamaño o cantidad de controles.
11. RackForge mantiene compatibilidad mediante negociación explícita, pruebas
    de conformidad y migraciones.
12. El formato portable será el camino principal; los plugins nativos actuales
    existirán solamente durante la transición o como extensión opcional.
13. `Plugin` es el término público para instrumentos, efectos, procesadores
    MIDI y utilities; `module` queda reservado para implementación interna.
14. El payload de un programa pertenece exclusivamente al plugin. Las
    superficies editan campos opacos mediante un árbol declarativo común y
    nunca conocen rutas JSON internas.

## Arquitectura objetivo

```text
Entradas MIDI ───────────────┐
Controlador LITTLE ──────────┤
RackForge WEB ───────────────┤
Automatización futura ───────┘
               │
               ▼
       Command / Event Bus
               │
               ▼
       Estado de la sesión
               │
       ┌───────┴────────┐
       ▼                ▼
Plugin Control API   Motor de tiempo real
       │                │
       ▼                ▼
LITTLE / WEB       Backend de audio
                        │
                        ▼
                 Scarlett / DAC
```

El estado de sesión es la única fuente de verdad. Una modificación realizada
desde WEB debe aparecer en LITTLE, y una modificación hecha con el encoder debe
llegar a WEB. Ninguna superficie implementa por separado la lógica musical del
plugin.

## Capas del sistema

### RackForge Core

Responsable de:

- descubrir, validar, instalar y actualizar plugins;
- crear y destruir instancias;
- administrar racks, programas, bancos y recursos;
- mantener el estado autoritativo de sesión;
- procesar comandos y publicar eventos;
- negociar capacidades y layouts;
- coordinar el motor de audio en tiempo real;
- aplicar permisos, límites y aislamiento;
- recuperar el estado después de reinicios.

### Backends de plataforma

Cada plataforma implementa adaptadores concretos:

| Área | Linux | Windows | macOS | Android |
|---|---|---|---|---|
| Audio | ALSA/PipeWire | WASAPI/ASIO | CoreAudio | AAudio/Oboe |
| MIDI | ALSA Sequencer | Windows MIDI | CoreMIDI | Android MIDI |
| Archivos | Backend POSIX | Backend Windows | Backend macOS | Storage Android |
| USB/controladores | Driver Linux | Driver Windows | Driver macOS | Driver Android |
| WEB | Servidor headless | Local/embebido | Local/embebido | WebView/embebido |

Los backends traducen el sistema operativo al modelo común. Los plugins nunca
ven estas diferencias.

### Runtime portable

El runtime cargará componentes WebAssembly y expondrá una API RackForge
versionada. El WebAssembly Component Model y WIT se usarán para ciclo de vida,
metadatos, estado, catálogos, comandos y capacidades.

La ruta crítica de audio tendrá una ABI mínima y preasignada:

```text
activate(sample_rate, max_frames)
process(frames, midi_events, audio_buffers)
deactivate()
```

El host reservará memoria al activar la instancia. El procesamiento se
realizará una vez por bloque y no serializará ni copiará muestra por muestra.
El SDK ocultará la memoria lineal y las llamadas de bajo nivel.

### Runtime nativo de transición

La ABI C y las bibliotecas `.so`/`.dll` existentes permanecerán disponibles
mientras se construye la ruta WebAssembly. Un adaptador permitirá que Core
trate instancias nativas y portables mediante el mismo modelo interno.

No se agregarán dependencias de plataforma nuevas a la API de plugins nativos.
RF-DLS será el primer plugin migrado y la ABI nativa se declarará heredada cuando
la implementación portable alcance paridad funcional y de rendimiento.

## API universal de plugins

La API pública debe cubrir, como mínimo:

- descriptor y versión del plugin;
- ciclo de vida de instancias;
- configuración de audio;
- procesamiento de audio y MIDI;
- parámetros y páginas declarativas;
- catálogos dinámicos;
- selección y edición de programas;
- serialización, restauración y migración de estado;
- recursos externos aportados por el usuario;
- almacenamiento privado del plugin;
- solicitud y devolución de foco de audition;
- comandos, eventos y suscripciones;
- layouts compatibles;
- contribuciones opcionales para la superficie WEB;
- logging y reloj monotónico mediante capacidades.

El plugin no recibirá rutas arbitrarias ni handles del sistema operativo.
RackForge entregará identificadores y operaciones acotadas:

```text
storage.plugin-data
storage.package-assets
resources.read
audio.render
midi.input
ui.little@1
ui.web
clock.monotonic
logging
```

Acceso de red saliente, procesos, dispositivos o archivos externos requerirá
capacidades separadas y estará desactivado por defecto.

## Command Bus, Event Bus y estado

Todas las entradas se traducirán a comandos tipados:

```text
SetParameter
SelectProgram
CreateProgram
SaveProgram
BeginAudition
EndAudition
Navigate
AllNotesOff
```

Los cambios aceptados producirán eventos:

```text
ParameterChanged
ProgramSelected
ProgramSaved
AuditionStarted
AuditionEnded
RouteChanged
InstanceStateChanged
```

Cada evento incluirá la instancia afectada y una revisión monotónica. Esto
permite sincronizar varias superficies, detectar ediciones obsoletas y
reconectar clientes WEB sin perder consistencia.

Los comandos del plano de control llegarán al motor de audio mediante colas
acotadas y no bloqueantes. El motor aplicará los cambios en límites seguros de
bloque.

## Superficies y controladores

### Contratos de layout

Los layouts son contratos versionados, no nombres descriptivos inferidos:

- `little@1`: header, cuerpo de dos líneas, footer y navegación mínima;
- `medium@N`: futura superficie explícitamente adaptada;
- `web@N`: futura superficie dentro del shell WEB de RackForge.

Un controlador MEDIUM no implementa LITTLE automáticamente. Debe declarar una
implementación nativa o una compatibilidad certificada y probada.

Los plugins declaran exactamente los layouts que soportan. RackForge negocia
solamente la intersección explícita entre plugin y controlador.

### Separación entre MIDI y superficie

Un dispositivo puede participar solamente como fuente MIDI:

```text
Controlador desconocido
  ├── Note/CC/Pitch/Aftertouch → permitido
  └── Display/SysEx/botones de superficie → bloqueado
```

Solo un `ControllerDriver` registrado puede:

- reconocer puertos concretos;
- abrir una salida de display;
- enviar SysEx;
- interpretar botones de navegación;
- declarar implementaciones certificadas de layouts.

### Paquetes instalables de controlador

La compatibilidad física se distribuye fuera de Core como `.rfcontroller`.
Cada paquete incluye un manifest versionado, matchers USB/endpoints,
implementaciones de layout, permisos y artefactos por plataforma.

El store conserva versiones inmutables y un registro activo separado. El nivel
de confianza (`official`, `certified`, `community`, `local`) lo asigna la
instalación y nunca el propio paquete. El host valida SHA-256 y la identidad
reportada por el driver antes de ejecutarlo.

`process-v1` permite iniciar la modularización usando procesos aislados por el
sistema operativo. `wasm-v1` será la frontera portable definitiva: RackForge
abrirá MIDI/USB y el módulo recibirá únicamente capabilities autorizadas.

## Superficie WEB

### Propiedad del shell

RackForge será dueño de:

- servidor HTTP y puerto configurable;
- autenticación y sesiones;
- SPA principal;
- router y URLs;
- header, navegación global y botón Atrás;
- estado de conexión, audio y dispositivos;
- estilos y tokens de tema;
- autorización;
- Command/Event Bus;
- ciclo de montaje y desmontaje de vistas.

Los plugins no abrirán servidores ni puertos. Contribuirán una vista que
RackForge montará dentro del área de contenido de su SPA:

```text
RACKFORGE WEB
├── Header y navegación global
├── Rutas de RackForge
└── Área del plugin
    └── RF-DLS: PLAY / CONFIG / PROGRAMS / ENVELOPE / FX
```

Las rutas pertenecerán al router de RackForge:

```text
/live
/plugins
/plugins/org.rackforge.rf-dls
/live/racks/concert/instances/layer-1/config
/live/racks/concert/instances/layer-1/programs/warm-piano
```

Las rutas apuntarán a `instance_id` cuando representen estado ejecutable. Dos
instancias del mismo plugin pueden tener programas y parámetros diferentes.

### Modos de interfaz WEB

Un plugin podrá elegir:

1. **Declarativo:** RackForge genera páginas, formularios y controles usando
   parámetros, catálogos y programas expuestos por la API.
2. **Personalizado:** el paquete aporta HTML, CSS y JavaScript para editores
   especiales, gráficas, secuenciadores o visualizadores.

La interfaz personalizada no reemplazará el shell. Podrá solicitar navegación,
pero RackForge decidirá la URL y conservará siempre la posibilidad de volver.

### Aislamiento de interfaces personalizadas

Código WEB de terceros no se importará directamente en el contexto de la SPA.
La primera arquitectura segura utilizará un `iframe sandbox` visualmente
integrado y un canal tipado basado en `MessagePort`.

El plugin no tendrá acceso directo a:

- DOM superior;
- credenciales;
- router interno;
- sockets;
- almacenamiento global;
- APIs administrativas;
- instancia WebAssembly;
- hilo de audio.

RackForge entregará solamente el contexto y las capacidades autorizadas. El
panel enviará comandos y recibirá eventos mediante el protocolo
`rackforge:web-ui@N`.

### Exposición de red

La configuración inicial será conservadora:

```toml
[web]
enabled = false
bind = "127.0.0.1"
port = 7465
```

La exposición a la red local requerirá una decisión explícita, autenticación y
límites de acceso. TLS, roles, sesiones, CSP, protección CSRF, límites de
mensajes y reconexión deberán formar parte del contrato antes de estabilizar
`web@1`.

En sistemas de escritorio o Android, la misma SPA podrá abrirse dentro de una
WebView. En una Raspberry headless podrá utilizarse desde un teléfono, tablet o
computadora de la red.

## SDK de RackForge

La API define el contrato binario; el SDK ofrece la experiencia de desarrollo.
Se crearán dos partes coordinadas.

### SDK de plugins

Inicialmente orientado a Rust:

- traits seguros sobre la ABI WebAssembly;
- bindings generados desde WIT;
- buffers preasignados para DSP;
- tipos para MIDI, audio, parámetros, programas y estado;
- almacenamiento y recursos mediante capacidades;
- macros o builders para manifiestos;
- migraciones de estado;
- harness de pruebas de tiempo real.

Otros lenguajes podrán generar bindings sin cambiar el runtime.

### SDK WEB

Un paquete TypeScript, por ejemplo `@rackforge/web-sdk`, ofrecerá:

- conexión segura con el shell;
- acceso a la instancia autorizada;
- lectura y escritura de parámetros;
- comandos y suscripciones;
- navegación solicitada;
- reconexión;
- revisiones y resolución de estado obsoleto;
- temas, idioma y accesibilidad;
- simulador fuera de RackForge.

### Herramientas

El CLI deberá incluir progresivamente:

```text
rackforge new
rackforge build
rackforge test
rackforge validate
rackforge package
rackforge inspect
rackforge dev
```

También habrá simuladores para layouts, MIDI, audio, WEB y ciclos de
reconexión, además de una suite de conformidad que todo plugin debe superar.

## Formato `.rfplugin`

El artefacto portable objetivo será independiente de CPU y sistema operativo:

```text
rf-dls.rfplugin
├── rackforge-plugin.toml
├── component.wasm
├── assets/
├── presets/
├── schemas/
├── web/
└── licenses/
```

El manifiesto declarará:

- identidad y versión;
- versión mínima/máxima de la API;
- componente ejecutable;
- capacidades requeridas y opcionales;
- layouts;
- recursos externos;
- esquemas de estado;
- contribuciones WEB;
- integridad de archivos;
- información de licencia.

Los recursos grandes o con licencias propias, como bancos DLS o ROMs, seguirán
fuera del paquete. Los datos modificables continuarán dentro del namespace:

```text
data/plugins/<plugin-id>/
```

Cada plugin decide su estructura interna. RackForge aplica aislamiento,
escrituras atómicas, cuotas, migraciones y rollback sin imponer nombres como
`programs` o `banks`.

## Compatibilidad y versionado

Se versionarán por separado:

- ABI de ejecución;
- interfaces WIT;
- protocolo de comandos y eventos;
- contratos de layout;
- protocolo WEB;
- manifiesto;
- esquema de programas;
- estado privado de cada plugin;
- formato del paquete.

Una versión mayor indica ruptura deliberada. Una versión menor solo agrega
capacidades negociables. RackForge no supondrá soporte por semejanza.

Cada versión estable tendrá:

- vectores de prueba;
- fixtures;
- validadores;
- pruebas en las plataformas soportadas;
- política de deprecación;
- guía de migración.

## Fases

### Fase 0 — Fundamentos y vocabulario

Estado: **en curso**

- [x] Separar Core, Plugin API, Control API, UI y bridge del KeyLab.
- [x] Crear programas, recursos externos y datos privados por plugin.
- [x] Crear catálogos dinámicos y selección desde LIVE.
- [x] Separar entrada MIDI de superficie registrada.
- [x] Crear contratos versionados de controlador/layout.
- [x] Registrar KeyLab Essential mk3 como `little@1`.
- [x] Hacer que RF-DLS declare `little@1`.
- [x] Crear manifests, store versionado y host genérico para `.rfcontroller`.
- [x] Instalar versiones inmutables y permitir activación/rollback atómico.
- [x] Extraer el KeyLab como primer paquete instalable con conformance suite.
- [x] Extraer el estado LITTLE a un Surface Runtime sin MIDI/SysEx.
- [ ] Mover el cliente de sesión restante fuera del proceso Arturia.
- [ ] Implementar el runtime `wasm-v1` con capabilities reales.
- [ ] Consolidar nombres: plugin, instancia, programa, recurso, superficie,
      driver, comando, evento y sesión.
- [ ] Documentar invariantes de tiempo real y ownership.

Criterio de salida: el modelo de dominio puede describir el instrumento actual
sin mencionar ALSA, KeyLab, Raspberry ni una biblioteca dinámica concreta.

### Fase 1 — Estado, comandos y eventos

Estado: **en curso**

- [x] Crear identificadores estables de instancia.
- [x] Extraer un `SessionState` autoritativo.
- [x] Definir comandos y eventos versionados.
- [x] Agregar revisiones monotónicas.
- [x] Unificar acciones de LITTLE con el Command Bus.
- [ ] Llevar edición y guardado de programas al mismo modelo.
- [x] Crear colas MIDI y de control acotadas hacia el hilo de audio.
- [x] Probar reconexión y reconstrucción completa de superficies.

Criterio de salida: LITTLE puede reiniciarse y reconstruirse desde el estado;
una segunda superficie puede observar y modificar la misma instancia sin
lógica especial en el plugin.

### Fase 2 — Contrato portable

- [ ] Diseñar los paquetes WIT iniciales.
- [ ] Definir ciclo de vida y negociación de capacidades.
- [ ] Diseñar la ABI de DSP preasignada.
- [ ] Seleccionar y encapsular el motor WebAssembly.
- [ ] Implementar límites de memoria, tiempo y fallos.
- [ ] Crear el adaptador común para plugins nativos y portables.
- [ ] Ejecutar un plugin Gain WebAssembly en Linux ARM64.
- [ ] Medir latencia, CPU, memoria y comportamiento ante fallos.

Criterio de salida: un componente Gain portable procesa audio en la Raspberry
con restricciones de tiempo real verificadas.

### Fase 3 — SDK y paquete universal

- [ ] Crear el SDK Rust desde WIT.
- [ ] Implementar `rackforge new/build/test/validate/package`.
- [ ] Definir la nueva estructura portable de `.rfplugin`.
- [ ] Implementar instalación atómica, integridad y rollback.
- [ ] Crear simuladores de audio, MIDI y layouts.
- [ ] Publicar una suite de conformidad.
- [ ] Documentar creación y migración de plugins.

Criterio de salida: un desarrollador puede crear, probar y empaquetar un plugin
sin importar módulos internos de Core ni conocer la memoria WebAssembly.

### Fase 4 — Migración de RF-DLS

- [ ] Portar el lector DLS y el motor de voces al SDK portable.
- [ ] Migrar programas custom, pitch, modulación, sustain y envelope.
- [ ] Mantener bancos DLS como recursos externos.
- [ ] Verificar equivalencia de audio con fixtures reproducibles.
- [ ] Probar edición y audition mediante Command/Event Bus.
- [ ] Comparar rendimiento nativo y WebAssembly.
- [ ] Ejecutar el mismo `.rfplugin` en Linux ARM64 y Windows.

Criterio de salida: un único paquete RF-DLS produce el mismo resultado y carga
el mismo estado en ambas plataformas.

### Fase 5 — RackForge WEB

- [ ] Crear el servidor configurable propiedad de RackForge.
- [ ] Crear la SPA, shell, router y navegación global.
- [ ] Definir el protocolo interno entre SPA y Core.
- [ ] Crear páginas globales: LIVE, PLUGINS, INSTANCIAS y CONFIG.
- [ ] Generar vistas declarativas desde la API de parámetros.
- [ ] Sincronizar WEB y LITTLE mediante eventos.
- [ ] Crear `@rackforge/web-sdk`.
- [ ] Prototipar paneles personalizados aislados.
- [ ] Implementar autenticación y exposición segura en red local.
- [ ] Estabilizar `web@1` solamente después de las pruebas.

Criterio de salida: RF-DLS puede editarse desde la SPA y el KeyLab refleja cada
cambio; el navegador puede reconectarse sin alterar ni interrumpir el audio.

### Fase 6 — Hosts de escritorio

- [ ] Implementar backends Windows de audio, MIDI y dispositivos.
- [ ] Implementar backends macOS.
- [ ] Embebir la SPA como interfaz local opcional.
- [ ] Implementar selección de dispositivos y recuperación hot-plug.
- [ ] Ejecutar la suite de conformidad en x86-64 y ARM64.
- [ ] Crear instaladores y actualizaciones.

Criterio de salida: el mismo `.rfplugin` certificado funciona en Raspberry,
Windows y macOS sin cambios del autor.

### Fase 7 — Android

- [ ] Implementar AAudio/Oboe y Android MIDI.
- [ ] Integrar almacenamiento y permisos Android.
- [ ] Ejecutar el runtime portable sin JIT obligatorio.
- [ ] Integrar la SPA en WebView.
- [ ] Probar suspensión, reanudación, desconexión USB y ahorro de energía.
- [ ] Crear empaquetado e instalación.

Criterio de salida: un dispositivo Android compatible puede alojar RackForge,
un controlador MIDI y el mismo plugin portable.

### Fase 8 — Ecosistema

- [ ] Firma opcional y procedencia de plugins.
- [ ] Repositorio y actualizaciones.
- [ ] Política de permisos visible al usuario.
- [ ] Compatibilidad automatizada por plataforma y versión.
- [ ] Crash isolation y reportes sin datos privados.
- [ ] Versionado de dependencias entre plugins.
- [ ] Documentación pública y plantillas.
- [ ] Política de publicación, licencias y recursos aportados por usuarios.

## Decisiones aplazadas deliberadamente

Se investigarán mediante prototipos antes de congelar una API:

- motor WebAssembly y estrategia AOT/JIT por plataforma;
- representación exacta de buffers de audio en memoria;
- formato definitivo de WIT;
- protocolo y seguridad de `web@1`;
- distribución y firma de plugins;
- soporte opcional para aceleradores nativos;
- compatibilidad o wrappers de estándares como CLAP, LV2 o VST;
- política de ejecución de interfaces WEB de terceros en Android y escritorio.

Estas decisiones están aplazadas, pero las fronteras que permitirán tomarlas
forman parte de las fases iniciales.

## Próximo hito

El siguiente trabajo debe ser la **Fase 1: Estado, comandos y eventos**. Esta
capa es necesaria tanto para WebAssembly como para WEB y evita que las
superficies actuales acumulen lógica específica de RF-DLS.

Después de estabilizar el modelo de sesión, el primer experimento portable será
Gain en WebAssembly. RF-DLS se migrará cuando la ruta de tiempo real haya sido
medida y validada en la Raspberry.
