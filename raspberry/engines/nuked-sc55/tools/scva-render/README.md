# scva-render

Prueba aislada del ABI exportado por `SCCore.dll`. Carga la DLL de una copia
legítima de Sound Canvas VA, envía Program Change 0 y una nota C4, renderiza
cuatro segundos estéreo a 44,1 kHz y escribe un WAV float de 32 bits.

El ejecutable es sólo una sonda de investigación para Windows x64. No convierte
la DLL a ARM ni redistribuye código o bancos propietarios.

```powershell
cargo run --release -- `
  "C:\ruta\SCCore.dll" `
  "C:\ruta\scva-c4.wav"
```

El proceso usa como directorio de trabajo la carpeta de la DLL, para que el
motor pueda encontrar los archivos auxiliares de la instalación.
