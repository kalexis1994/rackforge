# Motor Roland: Nuked-SC55

ArtuPy usa [Nuked-SC55](https://github.com/nukeykt/Nuked-SC55) como motor
de emulación Roland Sound Canvas. Emula los MCU y el chip PCM del equipo; no
es un SoundFont ni el plugin de Windows ejecutado mediante una capa de
compatibilidad.

El código está fijado como submódulo en `vendor/Nuked-SC55`. Su licencia MAME
original permite uso y redistribución no comercial, pero prohíbe construir un
producto comercial alrededor del emulador y usarlo en producción musical
comercial.

## ROMs

El emulador no incluye firmware ni ondas Roland. ArtuPy tampoco las descarga o
versiona. Deben proceder de hardware o software que el usuario pueda utilizar
legítimamente y se guardan solamente en:

```text
/home/kalex/artupy/share/nuked-sc55/
```

Para SC-55mkII se requieren:

```text
rom1.bin
rom2.bin
rom_sm.bin
waverom1.bin
waverom2.bin
```

Para SC-55mkI se requieren:

```text
sc55_rom1.bin
sc55_rom2.bin
sc55_waverom1.bin
sc55_waverom2.bin
sc55_waverom3.bin
```

`check-roms.sh mk2` o `check-roms.sh mk1` comprueba presencia y tamaño sin
modificar los archivos.

Desde Windows, un conjunto ya preparado se instala de forma segura con:

```powershell
.\raspberry\dev\install-nuked-roms.ps1 `
  -SourceDirectory "C:\ruta\a\las-roms" `
  -Model mk2
```

La herramienta exige todos los nombres esperados, rechaza archivos vacíos,
los deja con permisos privados y genera `SHA256SUMS` en la Raspberry.

## Compilación ARM64

Después de desplegar `raspberry/`:

```bash
cd /home/kalex/artupy/current/engines/nuked-sc55
bash ./build.sh
```

El resultado se instala en `/home/kalex/artupy/bin/nuked-sc55`; `back.data` se
instala junto a las ROMs en `share/nuked-sc55`.

## Arranque headless

```bash
cd /home/kalex/artupy/current/engines/nuked-sc55
bash ./run-headless.sh
```

El perfil inicial usa:

- SC-55mkII con reset GS;
- entrada RtMidi resuelta por el nombre `KL Essential 61 mk3 MIDI`;
- SDL con video dummy;
- ALSA `plughw:CARD=USB,DEV=0` para que la tasa nativa del SC-55 pueda
  convertirse a una admitida por la Scarlett;
- ocho páginas de audio de 512 muestras como valor conservador inicial.

`artupy-nuked-probe` enumera las entradas RtMidi y las salidas que ve SDL. El
daemon ArtuPy incorporará luego esta selección por identidad y supervisará las
reconexiones.

## Investigación de Sound Canvas VA

`tools/scva-inspect` cataloga y, bajo una opción explícita, extrae candidatos
de ondas de una copia legítima de Sound Canvas VA. `tools/scva-render` valida
en Windows el ABI de `SCCore.dll` mediante un render offline.

El análisis de la versión 1.1.2 confirmó bancos asociados a generaciones
SC-88/SC-8820, pero ninguna coincidencia exacta con las ROMs SC-55mkII que
espera Nuked-SC55. Los detalles reproducibles están en
`tools/scva-inspect/RESEARCH.md`; ningún dato propietario se versiona.
