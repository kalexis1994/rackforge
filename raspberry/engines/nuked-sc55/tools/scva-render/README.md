# scva-render

Prueba aislada del ABI exportado por `SCCore.dll`. Carga la DLL de una copia
legítima de Sound Canvas VA, envía un Program Change y una nota configurables,
renderiza cuatro segundos estéreo a 44,1 kHz y escribe un WAV float de 32 bits.

El ejecutable es sólo una sonda de investigación para Windows x64. No convierte
la DLL a ARM ni redistribuye código o bancos propietarios.

```powershell
cargo run --release --bin scva-render -- `
  "C:\ruta\SCCore.dll" `
  "C:\ruta\scva-c4.wav"
```

Para crear oráculos reproducibles de distintos programas y notas:

```powershell
cargo run --release --bin scva-render -- `
  --program 0 --note 60 --velocity 100 `
  "C:\ruta\SCCore.dll" `
  "C:\ruta\piano-c4.wav"
```

El modo de investigación `--patch-u32 OFFSET VALUE` nunca modifica el binario
original: crea una copia temporal junto a la DLL, cambia cuatro bytes antes de
cargarla y elimina la copia al terminar. Sirve para comprobar qué campos de las
tablas estáticas afectan al render:

```powershell
cargo run --release --bin scva-render -- `
  --patch-u32 0x18f57bc 0x753 `
  "C:\ruta\SCCore.dll" `
  "C:\ruta\probe.wav"
```

Para comparar los primeros cuatro acumuladores DPCM con la rutina interna de
SCCore 1.1.2 sin reproducir audio:

```powershell
cargo run --release --bin scva-decode-oracle -- `
  "C:\ruta\SCCore.dll" "C:\ruta\wave_bank.bin" `
  2 0x290a0 0x2e8db 0x311df
```

Se pueden agregar una cantidad de frames y un WAV de salida para ejecutar
también el avance y el interpolador internos:

```powershell
cargo run --release --bin scva-decode-oracle -- `
  "C:\ruta\SCCore.dll" "C:\ruta\wave_bank.bin" `
  0 0x74ee0 0x7ed23 0x836de 32768 "C:\temp\native-sample-7.wav"
```

El proceso usa como directorio de trabajo la carpeta de la DLL, para que el
motor pueda encontrar los archivos auxiliares de la instalación.
