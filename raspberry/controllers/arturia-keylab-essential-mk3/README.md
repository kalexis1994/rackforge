# Arturia KeyLab Essential mk3 controller package

Primer paquete de controlador de RackForge. Implementa el display de texto,
cuatro soft keys, encoder, LEDs, handshake DAW, health checks y restauración
usando exclusivamente MIDI/SysEx soportado por el firmware oficial.

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
