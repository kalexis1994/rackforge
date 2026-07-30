# RackForge Surface Runtime

Estado, navegación y composición de la interfaz `little@1` sin conocimiento de
MIDI, USB, SysEx ni modelos de controlador.

El runtime recibe `rackforge_ui::Input`, actualiza componentes y produce un
`Screen` lógico con header, dos líneas y cuatro soft keys. Un driver físico
solo traduce sus mensajes a esos inputs y codifica el `Screen` en el protocolo
del dispositivo.

La separación evita que un nuevo `.rfcontroller` tenga que copiar menús o
conocer RF-DLS. La siguiente evolución moverá también el cliente de sesión y
los adaptadores de vistas de addons desde el ejecutable Arturia hacia este
runtime, dejando en el paquete únicamente transporte y lifecycle.
