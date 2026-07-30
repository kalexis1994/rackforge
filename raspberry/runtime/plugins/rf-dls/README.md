# RF-DLS

RF-DLS es el plugin de instrumento DLS nativo de RackForge. El binario no
incluye sonidos: recibe un archivo DLS aportado por el usuario mediante el
recurso obligatorio `dls-bank`.

## Contrato inicial

- ID estable: `org.rackforge.rf-dls`
- Plugin API: 1.3
- Tipo: instrumento
- Polifonía máxima: 32 voces
- Salida: estéreo; sin FX conserva la señal mono original en ambos canales
- Banco inicial: General MIDI bank 0, program 0
- Preset inicial: `gm.piano-1`
- Estado v2: volumen e ID estable del programa seleccionado; sigue leyendo el
  estado v1 basado en banco/programa
- Catálogo dinámico separado en `DLS` (solo lectura) y `CUSTOM`

## MIDI implementado

- Note On
- Note Off
- Note On con velocidad cero como Note Off
- Pitch Bend de 14 bits, inicialmente con rango de ±2 semitonos
- CC1 modulation wheel aplicado a las conexiones LFO del instrumento DLS
- CC64 sustain, con liberación diferida hasta levantar el pedal
- CC121 Reset All Controllers
- CC120 All Sound Off, que corta y limpia inmediatamente todas las voces
- CC123 All Notes Off, que respeta el estado del pedal

El procesamiento acepta eventos posicionados dentro del bloque. El camino de
audio no abre archivos, no escribe logs, no toma locks y no asigna memoria al
crear voces.

## Recurso externo

Ejemplo de prueba en ARM64:

```bash
rackforge-core smoke plugins/rf-dls/package \
  --library target/release/librackforge_rf_dls.so \
  --resource dls-bank=/home/kalex/rackforge/data/plugins/rf-dls/banks/gm.dls \
  --preset gm.piano-1 \
  --data-root /home/kalex/rackforge/data
```

El `.dls` no debe copiarse al repositorio ni al futuro paquete `.rfplugin`.

## PLAY dinámico

Al crear la instancia, RF-DLS ordena los instrumentos por banco y programa,
elimina duplicados y publica un catálogo mediante Host API 1.4. Los IDs tienen
esta forma opaca:

```text
dls.b00000000.p00000030
```

RackForge usa el ID para seleccionar el sonido, pero la interfaz muestra el
nombre y un detalle corto como `B000 P048` o `DRUM P000`. El bridge del KeyLab
recibe además el banco lógico y presenta primero `DLS` o `CUSTOM`; no conoce la
estructura interna del plugin.

## Programas CUSTOM

Los instrumentos descubiertos dentro del DLS son inmutables. Un CUSTOM no
reescribe el banco: guarda una referencia a `dls-bank` + banco + programa y
únicamente sus overrides. RF-DLS busca documentos con sufijo
`.rackforge-program.json` en:

```text
data/plugins/org.rackforge.rf-dls/custom/
```

El payload v4 admite una capa `A` obligatoria y una capa `B` opcional. `A`
siempre está habilitada; `B` puede desactivarse conservando toda su
configuración. Cada capa referencia directamente un instrumento del DLS y
define rangos de tecla y velocidad. Sus overrides
incluyen nivel, transposición, afinación fina, rango de pitch bend, profundidad
de modulación, envolventes de amplitud y pitch, y LFO con rate, delay y
profundidades. Un valor opcional ausente significa `INHERIT`: el motor conserva
el comportamiento original codificado en el instrumento DLS.

Después del mix de capas, el programa dispone de una cadena FX compartida. El
primer nodo es un chorus estéreo con enable, rate, depth, delay, feedback,
width y mix. Sus cambios se preescuchan mientras se mueve el control y se
suavizan dentro del DSP para evitar saltos.

El segundo nodo es una reverb ROOM estéreo con enable, size, decay, pre-delay,
damping, width y mix. Usa una red de ocho delays realimentados y reserva todos
sus buffers al activar el plugin; el hilo de audio no asigna memoria ni realiza
I/O.

Los documentos v1 existentes se migran en memoria a una única capa `A`; los v2
conservan sus dos capas. Las versiones v1 y v2 reciben ambos FX desactivados, y
la v3 recibe la nueva reverb desactivada, sin reescribir el archivo. La
siguiente vez que el usuario los guarda se serializan como v4. IDs y slots
duplicados, symlinks, archivos
mayores a 256 KiB, payloads desconocidos o valores fuera de rango se ignoran
individualmente y se registran como advertencia; no impiden arrancar el resto
del banco.

El ejemplo versionado
`examples/custom.warm-piano.rackforge-program.json` se puede instalar mediante
el escritor atómico común:

```bash
rackforge-core program-save /home/kalex/rackforge/data \
  custom/custom.warm-piano.rackforge-program.json \
  examples/custom.warm-piano.rackforge-program.json
```

El ID de catálogo resultante es `custom.user.warm-piano`. Cambiar o agregar
archivos requiere reiniciar el motor RF-DLS para reconstruir el catálogo.

Durante una edición, Core entrega al plugin el documento completo mediante la
extensión de programas. RF-DLS preescucha ese borrador de forma transitoria:
las dos capas, sus overrides, el chorus y la reverb compartidos ya se oyen antes de
guardar, pero el catálogo y los archivos sólo cambian después de confirmar
`SAVE`.

## Límites de esta etapa

- Aún no responde a Bank Select ni Program Change MIDI.
- Todos los canales MIDI controlan la misma instancia.
- Los FX insertables y el paneo por capa todavía no están implementados.
