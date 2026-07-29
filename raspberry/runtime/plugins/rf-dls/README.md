# RF-DLS

RF-DLS es el addon de instrumento DLS nativo de RackForge. El binario no
incluye sonidos: recibe un archivo DLS aportado por el usuario mediante el
recurso obligatorio `dls-bank`.

## Contrato inicial

- ID estable: `org.rackforge.rf-dls`
- Plugin API: 1.3
- Tipo: instrumento
- Polifonía máxima: 32 voces
- Salida: mono duplicada en todos los canales ofrecidos por el host
- Banco inicial: General MIDI bank 0, program 0
- Preset inicial: `gm.piano-1`
- Estado persistente: volumen, banco y programa seleccionados
- Catálogo dinámico: todos los instrumentos únicos descubiertos en el DLS

## MIDI implementado

- Note On
- Note Off
- Note On con velocidad cero como Note Off
- CC64 sustain, con liberación diferida hasta levantar el pedal
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
  --resource dls-bank=/home/kalex/rackforge/data/addons/rf-dls/banks/gm.dls \
  --preset gm.piano-1 \
  --data-root /home/kalex/rackforge/data
```

El `.dls` no debe copiarse al repositorio ni al futuro paquete `.rfaddon`.

## PLAY dinámico

Al crear la instancia, RF-DLS ordena los instrumentos por banco y programa,
elimina duplicados y publica un catálogo mediante Host API 1.3. Los IDs tienen
esta forma opaca:

```text
dls.b00000000.p00000030
```

RackForge usa el ID para seleccionar el sonido, pero la interfaz muestra el
nombre y un detalle corto como `B000 P048` o `DRUM P000`. El bridge del KeyLab
no conoce la estructura DLS.

## Límites de esta etapa

- Aún no responde a Bank Select ni Program Change MIDI.
- Todos los canales MIDI controlan la misma instancia.
- La selección dinámica de instrumentos, programas RackForge, controles de
  envelope y FX se agregará sobre este contrato sin modificar su identidad.
