# scva-inspect

Herramienta Rust de análisis estático para archivos de una copia legítima de
Roland Sound Canvas VA.

La primera fase:

- identifica arquitectura y secciones PE;
- calcula hashes SHA-256 y entropía;
- enumera DLL importadas y símbolos exportados;
- localiza términos relevantes;
- compara candidatos del tamaño esperado con hashes conocidos de SC-55mkII.
- reconoce el layout de ondas observado en `SCCore.dll` 1.1.2;
- opcionalmente extrae candidatos con offsets y hashes verificables.

No carga ni ejecuta las DLL. El modo normal es de sólo lectura.

```powershell
cargo run --release -- `
  "C:\ruta\SCCore.dll" `
  "C:\ruta\SOUND Canvas VA.dll"
```

El dumpeo es explícito, rechaza directorios de salida que no estén vacíos y
escribe cada archivo mediante un temporal:

```powershell
cargo run --release -- `
  --dump-waves "C:\ruta\salida-vacia" `
  "C:\ruta\SCCore.dll"
```

La salida es material propietario derivado de la copia del usuario: debe quedar
fuera del repositorio y no se debe redistribuir.

Las tablas de control se extraen por separado y sólo para la versión exacta
1.1.2 verificada por SHA-256:

```powershell
cargo run --release -- `
  --dump-control "C:\ruta\control-vacio" `
  "C:\ruta\SCCore.dll"
```

El resolvedor estático sigue un tono y una nota MIDI a través de sus parciales,
mapas de onda y descriptores de muestra:

```powershell
cargo run --release -- `
  --resolve-tone 0 60 "C:\ruta\SCCore.dll"
```

Para buscar instrucciones x86-64 con referencias RIP-relative a un rango del
archivo PE:

```powershell
cargo run --release -- `
  --xrefs 0x966c0 0x100 "C:\ruta\SCCore.dll"
```

Los destinos que viven en la zona BSS de una sección no tienen offset físico.
Para ellos se usa directamente el RVA:

```powershell
cargo run --release -- `
  --xrefs-rva 0x1a1b7e8 8 "C:\ruta\SCCore.dll"
```

Para localizar llamadas y saltos directos a una función:

```powershell
cargo run --release -- `
  --callers-rva 0x3690 "C:\ruta\SCCore.dll"
```

`--pointers-to` busca además punteros VA de 64 bits y RVA de 32 bits almacenados
en las secciones de datos:

```powershell
cargo run --release -- `
  --pointers-to 0x18f57b0 0x100 "C:\ruta\SCCore.dll"
```

También puede desensamblar una ventana por RVA:

```powershell
cargo run --release -- `
  --disasm-rva 0x60170 0x100 "C:\ruta\SCCore.dll"
```
