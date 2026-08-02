# Arturia KeyLab Essential mk3 controller package

Primer paquete de controlador de RackForge. Implementa el display de texto,
cuatro soft keys, encoder, LEDs, handshake DAW, health checks y restauración
usando exclusivamente MIDI/SysEx soportado por el firmware oficial.

El Fader 9 queda reservado para `master_level`: en `DAW Program`, requerido por
la integración del display, envía CC 113 por el canal 1 y controla la ganancia
final de RackForge. La reserva forma parte del manifest y del perfil reportado
por el driver; ese CC se consume en Core y no llega a los plugins.

El Encoder 9, situado encima del fader, queda reservado para `master_pan` mediante
CC 104 por el canal 1. Los valores MIDI 60–68 forman una zona de encastre virtual
en el centro; Core conserva el balance en la sesión, lo restaura desde el
checkpoint y aplica el cambio suavemente después del audio del plugin.

El botón físico `PART` se declara como la acción momentánea reservada
`keyboard_parts`: en el hardware probado emite CC 119, valor 127 al presionar y
0 al soltar. Una pulsación corta alterna la vista `PART`: las flechas eligen
`PART 1/2`, `ENTER` abre su configuración y otra pulsación de `PART` la cierra.
Mantener `PART` y tocar una nota guarda el split inmediatamente; esa nota es la
primera de la zona derecha. Mantenerlo 1,5 segundos sin tocar una nota elimina
el split, deja `PART 1` a rango completo y desactiva `PART 2`. Las notas usadas
en el gesto se consumen antes del ruteo musical.

La configuración de Part contiene únicamente canal MIDI, split compartido y
octava. RackForge transforma primero el input físico a CH1/CH2 y recién después
evalúa los filtros de los Slots. Plugin, volumen, pan y salida de audio siguen
siendo propiedades del Slot y el orden de los Slots no define las partes.

La ilustración que aparece al mover Mod Wheel es un overlay nativo del firmware
Arturia, no una imagen dibujada por RackForge. El firmware oficial no muestra su
editor local de Part mientras la sesión DAW mantiene la pantalla; RackForge
ofrece esa función con sus propios menús de texto y conserva la configuración
en el Rack.

Al mover cualquiera de estos controles, el paquete 0.2.1 superpone su valor en
el header nativo durante 1,5 segundos. Cada movimiento renueva el plazo; después
se restaura el header de la pantalla actual sin redibujar ni perder el cuerpo o
el footer.

El código del driver vive en `hardware/keylab-bridge` mientras
se completa la extracción del Surface Runtime. Esa carpeta ya compila un
artefacto propio del paquete:

```text
rackforge-arturia-keylab-essential-mk3-driver
```

No forma parte de `rackforge-core`. El host genérico lo descubre e inicia desde
el store de controladores instalado.

`hardware/keylab-bridge/fixtures/protocol-v1.json` conserva los mensajes conocidos que
usa `self-test`. La conformidad offline comprueba identidad, presets
Arturia/DAW y los siete inputs físicos sin enviar ningún byte al teclado.

La compatibilidad declarada y certificada cubre únicamente el modelo 61 que se
probó físicamente. Los modelos 49 y 88 pueden reutilizar buena parte del driver,
pero deberán agregar sus VID/PID exactos y pasar el mismo conformance suite
antes de ser reclamados por el paquete.
