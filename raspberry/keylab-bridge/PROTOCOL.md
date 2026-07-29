# Protocolo del KeyLab Essential mk3

Registro técnico del protocolo MIDI/SysEx usado por ArtuPy con el firmware
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

[guide]: https://github.com/PrzemekBarski/arturia-keylab-essential-mk3-programming-guide
[bitwig-display]: https://github.com/bitwig/bitwig-extensions/blob/main/src/main/java/com/bitwig/extensions/controllers/arturia/keylab/essentialMk3/display/LcdDisplay.java
[bitwig-context]: https://github.com/bitwig/bitwig-extensions/blob/main/src/main/java/com/bitwig/extensions/controllers/arturia/keylab/essentialMk3/display/ContextPart.java

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

ArtuPy selecciona además el programa DAW:

```text
21 11 40 02 00 01
```

El teclado devuelve exactamente ese SysEx. ArtuPy usa el eco como ACK real para
la adquisición y el heartbeat; aceptar una escritura ALSA no alcanza para
considerar saludable la conexión.

## Display

### Limpiar

```text
04 01 60 61
```

### Header

```text
04 01 60 01 02 <ASCII, máximo 18> 00 00
```

Confirmado en hardware. ArtuPy lo usa para la sección actual.

### Cuerpo de dos líneas

```text
04 01 60 12 01 <LINEA 1> 00 02 <LINEA 2> 00 00
```

Confirmado en hardware. Cada línea admite hasta 18 caracteres ASCII. El tipo
`12` permite header y footer nativos.

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

## Contrato de navegación de ArtuPy

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
el resto de ArtuPy.
