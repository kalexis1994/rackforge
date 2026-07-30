# RackForge Controller API

Contratos puros y versionados para separar una fuente MIDI de una superficie de
control.

- Un puerto MIDI desconocido puede alimentar notas y controladores al motor.
- Solo un `ControllerDriver` registrado puede abrir una salida de display,
  enviar SysEx o interpretar controles de superficie.
- Los layouts nunca se infieren por resolución o cantidad de controles.
- La negociación considera únicamente implementaciones declaradas por el
  controlador y vistas declaradas por el addon.

`little@1` garantiza 18 columnas seguras, header, dos filas de cuerpo, cuatro
soft keys y las acciones Previous, Next, Confirm y Back.

Un controlador `medium@1` no obtiene compatibilidad LITTLE automáticamente.
Debe incluir y probar una implementación
`SurfaceQuality::CertifiedCompatibility`; si no la declara, un addon
exclusivamente LITTLE resulta incompatible.

La distribución dinámica vive en `rackforge-controller-package`: transforma
estos contratos en manifests `.rfcontroller`, versiones inmutables,
entrypoints por plataforma y niveles de confianza. `rackforge-controller-host`
descubre y supervisa esos paquetes; este crate continúa siendo una API pura,
sin filesystem, procesos ni conocimiento de marcas.
