# Protocolo del KeyLab Essential mk3

Registro técnico del protocolo MIDI/SysEx usado por RackForge con el firmware
oficial del Arturia KeyLab Essential 61 mk3. Última actualización:
2026-07-29.

Este documento separa deliberadamente tres niveles de evidencia:

- **Confirmado en hardware**: observado en el teclado físico del proyecto.
- **Confirmado en software público**: implementado por integraciones públicas,
  pero todavía no observado localmente.
- **Hipótesis**: pendiente de una prueba acotada o de ingeniería inversa.

## Hardware de referencia

- Modelo: KeyLab Essential 61 mk3.
- Firmware oficial: 1.2.1.
- Endpoint usado: `KL Essential 61 mk3 MIDI`, nunca `DINTHRU`, `MCU/HUI` ni
  `ALV`.
- Transporte actual: ALSA MIDI desde Raspberry Pi 4B.

Las operaciones descritas aquí pertenecen a la integración DAW oficial por
SysEx. No escriben firmware, flash, QSPI, templates ni memorias de usuario.

## Fuentes públicas

- [Guía no oficial de programación del KeyLab Essential mk3][guide], obtenida
  mediante ingeniería inversa de las integraciones de Logic Pro y FL Studio.
- [Implementación de pantalla de Bitwig][bitwig-display].
- [Tipos de marco del footer usados por Bitwig][bitwig-context].
- [Enum de pantallas generado por Arturia para la integración de FL Studio][arturia-displays].
- [Protocolo de pantalla de Ableton Live extraído de su integración oficial][ableton-display].

[guide]: https://github.com/PrzemekBarski/arturia-keylab-essential-mk3-programming-guide
[bitwig-display]: https://github.com/bitwig/bitwig-extensions/blob/main/src/main/java/com/bitwig/extensions/controllers/arturia/keylab/essentialMk3/display/LcdDisplay.java
[bitwig-context]: https://github.com/bitwig/bitwig-extensions/blob/main/src/main/java/com/bitwig/extensions/controllers/arturia/keylab/essentialMk3/display/ContextPart.java
[arturia-displays]: https://github.com/andromika/Arturia-KLE-mk3-FLstudio/blob/master/Displays.py
[ableton-display]: https://github.com/MrMatch246/KeyLab_Essential_mk3_TGE/blob/master/midi.py

## Envoltorio SysEx

Todos los comandos usan:

```text
F0 00 20 6B 7F 42 <PAYLOAD> F7
```

El payload sólo puede contener datos MIDI de 7 bits (`00..7F`).

### Sesión DAW

```text
CONNECT     02 0F 40 5A 01
DISCONNECT  02 0F 40 5A 00
```

RackForge selecciona además el programa DAW:

```text
21 11 40 02 00 01
```

El teclado devuelve exactamente ese SysEx. RackForge usa el eco como ACK real para
la adquisición y el heartbeat; aceptar una escritura ALSA no alcanza para
considerar saludable la conexión.

La aparición temprana del puerto ALSA no implica que la UI del teclado esté
lista. Después de observar una carrera real durante el arranque, el servicio
espera cinco segundos de identidad USB estable antes del primer mensaje y
separa dos segundos los reintentos de adquisición.

## Display

### Limpiar

```text
04 01 60 61
```

### Header

```text
04 01 60 01 02 <ASCII, máximo 18> 00 00
```

Confirmado en hardware. RackForge lo usa para la sección actual.

### Cuerpo de dos líneas

```text
04 01 60 12 01 <LINEA 1> 00 02 <LINEA 2> 00 00
```

Confirmado en hardware. Cada línea admite hasta 18 caracteres ASCII. El tipo
`12` permite header y footer nativos.

#### Jerarquía tipográfica nativa

Confirmado visualmente en el hardware del proyecto el 2026-07-29:

- `LINEA 1` usa el trazo grueso/principal del firmware;
- `LINEA 2` usa un trazo más fino/secundario;
- el espesor depende de la región nativa, no de mayúsculas, minúsculas,
  caracteres ni estilos de RackForge;
- el protocolo conocido no expone selección de fuente o peso dentro de una
  misma línea.

La pantalla debe modelarse como tres regiones nativas:

```text
HEADER
CUERPO: LINEA 1 (gruesa) + LINEA 2 (fina)
FOOTER
```

Regla de diseño: reservar `LINEA 1` para el valor o foco principal y usar
`LINEA 2` para opciones vecinas, contexto o información secundaria. Intentar
crear jerarquía tipográfica mezclando texto dentro de `LINEA 1` no funciona:
todo se renderiza con el mismo espesor.

### Catálogo de pantallas del firmware oficial

El archivo `Displays.py`, marcado como código auto-generado por Arturia en la
integración de FL Studio, completa el catálogo que la guía pública sólo
documenta hasta `1A`:

| ID | Nombre del enum | Naturaleza |
| ---: | --- | --- |
| `10` | `eFS_1Line` | texto |
| `11` | `e1Line` | texto |
| `12` | `e2Lines` | texto |
| `13` | `e2LinesScroll` | texto desplazable |
| `14` | `eKnob` | widget de perilla |
| `15` | `eFader` | widget de fader |
| `16` | `ePad` | widget de pad |
| `17` | `ePopup` | popup |
| `18` | `eBlinkScreen` | texto intermitente |
| `19` | `eLeftIcon` | icono predefinido y texto |
| `1A` | `eTopIcon` | icono predefinido y texto |
| `1B` | `e1InlineIcon` | icono predefinido en línea |
| `1C` | `e2InlineIcon` | dos iconos predefinidos |
| `1D` | `ePartScreen` | pantalla estructurada de partes |
| `1E` | `eFramedText` | texto enmarcado |
| `1F` | `e2InlineBlink` | variante intermitente |
| `20` | `eAutoComponent` | feedback automático de control |
| `21` | `eForceDefault` | restaura feedback predeterminado |
| `60` | `eBlankScreen` | pantalla vacía/negra |
| `61` | `eWhiteScreen` | limpieza/blanco |
| `62` | `eBorderedScreen` | borde predefinido |

El `1D` que usa Bitwig en `centerScreen()` no transporta píxeles. Su payload es
una lista corta de campos numerados y valores, coherente con `ePartScreen`.

No hay una entrada `bitmap`, `image`, `canvas`, `framebuffer` ni equivalente en
este enum. Tampoco apareció una en las integraciones públicas de Bitwig,
Ableton, FL Studio y Loopy Pro revisadas.

### Investigación de control por píxel

Estado de la evidencia para el firmware oficial 1.2.1:

| Capa | Resultado | Confianza |
| --- | --- | --- |
| Hardware/render interno | framebuffer monocromo `128×64`, 1 bit por píxel | alta |
| Tamaño de frame | `128 × 64 / 8 = 1024` bytes | alta |
| Layout interno | `buffer[(y >> 3) * 128 + x]`, bit `y & 7` | alta |
| API SysEx pública | texto, valores, iconos y widgets estructurados | alta |
| Carga de bitmap por SysEx | no encontrada | alta como ausencia en APIs conocidas; no es prueba matemática de inexistencia |
| Endpoint USB gráfico | no existe en la configuración USB de ejecución | alta, confirmado con `lsusb -v` |

El análisis offline localizó dos primitivas del renderer en el binario oficial:

- `0x0802C594`: valida `x <= 127`, `y <= 63` y pone o limpia el bit;
- `0x0802C690`: valida los mismos límites y lee el bit.
- `0x080095B8`: presenta el framebuffer completo en ocho páginas de 128 bytes;
- `0x0802C2A4`: método interno que dispara esa presentación.

Estas direcciones corresponden al firmware 1.2.1 exacto y no deben tratarse
como una ABI estable.

La rutina de presentación envía, para cada página, `B0 + page`, `10 04` y luego
128 bytes de datos. La inicialización del LCD usa comandos compatibles con la
familia ST7565. Esto cierra la ruta interna `framebuffer -> SPI1 -> LCD`, pero no
crea por sí solo una ruta desde USB/MIDI al framebuffer.

El descriptor USB de ejecución sólo presenta MIDI Streaming, además de la
interfaz DFU runtime. No presenta HID ni un endpoint bulk propietario dedicado
al display. Por tanto, una imagen enviada desde la Raspberry tendría que usar
un comando SysEx fragmentado que todavía no se ha encontrado, o una extensión
del firmware.

Conclusión operativa:

- con firmware oficial podemos componer únicamente los widgets que él expone;
- la pantalla física sí permite una gráfica ADSR real de 128×64;
- para control absoluto necesitamos encontrar un handler SysEx de bitmap aún
  desconocido o agregar uno mediante firmware modificado;
- un firmware extendido sólo necesitaría recibir 1024 bytes acotados y llamar
  al `present` ya localizado; no necesita reemplazar el renderer ni el driver
  del LCD;
- no se explorarán IDs o payloads al azar sobre el teclado.

### Prueba física de los enums de pantalla completa

Los nombres públicos `eBlankScreen (60)`, `eWhiteScreen (61)` y
`eBorderedScreen (62)` no deben interpretarse como operaciones de framebuffer.
La integración FL Studio denomina `ClearScreen()` a `eWhiteScreen` y lo
serializa así:

```text
F0 00 20 6B 7F 42 04 01 60 61 00 F7
                              ^^ cierre de payload
```

Se hicieron dos pruebas físicas el 2026-07-29:

1. Sin el `00` final, el firmware descartó el mensaje.
2. Con la serialización exacta, desapareció HOME, apareció la plantilla DAW y
   luego el servicio recuperó HOME; nunca se encendió la superficie completa.

El comportamiento concuerda con una operación que limpia o abandona la página
de contenido, no con `memset(framebuffer, 0xFF, 1024)`. El handler interno de
`0x080234E4` también remapea `61 -> 59` y `62 -> 5A` antes de delegar al
subsistema de presentación; esos enums son modos lógicos y no bytes del LCD.

Resultado: el firmware oficial sigue sin ofrecer una primitiva comprobada para
rellenar o cargar píxeles desde el host. Se retiraron los comandos experimentales
`framebuffer-test` y `screen-fill` para no presentar una capacidad inexistente.
`CLEAR_SCREEN` se conserva únicamente como parte de la restauración de sesión,
con el terminador correcto.

### Footer contextual

Comando base:

```text
04 01 60 03 <BOTON 1> <BOTON 2> <BOTON 3> <BOTON 4>
```

Cada botón usa el nibble alto como posición:

| Posición física | Base |
| --- | ---: |
| Button 1, izquierda | `10` |
| Button 2 | `20` |
| Button 3 | `30` |
| Button 4, derecha | `40` |

El nibble bajo selecciona el atributo:

| Atributo | ID para Button 1 | Datos |
| --- | ---: | --- |
| Estado/marco | `10` | `<frame> 00` |
| Texto | `11` | `<ASCII> 00` |
| Icono | `12` | `<icon_id> 00` |

Para los demás botones se suma `10`, `20` o `30`. El texto admite como máximo
7 caracteres; si se excede ese límite el botón puede no renderizarse.

Footer textual mínimo confirmado en hardware:

```text
04 01 60 03
  11 4F 4B 00
  21 3C 00
  31 3E 00
  41 42 41 43 4B 00
```

Corresponde a `OK`, `<`, `>` y `BACK`. A diferencia de escribir esos nombres
como segunda línea del cuerpo, este comando los muestra pequeños y pegados al
borde inferior.

## Estados visuales del footer

Bitwig nombra los valores así:

| Valor | Nombre público | Evidencia local |
| ---: | --- | --- |
| `00` | `NONE` | Seleccionado como estado neutro; pendiente de confirmación visual |
| `01` | `BAR` | Confirmado: dibuja un borde/línea inferior |
| `02` | `FRAME_SMALL` | Pendiente de observar |
| `03` | `FRAME_FULL` | Confirmado: dibuja un marco exterior completo |

Hallazgo importante: `FRAME_FULL` **no** significa fondo relleno. En el
firmware 1.2.1 del teclado del proyecto sólo creó un contorno. `BAR` tampoco es
un estado neutro invisible: deja una línea inferior aun sin tocar el botón.

Hasta ahora ninguna fuente pública encontrada documenta un valor que pinte
toda la superficie negra e invierta el texto. Por tanto:

- no se debe mapear `VisualState::Pressed` a `FRAME_FULL` suponiendo inversión;
- el estado normal deseado debe probarse con `NONE`;
- el relleno/inversión requiere comprobar estados adicionales de forma acotada
  o localizar la rutina correspondiente en el firmware oficial;
- no se deben recorrer indiscriminadamente los 128 valores.

## Entradas físicas

Captura confirmada en hardware, canal MIDI 1:

| Entrada | Press/giro | Release |
| --- | --- | --- |
| Button 1 | `B0 2C 7F` | `B0 2C 00` |
| Button 2 | `B0 2D 7F` | `B0 2D 00` |
| Button 3 | `B0 2E 7F` | `B0 2E 00` |
| Button 4 | `B0 2F 7F` | `B0 2F 00` |
| Encoder izquierda | `B0 74 00..3F` | — |
| Encoder neutral | `B0 74 40` | — |
| Encoder derecha | `B0 74 41..7F` | — |
| Encoder press | `B0 75 7F` | `B0 75 00` |

Los releases no ejecutan nuevamente la acción. Sí deben conservarse como
eventos para retirar feedback visual exactamente al soltar el botón.

## Contrato de navegación de RackForge

De izquierda a derecha:

```text
Button 1     Button 2     Button 3     Button 4
OK           <            >            BACK
```

En un editor:

- `OK` inicia o confirma la edición;
- `BACK` durante la edición cancela y restaura el valor original;
- `BACK` fuera de edición sale del editor;
- `<` y `>` cambian el punto o parámetro seleccionado.

### Pulsaciones largas y escape del host

Los cuatro botones producen una pulsación corta al soltarse o una pulsación
larga una vez alcanzados 650 ms. Ambos gestos son mutuamente excluyentes.

- `BACK` sostenido vuelve al modo activo de RackForge. En `PLAY` regresa al
  plugin seleccionado y le permite sugerir el programa que debe quedar centrado;
  en `LIVE` vuelve al rack o instrumento seleccionado. El modo se lee del
  estado de sesión persistido por Core y se resincroniza después de reiniciar o
  reconectar el driver.
- `OK` + `BACK`, iniciados con una diferencia máxima de 250 ms y sostenidos
  durante 650 ms, fuerzan `HOME`.
- El acorde de emergencia tiene prioridad: no genera además `OK LONG` ni
  `BACK LONG`.
- Estas dos rutas son propiedad del host. Un plugin puede ser notificado después
  de la navegación, pero no puede consumirlas, cancelarlas ni impedirlas.

### Fader y encoder reservados por el host

La definición de una memoria User instalada por MIDI Control Center asigna por
defecto el Fader 9 a CC 85. Sin embargo, RackForge adquiere la superficie en
`DAW Program`; el script de integración Ableton para este modo identifica el
Fader 9 como CC 113 por el puerto MIDI principal.[^ableton-fader] El paquete 0.1.9 declara
`channel = 0`, `controller = 113` —el canal se expresa en base cero en la API—
como `master_level`.

Desde el paquete 0.2.0, el Encoder 9 situado encima del fader se declara como
`master_pan` con `channel = 0`, `controller = 104`. Es un encoder absoluto:
RackForge convierte 0…127 a -1000…1000 y trata 60…68 como centro exacto. Esta
zona de encastre virtual facilita recuperar el centro sin depender de que el
control físico entregue exactamente 64.

El paquete 0.2.1 superpone el último valor recibido en el header durante 1,5
segundos: `MASTER VOL n%`, `MASTER PAN L n%`, `MASTER PAN R n%` o
`MASTER PAN CENTER`. El temporizador se renueva con cada movimiento. Al expirar,
el driver restaura el header actual del menú; cuerpo y footer permanecen
intactos durante toda la notificación.

Estos mensajes no son navegación ni MIDI de plugin. El driver los envía al Core
como un comando tipado y Core descarta el CC original antes de procesar el
instrumento. La registración ocurre antes de abrir el input y se repite cuando
cambia la instancia del socket de control. Tanto `master_level` como
`master_pan` son estado persistente y autoritativo de la sesión.

### Botón PART reservado por el host

La captura directa del puerto MIDI principal en hardware real confirmó que
`PART` emite CC 119 por el canal 1: valor 127 al presionar y valor 0 al soltar.
No es un selector interno silencioso. El paquete lo declara como la acción
momentánea `keyboard_parts`, separada de los controles continuos. Un toque
corto alterna la vista de partes. Mantenerlo y tocar una nota fija el split en
esa nota; el driver consume el gesto para que no se transforme además en toque
corto o largo. Mantener `PART` 1,5 segundos sin nota elimina el split. Core
reserva tanto el CC como las notas del acorde antes de cualquier ruta de plugin.

Con la pantalla adquirida en `DAW Program`, el firmware no abre su vista local
de Part, aunque sí sigue mostrando el sofisticado overlay nativo de Mod Wheel.
Esto demuestra que aquel dibujo no proviene del framebuffer de RackForge. El
atajo de Part muestra `PART 1` y `PART 2` del Rack activo; las flechas eligen la
parte y `ENTER` abre sus opciones. La nota de split pertenece a la zona derecha;
el host deriva el final de la izquierda para impedir huecos o solapamientos.

[^ableton-fader]: Implementación inspeccionada:
    [`elements.py`, revisión `ccfad86`](https://github.com/MrMatch246/KeyLab_Essential_mk3_TGE/blob/ccfad86570a66f419f323a080d6cd20ad1c76f6c/elements.py#L143-L145).

El editor de texto reutilizable asigna `OK` corto a entrar o confirmar el texto,
`<` y `>` cortos a cambiar el carácter, y sus pulsaciones largas a mover el
cursor. `OK` largo borra el carácter actual y `BACK` corto cancela toda la
edición restaurando el original. Al mover el cursor a la derecha desde el final
se crea un nuevo espacio editable. El giro y la presión del encoder replican
las acciones cortas. `BACK` largo y el acorde de emergencia conservan siempre
el significado global.

## Próxima prueba visual

La siguiente prueba debe ser reversible y limitada al display:

1. usar `NONE (00)` en reposo para confirmar que elimina la barra;
2. observar `FRAME_SMALL (02)` una sola vez;
3. si tampoco rellena, probar únicamente un rango pequeño de valores
   inmediatamente posterior a los documentados, uno por vez;
4. restaurar el footer conocido después de cada muestra y ante timeout;
5. detenerse si el display ignora un valor, parpadea o pierde el layout.

Si ningún estado soportado produce relleno negro, el feedback deberá usar el
mejor fallback oficial (marco o LED contextual) hasta comprender mejor el
renderer del firmware. No hace falta reemplazar el firmware para continuar con
el resto de RackForge.
