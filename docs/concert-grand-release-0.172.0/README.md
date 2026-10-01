# RF-Concert Grand 0.172.0

Integración del modelo seleccionado el 1 de octubre de 2026. Los coeficientes ya están en el código portable; no dependen de archivos de ajuste en `local/`.

## Modelo seleccionado

Perfil completo: `plugins/concert-grand/calibration/factory-0.172.0.json`.

- Tabla y puente físicos, tres carriles de cuerda por unísono, afinación coincidente y polarización sin detune.
- Contacto de fieltro dependiente de velocidad y curva global de rigidez; transferencia reducida de graves con esquina de 400 Hz y transición continua hasta C4.
- Clack añadido y golpe de fondo de tecla desactivados; mecanismos de pedal y liberación conservados.
- Reiteración con llegada de 4 ms, retención efectiva de 0.7 del estado anterior y compensación para conservar el nuevo golpe completo.
- Dúplex con llegada finita independiente; no recibe la pérdida de contacto de la cuerda que golpea el martillo.
- Programa Concert: ancho 1, distancia del par 2.64 m, amortiguación de tabla 4.6%.
- Ganancia común de la audición, 25.120884443707872 dB, después del preamplificador y antes del techo de salida existente. No se añade EQ ni compresor.

Los dieciséis programas usan el nuevo modelo. Mantienen sus dimensiones, posición de micrófonos y voicing propios. Al cambiar la referencia de Concert de 0.66 a 2.64 m, los otros programas reciben una corrección común de nivel de −12.044 dB; Bright recibe −2 dB adicionales. La prueba de niveles entre Concert/Mellow/Bright cubre graves, medios y agudos y exige diferencias inferiores a 4 dB.

Las sesiones anteriores conservan sus controles guardados. La conversión del antiguo fader de amortiguación queda anclada a la calibración histórica de 2.3%, independiente del nuevo coeficiente de tabla. Para escuchar la configuración nueva después de instalar, cargar el programa Concert Grand.

## Compatibilidad portable

Se sustituyeron `powi` y `ceil` usados en investigación por operaciones disponibles en `no_std`. El observador FIR retardado permanece como diagnóstico nativo, desactivado en WASM; el observador físico seleccionado es el mismo en ambos.

El procesador nuevo excedía la pila WASM de 8 MiB al inicializarse. `plugins/concert-grand/build.rs` reserva 16 MiB para este componente, sin cambiar la reserva de los otros componentes del workspace. Las pruebas del runtime se ejecutaron contra el componente optimizado extraído del `.rfplugin` final.

## Verificación

- 115 pruebas nativas aprobadas; 75 diagnósticos ignorados.
- Huellas de audio y pedal actualizadas deliberadamente por la nueva física, captación y nivel, y verificadas: `835cf23d24021cb0`, `6042f3d24fb94a91`.
- Arranque del componente en el runtime de RackForge y equivalencia entre una instancia y cuatro trabajadores aprobados.
- Instalación del archivo final en un almacén de validación aislado aprobada; metadatos archivados iguales a los del código fuente.
- Configuración compilada contra el renderer congelado de investigación: **error máximo cero**, 176400 frames a 44.1 kHz, bloques de 256, con pedal, reiteraciones, velocidades y registros variados. Se compara la misma calibración de salida y el techo existente.
- Las pruebas anteriores de llegada finita y balance de graves se mantienen en los informes de investigación. Dos aserciones de audio de la antigua calibración Salamander se sustituyeron por controles del modelo seleccionado: conservación de tres cuerdas sin detune y pérdida de energía de las fundamentales. La tabla histórica de prompt sigue comprobándose.

El informe de fuel es diagnóstico sin gobernador: el modelo completo puede superar 200 M de instrucciones en una única llamada con polifonía alta y bloques largos. No se afirma rendimiento uniforme para cualquier dispositivo, tamaño de bloque o política de calidad. El test paralelo usa los límites reales del runtime. Este paquete no constituye una identificación completa del Steinway ni resuelve totalmente la interacción del martillo con cuerdas ya en movimiento.

## Construcción

```powershell
cargo +1.98.0-x86_64-pc-windows-msvc build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
cargo +1.98.0-x86_64-pc-windows-msvc run --release -p rackforge-store -- pack-wasm plugins/concert-grand/package target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin
```

`tools/promote-concert-grand-profile.py` actualiza los coeficientes y metadatos sobre el código portable canónico. `tools/verify-concert-grand-promoted-profile.py` verifica la promoción contra un renderer congelado. No se incluyen grabaciones ni binarios de investigación en Git.
