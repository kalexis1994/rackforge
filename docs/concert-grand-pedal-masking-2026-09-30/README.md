# Trino con pedal y captación estéreo

## Observación

El usuario señaló que Nocturno, entre 201 y 203 segundos, pierde la articulación y se convierte en un colchón. En el MIDI, CC64 permanece en 127 y hay veinte ataques, principalmente alternancias de La#4 y Si4 cada 60–190 ms. Las velocidades no son uniformes. Después de 203 s hay ataques de velocidad 3–4, que no deben amplificarse artificialmente para igualarlos a los anteriores.

Los renders de diagnóstico parten del frame cero, no del inicio del recorte. Conservan notas previas, pedal, velocidades, block de 256 frames, 44.1 kHz y una ganancia común de 25.120884443707872 dB. No hay normalización por nota ni compresor.

## Ablaciones

`C:/SHARED/Nocturno-ataques-diagnostico` contiene el modelo aceptado, contacto de 1 ms, reiteración con voz nueva y cola global más corta. Acortar solamente la llegada del golpe tiene poco efecto. Cambiar la reiteración o las colas reduce el fondo. Reiniciar voces es una herramienta de diagnóstico, no una propuesta física.

El contacto actual se simula sobre cuerdas inicialmente quietas. Después se superpone su resultado modal con la cuerda que está sonando. No se resuelve el contacto del martillo contra el desplazamiento y velocidad reales de esa cuerda. La vibración vieja puede acumularse y también interferir con el golpe nuevo. El sampler de referencia tampoco identifica esa colisión: es una comparación audible, no evidencia directa del mecanismo real.

## Experimento de pérdida durante la reiteración

`RESTRIKE_OLD_RETENTION`, entre 0.5 y 1, es exclusivamente una constante efectiva de investigación. No es un coeficiente medido del fieltro ni un nuevo refit. Su valor por defecto es 1: el modelo aceptado se conserva. La interfaz y el plugin instalado no se modificaron.

Durante el contacto finito de una tecla repetida:

- `S(t)` es la envolvente smoothstep ya usada por el modelo.
- La ganancia de la vibración previa es `g(t) = 1 - (1-r) S(t)`.
- Se aplica `g_after / g_before` al estado anterior.
- El incremento nuevo se pondera por `delta_S * g_after / r`.

En un oscilador lineal, el estado final es exactamente `r * old_free + complete_new_free`. La compensación impide que la amortiguación de la vibración antigua reduzca también el golpe nuevo. Las fuentes mecánicas y el segmento dúplex, que el martillo no golpea, mantienen su llegada original y no reciben esta pérdida. Tampoco se amortiguan otras teclas ni se cambia el pedal.

La primera ablación aplicaba pérdida al dúplex. Aumentó 3.37 dB la energía de 8–18 kHz del primer milisegundo del ataque antes investigado en La Campanella (97.40755 s). Se descartó esa ruta; su registro queda en `local/concert-grand-restrike-contact/duplex-loss-ablation`. La verificación final registra esa métrica nuevamente.

Es una aproximación reducida, no un contacto no lineal completo. No garantiza pasividad de todo el sistema acoplado ni mejora perceptiva en cada contexto. La interferencia de fase puede producir resultados no monótonos al bajar r; no se adopta automáticamente la variante más amortiguada.

## Evidencia y reproducción

`C:/SHARED/Nocturno-contacto-reiteracion/validacion.json` registra:

- igualdad de todas las muestras del control desactivado con los primeros 207 s del modelo aceptado;
- igualdad exacta de seis ataques aislados con la pérdida activa;
- contraste energético previo/posterior en tres bandas para cada ataque del trino;
- comprobación del ataque anteriormente señalado en La Campanella;
- hashes del binario y de los eventos MIDI.

La suite nativa tiene 115 tests aprobados y 75 diagnósticos ignorados. El nuevo test comprueba el estado final de cuatro osciladores entre 261 y 16400 Hz, con distintas pérdidas y con/sin vibración previa. No confundir contraste energético con evaluación auditiva de fidelidad.

Resultado final con r=0.7: el nivel previo mediano baja 3.19 dB (100–600 Hz), 3.11 dB (600–1800 Hz) y 1.00 dB (1800–8000 Hz). El contraste mediano mejora de 0.343/0.447/0.432 dB a 0.423/0.569/0.540 dB: mejora pequeña, no resolución completa. r=0.5 reduce más el fondo pero empeora el contraste; no se propone como solución. El control de La Campanella cambia +0.547 dB en el primer milisegundo de 8–18 kHz, dentro del límite de regresión de +3 dB. Este control energético no demuestra por sí mismo ausencia perceptiva de clic.

Herramientas: `tools/audit-concert-grand-attack-sources.py`, `tools/verify-concert-grand-restrike-contact.py`. Binario congelado y configuración: `local/concert-grand-restrike-contact`.

## Espacialidad

La captación actual de la tabla usa `MIC_SPACING_M * width`: con 17 cm y width 0.35, los receptores quedan a 5.95 cm. Además, la directividad de la tabla apunta hacia abajo en ambos canales; la ruta auxiliar/sala sí usa ejes abiertos ±55°. Hay una diferencia de geometría entre rutas que el parámetro de ancho no resuelve completamente.

En La Campanella 36–44 s, la correlación de canales baja de 0.886 a 0.816 al abrir width de 0.35 a 1; la referencia Stage ORTF tiene 0.513. La relación lateral/central pasa de −11.85 a −9.57 dB; referencia −4.83 dB. Abrir el control ayuda parcialmente. Los archivos tienen margen digital, por lo que esa concentración no demuestra clipping ni saturación del dispositivo.

Pruebas en `C:/SHARED/Espacialidad-graves-con-ataque`; control y aperturas 0.7/1, misma ganancia, versiones mono y hashes. Se debe evaluar la geometría y la contribución de sala antes de atribuir toda la diferencia a ancho estéreo. ORTF convencional: [DPA, 17 cm y 110°](https://www.dpamicrophones.com/dict/ortf/).
