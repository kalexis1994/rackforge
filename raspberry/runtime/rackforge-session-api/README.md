# RackForge Session API

Modelo puro, serializable e independiente de plataforma para describir una
sesión RackForge.

La sesión es la fuente de verdad compartida por LITTLE, la futura superficie
WEB y cualquier otra interfaz. Los inputs producen `SessionCommand`; los
cambios aceptados producen `SessionEvent` con una revisión monotónica.
Cada comando lleva un `client_id` y un `command_id`, por lo que eventos de
varias superficies pueden correlacionarse sin colisiones.

Este crate no conoce ALSA, USB, KeyLab, sockets ni bibliotecas dinámicas.
