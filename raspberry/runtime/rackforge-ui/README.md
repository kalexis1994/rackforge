# rackforge-ui

Framework de interfaz de hardware para RackForge. Es independiente de MIDI, SysEx,
KeyLab y del motor de audio: recibe entradas lógicas, administra componentes y
produce un frame de celdas con estilo.

## Contratos

- `Input`: siete controles físicos y sus gestos lógicos de pulsación larga y
  escape de emergencia.
- `NavigationAction`: navegación base sin imponerla a cada plugin.
- `Frame`, `Cell` y `Rect`: superficie recortada y verificable.
- `Style`: colores semánticos y rol visual.
- `Component`: identidad estable, estado, manejo de eventos y render.
- `FocusRing`: foco circular desacoplado del layout.
- `EditorState`: `OK` inicia y confirma la edición; `BACK` cancela la edición
  activa y sólo solicita salir cuando ya está en navegación.
- `TextFallback`: degradación determinista para displays sin estilos avanzados.

La paleta inicial es deliberadamente binaria:

| Estado | Fondo | Texto |
| --- | --- | --- |
| Normal | Blanco | Negro |
| Focused | Negro | Blanco |
| Pressed | Negro | Blanco |
| Disabled | Blanco | Negro |

El backend textual representa una región enfocada con corchetes. Un backend que
admita inversión o píxeles debe usar directamente `Style::FOCUSED`, sin cambiar
los componentes.

## Regiones del KeyLab

El backend oficial del KeyLab impone su propia jerarquía: header, cuerpo de dos
líneas y footer. Dentro del cuerpo, la línea 1 tiene trazo grueso y la línea 2
trazo fino. Ese peso pertenece a la línea completa y no puede expresarse por
celda con el SysEx conocido.

En consecuencia, `Style` conserva intención semántica para otros backends, pero
el layout del KeyLab debe colocar contenido principal en la línea 1 y contenido
secundario en la línea 2. Minúsculas o `Style::SECONDARY` dentro de la línea 1
no cambian físicamente el peso de la fuente.

## Componentes iniciales

- `Button`: normal, focused, pressed y disabled; activación por botón contextual
  1 (`OK`) o presión del encoder.
- `SimpleCarousel`: muestra una sola opción por pantalla en la línea 1 y su
  detalle corto en la línea 2. Flechas y rueda sustituyen esa opción sin
  animación ni vecinos visibles.
- `ValueCarousel`: muestra una sola variable en la línea 1 y su valor en la
  línea 2. `OK` o la presión del encoder entra y confirma edición; las flechas
  o la rueda modifican el valor; `BACK` cancela y restaura el original. Fuera
  de edición, `BACK` solicita salir.
- `TextEditor`: edición ASCII determinista para superficies pequeñas. Muestra
  el carácter enfocado entre corchetes; las flechas cortas cambian el carácter
  y las largas mueven el cursor. `OK` corto confirma, `OK` largo borra y `BACK`
  corto cancela la edición completa. El encoder replica las acciones cortas.
- `ConfirmationDialog`: pregunta de dos líneas con opciones recorribles;
  `OK` confirma la opción y `BACK` cierra el diálogo sin elegir. Sirve para
  decisiones reversibles como guardar o descartar un draft modificado.

El backend del KeyLab no agrega corchetes al carrusel simple: al mostrar una
sola opción no hacen falta. El carrusel de valores sí los usa para indicar si
el foco está en el nombre o en el valor editable.
- `Select`: selección circular, estado abierto/cerrado, cancelación y activación.

Los próximos componentes deben implementar el mismo trait: listas, toggles,
sliders, medidores, diálogos y editores de parámetros de plugins.

Los componentes no consumen `BACK` largo ni `HomeChord`. Son gestos reservados
por el host para volver al modo activo y forzar `HOME`, respectivamente.
