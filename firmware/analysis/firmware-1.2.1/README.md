# Mapa provisional del KeyLab Essential 61 mk3

Este análisis es **offline y de solo lectura**. Se hizo sobre el firmware oficial
instalado por Arturia MIDI Control Center. Hasta ahora no se ha enviado ningún
comando DFU, no se ha entrado al bootloader y no se ha escrito flash.

## Evidencia analizada

- Paquete: `keylab-essential-61-mk3_Firmware_Update_1.2.1.kle3`
- Versión declarada: 1.2.1, fecha 2025-08-27
- Método declarado por `info.json`: `dfu`
- Binario de 61 teclas: `Kle3_fw_2__fw1_2_1_746__2025_08_27.bin`
- Copia local: `keylab-essential-61-mk3.bin`
- SHA-256 del paquete: `C57604BEBB688F3C03D508FF5A24171FB142331EDD64A1CDDE815AAD06DBFC93`
- SHA-256 del binario: `819258D9EFE53E5E5026489F097E3E0DC9F132FD051EE00D28614000D2965269`

Los payloads de los modelos de 49, 61 y 88 teclas son idénticos. Solamente
cambian los identificadores de producto de la cabecera:

| Modelo | USB PID |
|---|---:|
| 49 mk3 | `0x024C` |
| 61 mk3 | `0x028C` |
| 88 mk3 | `0x02CC` |

## Procesador

La combinación de vector Cortex-M, límites de SRAM y direcciones de periféricos
coincide con la familia **Nations Technologies N32G455**:

- ARM Cortex-M4F
- hasta 144 MHz
- FPU de precisión simple
- caché de instrucciones de 8 KiB
- USB Full Speed
- controlador QSPI con ventana XIP

La familia está identificada con confianza alta. El número exacto de parte y el
encapsulado siguen pendientes de leer la serigrafía física del chip.

## Cabecera y mapa de memoria inferido

La cabecera Arturia ocupa 64 bytes. El vector Cortex-M comienza en el offset
`0x40` del archivo.

| Campo | Valor |
|---|---:|
| VID | `0x1C75` |
| PID (61) | `0x028C` |
| Longitud del payload | 193.348 bytes |
| Base de aplicación | `0x08007800` |
| Fin declarado | `0x0803FFF7` |
| Stack pointer inicial | `0x20024000` |
| Reset handler | `0x08008DC1` |

Mapa resultante:

| Región | Rango | Capacidad |
|---|---|---:|
| Flash interna total | `0x08000000..0x0803FFFF` | 256 KiB / 0,25 MiB |
| Bootloader/reservado | `0x08000000..0x080077FF` | 30 KiB |
| Slot de aplicación | `0x08007800..0x0803FFFF` | 226 KiB |
| Payload actual | desde `0x08007800` | 188,82 KiB |
| Margen nominal del slot | — | 37,18 KiB |
| SRAM principal + retention | `0x20000000..0x20023FFF` | 144 KiB / 0,140625 MiB |

El valor `0x20024000` es el primer byte posterior a la SRAM, que es el valor
normal que se coloca como stack pointer inicial en un Cortex-M descendente.

### Checksums de la imagen

La cabecera contiene dos checksums aditivos encadenados:

| Offset | Regla |
|---:|---|
| `0x0A` | complemento a dos de 8 bits de la suma de todos los bytes del payload |
| `0x3F` | complemento a dos de 8 bits de la suma de los primeros 63 bytes de cabecera |

En otras palabras:

```text
(sum(payload) + header[0x0A]) & 0xFF == 0
sum(header[0x00:0x40]) & 0xFF == 0
```

La regla del payload se confirmó en tres imágenes oficiales independientes:

| Imagen | Suma del payload | `header[0x0A]` |
|---|---:|---:|
| KeyLab Essential mk3 1.2.1 | `0x27` | `0xD9` |
| MiniLab 3 1.2.0, PID `0x020B` | `0xEF` | `0x11` |
| MiniLab 3 1.2.0, PID `0x220B` | `0x1A` | `0xE6` |

El byte `0x0B` vale `0x01` en las tres imágenes y todavía no tiene semántica
confirmada.

## Periféricos observados

El binario contiene referencias alineadas compatibles con el mapa N32G455:

| Periférico | Base | Referencias |
|---|---:|---:|
| RCC | `0x40021000` | 25 |
| Controlador flash | `0x40022000` | 9 |
| USB FS | `0x40005C00` | 2 |
| SPI1 | `0x40013000` | 2 |
| Registros QSPI | `0xA0001000` | 0 |

Las cinco apariciones alineadas de `0x90000000`, inicio de la ventana QSPI XIP,
caen dentro de datos gráficos/bitmap y no constituyen evidencia de accesos QSPI.
El firmware no parece configurar QSPI. Esto no demuestra que el chip de memoria
externa no exista; hay que inspeccionar ambas caras de la placa.

## Display y framebuffer

El firmware contiene un renderer monocromo con coordenadas de `128 × 64`
píxeles. La representación interna ocupa 1024 bytes y usa páginas verticales de
ocho píxeles:

```text
byte_index = (y >> 3) * 128 + x
bit_mask   = 1 << (y & 7)
```

La evidencia proviene de dos rutinas desensambladas del binario oficial:

| Dirección 1.2.1 | Comportamiento observado |
|---:|---|
| `0x0802C594` | valida `x <= 127`, `y <= 63` y pone/limpia un píxel |
| `0x0802C690` | valida `x <= 127`, `y <= 63` y lee un píxel |

Esto confirma que una gráfica de envelope completa es viable físicamente y
también desde un firmware propio. Las direcciones son internas, específicas de
esta compilación y no forman una API reutilizable.

### Objeto gráfico interno

El constructor de `0x0802D21C` permite reconstruir el objeto global del display
con bastante precisión:

| Dirección/offset | Contenido |
|---:|---|
| Objeto global | `0x200084F4` |
| `objeto + 0x20` | comienzo del framebuffer (`0x20008514`) |
| `objeto + 0x420` | objeto bitmap embebido |
| `objeto + 0x424` | puntero del bitmap al framebuffer |
| `objeto + 0x428` | contexto de dibujo embebido |
| `objeto + 0x444` | ancho `128` |
| `objeto + 0x446` | alto `64` |

El framebuffer termina en `0x20008913`, exactamente antes del objeto bitmap:
`0x420 - 0x20 = 0x400 = 1024` bytes. La vtable del bitmap está en
`0x08034888` y contiene las primitivas de lectura, escritura y limpieza
identificadas arriba.

### Presentación del framebuffer

La ruta completa del framebuffer al LCD también quedó localizada:

| Dirección 1.2.1 | Rol confirmado |
|---:|---|
| `0x0802C2A4` | método `present` del objeto de display; inicia la ruta de presentación |
| `0x08009718` | adaptador que obtiene el puntero al framebuffer |
| `0x080095B8` | rutina de `flush` de 1024 bytes |
| `0x08009598` | envío de comandos al controlador |
| `0x080094CC` | transferencia SPI con D/C en modo datos |
| `0x08009516` | transferencia SPI con D/C en modo comando |
| `0x08009624` | reset e inicialización del controlador |

ABI inferida y validada por todos los callers:

```c
bool lcd_flush(void *lcd_bus, const uint8_t framebuffer[1024]);
```

En el firmware 1.2.1, `lcd_bus` es el objeto global `0x20002130`. La rutina
itera ocho páginas. Para cada página `p`:

```text
command: B0 + p
command: 10 04
data:    framebuffer + p * 128, 128 bytes
```

Por tanto, cada `flush` hace 8 transferencias de 128 bytes, precedidas por tres
bytes de direccionamiento por página. El `10 04` fija la columna inicial en 4
(nibble alto `0`, nibble bajo `4`);
es un desplazamiento del panel/controlador, no cuatro píxeles ausentes del
framebuffer lógico.

El método `present` no usa una segunda copia ni un compositor oculto. La cadena
es directa:

```text
display + 0x20 -> adaptador global -> lcd_flush -> SPI1 -> LCD
```

El adaptador global se enlaza al framebuffer durante el arranque en
`0x080172C4`.

### Controlador y señales

La inicialización observada es:

```text
reset GPIO
E2 A2 A1 C0 24 81 28 2F
clear/flush
A6 AF 40
```

Estos comandos, junto con el direccionamiento por páginas `B0..B7`, identifican
con confianza alta un controlador **compatible con la familia ST7565**. El
modelo exacto de silicio sigue sin estar demostrado sin leer el encapsulado o
la referencia del módulo.

La ruta de transferencia usa `SPI1` (`0x40013000`) mediante el handle global
`0x20001818`. Los descriptores GPIO iniciales muestran las siguientes señales
en el mismo puerto `0x50000400`:

| Señal | Máscara | Pin inferido |
|---|---:|---:|
| D/C | `0x0010` | 4 |
| RESET | `0x0040` | 6 |
| CS | `0x0080` | 7 |

La identificación del nombre del puerto queda pendiente de cruzarla con el
encapsulado exacto del N32G455; las máscaras y direcciones sí están confirmadas
en el binario.

La interfaz MIDI/SysEx conocida no permite pasar ese framebuffer. El enum
auto-generado por Arturia para sus integraciones sólo expone texto, iconos
predefinidos y widgets (`knob`, `fader`, `pad`, partes, marcos y feedback
automático). La configuración USB de ejecución tampoco contiene un endpoint
gráfico dedicado: presenta MIDI Streaming y DFU runtime.

El parser MIDI propietario también quedó localizado:

| Dirección 1.2.1 | Rol observado |
|---:|---|
| `0x0800C4B8` | constructor del parser y sus callbacks |
| `0x0800C526` | reinicio de estado y limpieza del buffer |
| `0x0800C614` | máquina de estados que reconoce `00 20 6B ... 42` |
| `0x0800C5A8` | cierre del mensaje e invocación del callback |

El buffer de payload comienza en `parser + 0x15` y su capacidad es 100 bytes.
La función de cierre invoca el callback guardado en `parser + 0xBC`. No se
encontró en esa ruta un handler que copie fragmentos al framebuffer ni un
comando de presentación de bitmap. Además, un frame de 1024 bytes nunca podría
llegar como un único mensaje en esta implementación: una extensión tendría que
fragmentarlo y validar offset, longitud y checksum.

Por ahora deben mantenerse separadas estas dos capacidades:

1. **Renderer interno por píxel:** confirmado.
2. **Carga de imágenes desde el host con firmware oficial:** no encontrada.

Dos intentos físicos del 29 de julio de 2026 descartaron la interpretación de
`eWhiteScreen` como relleno de píxeles. El primero carecía del byte `00` que
cierra el payload y fue ignorado. El segundo usó la serialización exacta
`04 01 60 61 00`, coincidente con la integración FL Studio: abandonó HOME,
mostró la plantilla DAW y luego RackForge recuperó HOME, sin encender la superficie
completa.

El handler de `0x080234E4` remapea los modos públicos `0x61 -> 0x59` y
`0x62 -> 0x5A` antes de delegarlos. Son órdenes lógicas de pantalla, no
equivalentes a llenar el framebuffer con `0xFF`. Estas pruebas no verifican una
ruta de píxeles desde el host.

La vía mínima para habilitarlo en firmware modificado sería un comando
versionado que escriba fragmentos acotados de los 1024 bytes, con longitud,
offset y checksum, seguido por un `present`. Ya están identificadas la rutina
de `flush`, su ABI y la familia del controlador; falta localizar el dispatcher
SysEx más seguro donde agregar ese comando sin interferir con el protocolo
oficial.

### Candidata offline del hook gráfico

El dispatcher que consume mensajes MIDI ya parseados comienza en
`0x0802C31C`. Recibe una estructura cuyo largo está en el offset `+7` y cuyo
payload SysEx comienza en `+13`. Esto permite interceptar únicamente la firma
RackForge antes de delegar todos los demás mensajes al código original.

La candidata reproducible de `firmware/patch/`:

- reemplaza los cuatro bytes iniciales `F0 B5 0D 00` por un `B.W`;
- ubica el código añadido en `0x08036B80`, después del payload oficial;
- reproduce las dos instrucciones desplazadas antes de volver a
  `0x0802C320`;
- escribe fragmentos de como máximo 40 bytes dentro de
  `0x20008514..0x20008913`;
- exige checksum de 7 bits por fragmento;
- exige CRC-16/CCITT-FALSE de los 1024 bytes antes de llamar a
  `0x080095B8`;
- no contiene operaciones de flash, DFU, option bytes ni reset.

El primer build ocupa 378 bytes y no contiene relocaciones pendientes. Esas
validaciones offline no demostraban todavía que el actualizador aceptara el
payload ni que la aplicación parcheada arrancara.

### Resultado físico de la primera candidata

La primera instalación del hook terminó con error en MIDI Control Center y el
teclado permaneció en bootloader. La restauración inmediata del paquete oficial
1.2.1 funcionó y devolvió todas las interfaces MIDI normales.

La cabecera modificada conservaba:

- VID/PID y base de aplicación correctos;
- longitud coherente con el archivo;
- suma aditiva de los 64 bytes igual a cero;
- código dentro del slot y branch correctamente resuelto.

El análisis posterior demostró que eso era insuficiente: la candidata no
actualizaba el checksum aditivo del payload almacenado en `0x0A`. Por tanto, no
sirve para evaluar si el hook arrancaba ni para atribuirle un fallo temprano.

El build queda rechazado y no se repetirá. Antes de otra prueba física se debe
obtener el código de error exacto del actualizador o reproducir la validación
offline, y reducir el siguiente experimento a un parche inerte que no enganche
el dispatcher.

### Resultado del diagnóstico inerte de igual longitud

El segundo experimento conservó byte por byte la cabecera oficial y la
longitud total de 193.412 bytes. Su único diff son 16 bytes dentro del hueco
borrado `0x08035C0C..0x08035D97`: en `0x08035CC0` reemplaza `FF` por una
marca inerte de 16 bytes bajo la identidad anterior del proyecto.

No modifica vectores, instrucciones, dispatcher ni flujo de ejecución. Sus 12
pruebas automatizadas verificaron además que el contenedor solo reemplazara la
imagen del modelo de 61 teclas.

La transferencia terminó, pero la aplicación no enumeró. MIDI Control Center
mostró `Failed to open the device`, Windows mantuvo únicamente la interfaz de
bootloader y no había otro DAW o bridge usando MIDI. La tercera restauración
del paquete oficial devolvió inmediatamente las interfaces base y MEDIA.

Esta candidata también queda rechazada. El análisis de tres firmwares oficiales
reprodujo después la validación que faltaba: el payload modificado sumaba
`0x32`, por lo que `header[0x0A]` debía cambiar de `0xD9` a `0xCE`. La cabecera
idéntica conservó el valor obsoleto y explica el rechazo sin involucrar el flujo
de ejecución.

### Resultado de la sonda con checksums corregidos

Una tercera prueba cambió otra marca de 16 bytes en el mismo hueco y actualizó
únicamente `header[0x0A]` y `header[0x3F]`. El diff total fue de 18 bytes, sin
modificar vectores, instrucciones ni flujo de control.

MIDI Control Center terminó la instalación correctamente, la aplicación
arrancó y Windows recuperó las interfaces `arturiausbmidi_sc` y `MEDIA`. Esto
confirma experimentalmente:

1. la regla aditiva del checksum del payload;
2. que no hay una firma criptográfica bloqueando esta modificación acotada;
3. que el hueco `0x08035C0C..0x08035D97` puede contener datos;
4. que los dos intentos fallidos anteriores no evaluaron realmente el hook.

### Resultado del hook con checksums corregidos

La candidata del hook con ambos checksums válidos se instaló y arrancó
correctamente. Sin embargo:

- 27 mensajes de framebuffer válidos, incluido el CRC final, no produjeron
  ningún cambio de píxeles;
- los comandos oficiales de pantalla siguieron funcionando;
- el eco oficial `DAW Program`, usado como ACK por el bridge, dejó de llegar.

La revisión del desensamblado mostró que `0x0802C31C` procesa una estructura
interna de UI: usa datos desde `r1 + 0x0E`, mientras la primera implementación
había inferido `r1 + 0x0D`. No es el dispatcher SysEx general. Esta candidata
queda descartada y debe restaurarse el firmware oficial antes de localizar una
ruta de intercepción correcta.

### Resultado del hook en el callback SysEx crudo

La candidata posterior interceptó `0x0800B802`, actualizó ambos checksums y
arrancó con las cuatro interfaces ALSA visibles. Sin embargo, dejó de llegar
tanto el ACK oficial `DAW Program` como cualquier nota MIDI tocada desde el
teclado. Por lo tanto, el wrapper altera la ruta MIDI primaria aunque el
dispositivo enumere correctamente.

El firmware oficial 1.2.1 restauró la operación normal. La imagen
`00ce83922290bf11b69d872b5aa4e54a175dcd19f5772e86f40810a920446e29`
queda archivada para análisis, sin extensión instalable y no debe repetirse.
El desarrollo continúa sobre el protocolo permitido por el firmware oficial.

## Presupuesto para Doom

El WAD shareware disponible mide 4.196.020 bytes (~4,00 MiB), unas 16 veces la
flash interna completa. Un build directo de Chocolate Doom necesita
aproximadamente 300 KiB de datos mutables estáticos y 700 KiB de zone memory.
Incluso el port muy optimizado para RP2040 aprovecha 264 KiB de RAM y flash QSPI
externa.

Por tanto, el Doom original no cabe tal cual. Las rutas técnicamente razonables
son:

1. Confirmar y usar/agregar flash QSPI y, probablemente, PSRAM externa.
2. Transmitir recursos por USB desde la PC y reducir agresivamente el motor.
3. Crear una reimplementación extremadamente pequeña compatible con los mapas,
   que ya no sería el ejecutable original de Doom.

La primera ruta es la más cercana a un port nativo auténtico, pero depende del
encapsulado exacto, los pines expuestos y la topología real de la placa.

## Regla de seguridad

La restauración oficial 1.2.1 ya fue comprobada dos veces sin abrir el teclado.
Cada prueba física posterior debe ser mínima y distinguir una sola hipótesis:

1. conservar siempre el paquete oficial fijado por SHA-256;
2. validar offline cada byte modificado y cada límite;
3. no leer ni cambiar option bytes;
4. no repetir una candidata rechazada;
5. ante un fallo, registrar el texto exacto y restaurar inmediatamente el
   firmware oficial antes de continuar.

El inspector Rust `firmware_map` solamente lee un archivo local y reproduce este
mapa. No incluye código USB, MIDI ni DFU.

El N32G455 dispone oficialmente de SWD/JTAG y de bootloader USB/UART. Eso da dos
posibles vías de recuperación, pero no garantiza que estén accesibles en esta
placa ni que Arturia haya dejado el chip sin protección. Intentar bajar el nivel
de read protection puede borrar toda la flash; por eso no se leerán ni tocarán
option bytes hasta identificar pads, estado de protección y procedimiento de
rescate.
