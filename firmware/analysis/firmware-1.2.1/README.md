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

No se construirá un contenedor instalable ni se intentará DFU hasta contar con:

1. fotos legibles de ambas caras de la placa;
2. identificación de MCU, memorias y controlador de pantalla;
3. captura completa y validada del protocolo de actualización oficial;
4. método de recuperación probado;
5. checksum/firma de la cabecera comprendidos.

El inspector Rust `firmware_map` solamente lee un archivo local y reproduce este
mapa. No incluye código USB, MIDI ni DFU.

El N32G455 dispone oficialmente de SWD/JTAG y de bootloader USB/UART. Eso da dos
posibles vías de recuperación, pero no garantiza que estén accesibles en esta
placa ni que Arturia haya dejado el chip sin protección. Intentar bajar el nivel
de read protection puede borrar toda la flash; por eso no se leerán ni tocarán
option bytes hasta identificar pads, estado de protección y procedimiento de
rescate.
