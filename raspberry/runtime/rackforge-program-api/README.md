# RackForge Program API

Contratos serializables e independientes de plataforma para programas,
peticiones de edición y documentos preparados por un addon.

El addon posee y valida el `payload`; RackForge valida el sobre común, controla
el estado de edición y persiste el documento atómicamente dentro del namespace
privado del addon.
