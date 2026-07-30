# RackForge Arturia controller driver

Driver empaquetable y seguro entre RackForge y el Arturia KeyLab Essential 61
mk3. El binario pertenece al paquete
`org.rackforge.arturia-keylab-essential-mk3.rfcontroller`; el host genérico no
contiene identificación ni protocolo Arturia.

El bridge contiene el primer `ControllerDriver` certificado. El perfil
`arturia.keylab-essential-mk3` ofrece exclusivamente `little@1`; la apertura de
la sesión SysEx vuelve a validar driver y layout incluso cuando el usuario
indica un puerto manualmente. Un endpoint desconocido nunca recibe comandos de
pantalla.

La primera etapa reproduce la prueba de pantalla con Rust y `midir`. Detecta
automáticamente el endpoint terminado en `MIDI`, hace dry-run por defecto y
restaura la pantalla y el programa Arturia al finalizar.

```powershell
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- list
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- demo
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- demo --execute --seconds 30
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- menu-demo --execute --seconds 30
```

No actualiza firmware ni escribe templates o memorias de usuario. El cambio al
programa DAW solo dura durante la sesión.

Los enums públicos `eBlankScreen`, `eWhiteScreen` y `eBorderedScreen` no son
una API de framebuffer. En hardware, `eWhiteScreen` se comporta como
`ClearScreen`: abandona la página de contenido y reaparece la plantilla DAW; no
enciende todos los píxeles. El bridge no expone comandos con nombres que
sugieran control de píxeles mientras esa ruta no exista.

## Modelo de menú

`src/menu.rs` mantiene navegación y presentación separadas del transporte
SysEx. La jerarquía inicial es:

- `HOME`: LIVE, PLAY y CONFIG;
- `LIVE`: modo superior de RackForge para performances y racks ordenados;
- `PLAY`: navegador de addons que abre la sección de ejecución del elegido;
- `CONFIG`: addons, setlists, audio y sistema;
- `ADDONS`: selección del addon que se va a configurar;
- `RF-DLS PLAY`: selector de colección `DLS` o `CUSTOM`, seguido por sus
  programas tocables;
- `RF-DLS CONFIG`: `ADD NEW`, los CUSTOM existentes y las secciones `NAME`,
  `LAYER A`, `LAYER B`, `FX`, `OUTPUT` y `SAVE`. Cada layer contiene sus
  propias secciones `TIMBRE`, `AMP ENV`, `PITCH ENV`, `LFO`, `TUNING`, `RANGE`
  y `VOLUME`.

`LIVE` nunca pertenece a RF-DLS ni a otro addon: carga programas de RackForge
que pueden combinar varias instancias y efectos. RF-DLS tiene dos rutas
independientes: `PLAY → RF-DLS → RF-DLS PLAY` y
`CONFIG → ADDONS → RF-DLS → RF-DLS CONFIG`.

Los selectores PLAY y ADDONS contienen únicamente addons descubiertos como
instalados. No muestran placeholders, opciones deshabilitadas ni estados
`Installed`/`Not installed`. ADDONS tampoco usa subtítulo: el nombre de cada
addon es información suficiente.

En RF-DLS PLAY, nombres y posiciones provienen del catálogo dinámico publicado
por el addon a Core. El bridge consulta `live-control.sock` mediante
`rackforge-control-api` y envía una orden genérica `SelectSound` al confirmar
con OK. Nunca abre el `.dls`, no conoce bancos MIDI y normaliza texto no ASCII
a los 18 caracteres seguros del display.

La entrada al editor adquiere una lease exclusiva de audition. En `ADD NEW` el
nombre provisional se genera como `CUSTOM NNN` y se preselecciona el primer
timbre DLS; así el draft nunca carece de los dos requisitos mínimos de guardado.
El bridge renueva la lease con su heartbeat. `BACK`, una desconexión o el
timeout del motor devuelven el foco y restauran el sonido anterior.

Cada CUSTOM comienza con una capa `A` obligatoria, que no se puede desactivar.
`LAYER B` abre primero su menú `ENABLED`; mientras esté en `OFF` no muestra
`TIMBRE` ni genera voces. `OK` sobre `ENABLED` alterna directamente ON/OFF, sin
abrir otro editor. Al pasar a `ON`, el bridge crea la capa copiando la
configuración de `A` y agrega `TIMBRE` como opción hermana del mismo carrusel;
recién `OK` sobre `TIMBRE` abre la lista DLS. Volver a `OFF` conserva su fuente
y overrides para poder reactivarla sin perder el trabajo.

Las configuraciones de síntesis nunca dependen del último layer visitado:
viven dentro del carrusel de `LAYER A` o `LAYER B`. `VOLUME` mezcla ese layer
de forma independiente (`0.00x` lo silencia y `1.00x` conserva su nivel);
después se suman A+B, pasan por una única cadena `FX` compartida y finalmente
por `OUTPUT`. En esta etapa `OUTPUT` ya modifica el gain persistente del
programa y `FX` muestra honestamente una cadena vacía hasta que exista el motor
de efectos.
Los editores de envolventes, LFO, tuning, rango y volumen actúan sobre la última
capa seleccionada. Los campos opcionales muestran `INHERIT`; al editarlos se
crea el override y al bajar del mínimo vuelven a heredar el valor DLS.

Core preescucha el documento completo después de cada cambio, no sólo el timbre
base. Por eso las capas, splits de tecla/velocidad y overrides se pueden tocar
desde el KeyLab antes de guardar. El addon valida y normaliza el JSON en cada
reemplazo; la superficie sólo modifica campos permitidos.

Los carruseles numéricos mantienen una working copy auditiva. Mientras están en
edición, cada paso de flecha o encoder envía un preview transitorio al Core; se
puede tocar inmediatamente para oír el resultado. `OK` confirma el valor en el
draft y recién entonces lo marca dirty. `BACK` restaura tanto el valor visual
como el audio del documento confirmado. El transporte síncrono aplica
backpressure, por lo que nunca acumula una cola ilimitada de movimientos.

Core conserva el estado `dirty` publicado para el draft del addon. Al intentar
abandonar un programa modificado, tanto con `BACK` desde sus secciones como con
`BACK` sostenido, el bridge muestra `SAVE CHANGES?` y permite elegir `SAVE` o
`DISCARD`; `BACK` cierra el diálogo y continúa editando. El acorde de emergencia
`OK` + `BACK` es la única excepción deliberada: fuerza `HOME` y libera el draft
de forma best-effort para que un addon defectuoso no pueda atrapar al usuario.

La envolvente no es una configuración global. Sus valores pertenecen al
programa CUSTOM abierto desde CONFIG. A medida que el menú se conecte con
`rackforge-core`, cada addon declarará sus páginas y parámetros mediante la
Plugin API en lugar de agregarlos al árbol global.

El renderer produce header, dos líneas ASCII de hasta 18 caracteres y un footer
contextual nativo. Las acciones abstractas `Previous`, `Next`, `Back` y
`Select` permanecen separadas del transporte MIDI.

El cuerpo no es tipográficamente homogéneo: en el teclado físico, la primera
línea usa el texto grueso/principal del firmware y la segunda usa un texto más
fino/secundario. No existe un selector de peso por carácter. Por eso los
componentes deben poner el foco en la línea 1 y trasladar vecinos o contexto a
la línea 2 cuando necesiten una jerarquía visual real.

El header superior es una región opcional. La navegación puede usarlo para
`HOME`, `LIVE SET`, `PLAY` o `CONFIG`; una página declarada por un plugin puede
pedirlo mediante la Plugin API. RF-DLS distingue `RF-DLS PLAY` de
`RF-DLS CONFIG`, y el editor inmersivo de envolvente puede ocultarlo para
recuperar espacio.

Las listas usan dos componentes sin vecinos visibles. El carrusel simple
muestra la opción actual en la línea 1 y una descripción breve en la línea 2.
El carrusel de valores muestra el nombre del parámetro en la línea 1 y su valor
en la línea 2; el foco baja al valor durante la edición y vuelve al nombre al
confirmar o cancelar. Los desplazamientos reemplazan el contenido directamente,
sin animación, vecinos ni indicadores `v/^`. El carrusel simple no agrega
corchetes. El carrusel de valores sí los conserva para mostrar si el foco está
en el nombre o en el valor editable.

La interfaz física pública conserva siete entradas independientes:
`Button1..Button4`, `EncoderLeft`, `EncoderRight` y `EncoderPress`. De izquierda
a derecha, la navegación base asigna los botones a `OK`, `<`, `>` y `BACK`,
mientras la rueda navega y confirma. Una pantalla o un plugin podrá reemplazar
esas acciones contextualmente sin modificar el lector MIDI.

Los cuatro botones también producen gestos lógicos de pulsación corta y larga.
El umbral de pulsación larga es 650 ms: la acción corta se resuelve al soltar y
nunca se dispara después de una acción larga. `BACK` sostenido pertenece al
host y vuelve al modo activo (`LIVE` o `PLAY`); el addon recibe después una
notificación de activación y puede sugerir qué elemento centrar, pero no puede
bloquear ni reemplazar la navegación.

`OK` + `BACK`, presionados con una diferencia máxima de 250 ms y sostenidos
650 ms, forman el escape de emergencia. Tiene prioridad sobre las pulsaciones
largas individuales, lleva inmediatamente a `HOME` y solicita la cancelación
del draft activo de forma best-effort. La pantalla vuelve a ser controlada por
RackForge incluso si el addon no responde.

El mapeo capturado en hardware real es:

| Entrada | Mensaje |
| --- | --- |
| Button 1–4, izquierda a derecha | CC 44–47, valor 127 |
| Encoder izquierda | CC 116, valor menor que 64 |
| Encoder derecha | CC 116, valor mayor que 64 |
| Encoder press | CC 117, valor 127 |

Los valores cero de los botones son liberaciones: no generan una segunda
acción, pero se conservan para retirar feedback visual al soltar. El valor 64
del encoder relativo es neutral.

Los comandos, límites y resultados observados en hardware se registran en
[`PROTOCOL.md`](PROTOCOL.md). En particular, `BAR (01)` dibuja una línea
inferior y `FRAME_FULL (03)` sólo dibuja un contorno; ninguno produce por sí
solo el relleno negro de un botón presionado.

## Pantalla persistente

En la Raspberry, `serve --execute` espera al KeyLab aunque todavía no esté
conectado, toma la sesión OLED al detectarlo y mantiene HOME visible. Si se
desenchufa, detecta el cambio de instancia física mediante `sysfs`, descarta la
ruta ALSA anterior y recupera la pantalla con una sesión MIDI nueva al
reconectarse. Esto evita depender de errores de escritura: ALSA puede aceptar
mensajes destinados a una suscripción que ya desapareció.

Después de cada detección, el bridge exige cinco segundos continuos de identidad
USB estable antes de abrir una sesión o enviar el primer byte. Esto evita tomar
la pantalla mientras el firmware todavía inicializa USB y su UI. Luego pulsa el
handshake DAW/OLED hasta recibir del propio KeyLab el SysEx que confirma
`DAW Program`; los reintentos quedan separados por dos segundos y no declara
éxito sólo porque ALSA aceptó el envío. En estado activo revalida ese ACK cada
seis segundos y vuelve a adquisición después de dos respuestas perdidas.

`systemd/rackforge-controller-host.service` inicia el host genérico. El host
descubre este paquete desde el store versionado, supervisa su proceso y ejecuta
`restore` durante el cierre, devolviendo el teclado al programa Arturia oficial.

```bash
cd /home/kalex/rackforge/current/keylab-bridge
cargo build --release --bin rackforge-arturia-keylab-essential-mk3-driver
cd ../runtime
cargo build --release --bin rackforge-controller-host
cd ../controllers
bash ./install.sh
systemctl status rackforge-controller-host.service
```

En Windows se usa explícitamente el toolchain MSVC para evitar depender de
`dlltool.exe` del entorno GNU.
