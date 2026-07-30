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

Al mover cualquiera de estos controles, el paquete 0.2.1 superpone su valor en
el header nativo durante 1,5 segundos. Cada movimiento renueva el plazo; después
se restaura el header de la pantalla actual sin redibujar ni perder el cuerpo o
el footer.

El código del driver vive temporalmente en `raspberry/keylab-bridge` mientras
se completa la extracción del Surface Runtime. Esa carpeta ya compila un
artefacto propio del paquete:

```text
rackforge-arturia-keylab-essential-mk3-driver
```

No forma parte de `rackforge-core`. El host genérico lo descubre e inicia desde
el store de controladores instalado.

`keylab-bridge/fixtures/protocol-v1.json` conserva los mensajes conocidos que
usa `self-test`. La conformidad offline comprueba identidad, presets
Arturia/DAW y los siete inputs físicos sin enviar ningún byte al teclado.

La compatibilidad declarada y certificada cubre únicamente el modelo 61 que se
probó físicamente. Los modelos 49 y 88 pueden reutilizar buena parte del driver,
pero deberán agregar sus VID/PID exactos y pasar el mismo conformance suite
antes de ser reclamados por el paquete.
