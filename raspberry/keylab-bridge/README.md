# artupy bridge (Rust)

Puente seguro entre artupy y el Arturia KeyLab Essential 61 mk3.

La primera etapa reproduce la prueba de pantalla con Rust y `midir`. Detecta
automáticamente el endpoint terminado en `MIDI`, hace dry-run por defecto y
restaura la pantalla y el programa Arturia al finalizar.

```powershell
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- list
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- demo
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\raspberry\keylab-bridge\Cargo.toml -- demo --execute --seconds 30
```

No actualiza firmware ni escribe templates o memorias de usuario. El cambio al
programa DAW solo dura durante la sesión.

En Windows se usa explícitamente el toolchain MSVC para evitar depender de
`dlltool.exe` del entorno GNU.
