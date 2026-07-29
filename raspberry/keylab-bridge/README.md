# artupy bridge (Rust)

Puente seguro entre artupy y el Arturia KeyLab Essential 61 mk3.

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

## Modelo de menú

`src/menu.rs` mantiene navegación y presentación separadas del transporte
SysEx. La jerarquía inicial es:

- `HOME`: LIVE, PLAY y CONFIG;
- `LIVE`: performances ordenadas para tocar;
- `PLAY`: navegador directo de plugins;
- `CONFIG`: instrumentos, setlists, audio y sistema;
- `INSTRUMENT`: rack y parámetros propios del instrumento.

El renderer produce header, dos líneas ASCII de hasta 18 caracteres y un footer
contextual nativo. Las acciones abstractas `Previous`, `Next`, `Back` y
`Select` permanecen separadas del transporte MIDI.

El header superior pertenece al estado de navegación y muestra `HOME`, `LIVE
SET`, `PLAY` o `CONFIG` y, cuando corresponde, su posición. Al entrar a un
instrumento o editor inmersivo cambia explícitamente a modo fullscreen y oculta
el header. Las dos líneas principales quedan reservadas para componentes: HOME
usa botones grandes en dos filas y las acciones aparecen pequeñas en el footer
oficial, alineadas con los botones físicos.

La interfaz física pública conserva siete entradas independientes:
`Button1..Button4`, `EncoderLeft`, `EncoderRight` y `EncoderPress`. De izquierda
a derecha, la navegación base asigna los botones a `OK`, `<`, `>` y `BACK`,
mientras la rueda navega y confirma. Una pantalla o un plugin podrá reemplazar
esas acciones contextualmente sin modificar el lector MIDI.

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

Después de cada detección, el bridge exige medio segundo de identidad USB
estable. Luego pulsa el handshake DAW/OLED hasta recibir del propio KeyLab el
SysEx que confirma `DAW Program`; no declara éxito sólo porque ALSA aceptó el
envío. En estado activo revalida ese ACK cada seis segundos y vuelve a
adquisición después de dos respuestas perdidas.

`systemd/artupy-display.service` lo inicia automáticamente con el sistema. Su
`ExecStopPost` ejecuta `restore --execute`, por lo que un apagado o una detención
normal del servicio devuelve el teclado al programa Arturia oficial.

```bash
cd /home/kalex/artupy/current/keylab-bridge
cargo build --release --bin artupy-bridge
bash ./install.sh
systemctl status artupy-display.service
```

En Windows se usa explícitamente el toolchain MSVC para evitar depender de
`dlltool.exe` del entorno GNU.
