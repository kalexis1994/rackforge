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
