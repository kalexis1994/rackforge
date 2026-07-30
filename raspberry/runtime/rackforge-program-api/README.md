# RackForge Program API

Contratos serializables e independientes de plataforma para programas,
peticiones de edición y documentos preparados por un plugin.

El plugin posee y valida el `payload`; RackForge valida el sobre común, controla
el estado de edición y persiste el documento atómicamente dentro del namespace
privado del plugin.

La edición visual usa un árbol declarativo versionado (`ProgramEditorView`).
Cada página contiene subpáginas y campos tipados (`toggle`, `number`, `choice`
o `sound`). Los números viajan como enteros con una cantidad explícita de
decimales para evitar diferencias de coma flotante entre plataformas.

Una superficie modifica un borrador enviando únicamente
`ProgramFieldEditRequest { field_id, value }`. El `field_id` es opaco para el
host: sólo el plugin conoce la ruta real dentro de su `payload`, aplica la
mutación, valida todas las invariantes y devuelve un `PreparedProgram`
canónico. El campo también declara si admite preview auditivo transitorio.
