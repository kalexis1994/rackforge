# artupy-ui

Framework de interfaz de hardware para ArtuPy. Es independiente de MIDI, SysEx,
KeyLab y del motor de audio: recibe entradas lógicas, administra componentes y
produce un frame de celdas con estilo.

## Contratos

- `Input`: las siete entradas físicas públicas de ArtuPy.
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

## Componentes iniciales

- `Button`: normal, focused, pressed y disabled; activación por botón contextual
  1 (`OK`) o presión del encoder.
- `Select`: selección circular, estado abierto/cerrado, cancelación y activación.

Los próximos componentes deben implementar el mismo trait: listas, toggles,
sliders, medidores, diálogos y editores de parámetros de plugins.
