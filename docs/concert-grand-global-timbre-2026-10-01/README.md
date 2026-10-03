# Brillo, cuerpo y carácter de campana: diagnóstico global

El usuario oye graves y medios cerrados y agudos con poco cuerpo, también
con Concert 308. Se conserva el candidato sin duplex seleccionado por oído:
renderer `3b6ac0541fcd740b2cdab1586158b2e66790a6d367438f7ac91228970bb1d2b7`.
No se incorpora ninguna variante de este diagnóstico al sonido de fábrica.

## Evidencia actual

Se comparan once raíces grabadas de C2 a C7, cada seis semitonos, a
velocidades 60 y 110: 22 pares. Son ataques sostenidos dos segundos, cuatro
segundos de render, 44,1 kHz, bloques de 256 cuadros y 200 ms de preparación.
El Steinway usa el mismo sampler SFZ histórico, con los archivos y capas
verificados por hash y sin transposición. No es una nueva actuación acústica.
Las métricas tienen ganancia fija; para exportar WAV se aplica, si hace falta,
una reducción común a todo el conjunto. No se igualan notas por separado.

Ventanas: 0–20, 50–200, 200–500 y 500–1500 ms desde el inicio detectado.
Se separan bandas y H1–H6, con centros espectrales estimados para cada audio.
Los filtros se usan para medir; no se aplican a las audiciones. La tabla
muestra energía integrada de H1 en la ventana, no una medida de fuerza o
una constante física identificada.

| Nota, v60 | H1 modelo menos Steinway, 50–200 ms | H1, 500–1500 ms | Error adicional de caída |
| --- | ---: | ---: | ---: |
| C4 | −3,78 dB | −21,28 dB | −17,50 dB |
| C5 | −6,39 dB | −3,75 dB | +2,64 dB |
| C6 | −4,13 dB | −9,68 dB | −5,55 dB |
| F#6 | +6,40 dB | +9,69 dB | +3,29 dB |

Un ajuste universal de nivel o brillo no resuelve estas diferencias opuestas.
Hay déficits en 600–4000 Hz en varios graves mientras ciertas notas fuertes
ya exceden la referencia por encima de 4 kHz. La pérdida de fundamental y
la distribución entre armónicos son compatibles con falta de cuerpo, sin
identificar por sí solas la causa perceptiva del carácter de campana.

## Variantes controladas

Todas conservan el duplex desactivado y el ancho de unísono cero. Se ensayan
por separado rigidez grave compensada desde C4, pérdida del contacto,
proximidad, amortiguamiento de tabla, integral espacial más densa, observador
causal de presión, resonancia simpática, pérdida adicional del unísono y
movilidad media del puente. Los cambios en movilidad conservan su uso tanto
en carga como en radiación; la compensación global de presión se registra.

- Endurecer sólo el contacto grave recupera poco brillo; no resuelve el cuerpo
  de medios y agudos. Quitar su pérdida de velocidad mejora la medida de brillo
  grave, con resultados distintos por nota y sin resolver las caídas de H1.
- Retirar proximidad cambia poco el equilibrio. Duplicar la cuadratura espacial
  cambia poco el resultado; no sustenta atribuir el defecto a esa discretización.
- El observador causal mejora H1 de C4/C5 y empeora fuertemente C6. Es un
  diagnóstico nativo, no una variante portable seleccionada.
- Retirar la resonancia simpática mejora claridad grave pero no recupera H1
  de C6; tampoco demuestra que falte sólo resonancia paralela.
- Sin pérdida adicional del unísono, el error absoluto medio de H1 tardío
  pasa de 5,48 a 7,23 dB; C4 y C6 empeoran. No se selecciona.
- Media movilidad del puente con presión global compensada mejora C4 y C6,
  pero F#6/v60 pasa de +9,69 a +17,29 dB de exceso tardío. No se selecciona.
- Aumentar la pérdida de tabla de 4,6 % a 7/9/12 % mejora parte del error de
  caída, con mayor oscuridad grave y sin resolver el equilibrio tonal global.
- Desactivar la carga reactiva no demuestra una corrección global del timbre.

La escucha de estas ablaciones queda disponible. Ninguna se declara una
solución global ni una identificación de materiales reales.

## Por qué el refit no garantizó el resultado percibido

El informe del cuerpo sostenido del 30 de septiembre ya registraba una
caída excesiva de H1 en C4. Aquel ajuste redujo el error compuesto de 7,294
a 5,511 y el peor par de 9,920 a 7,768; seguían siendo errores grandes.
El resultado fue un compromiso aceptado para escucha, no una igualdad con
el Steinway. Se conservaban promedio y peor caso: el problema fue aceptar
residuos grandes, no que todas las métricas los ocultaran.

Su configuración habitual ajustaba C4–C7 a 60/124, H1–H6 y tres ventanas.
No cubría el grave en ese ajuste específico. Hubo evaluaciones más amplias,
que también señalaban déficits. El modelo y la captación se modificaron
después; una mejora histórica no valida automáticamente el sonido actual.

Además, hay dos calibraciones antiguas que el camino físico sustituye:
`MIDRANGE_BALANCE` sólo se aplica sin puente físico, y
`PROMPT_MEASURED_POWER` modifica un T60 que la rama física reemplaza por
admitancia proyectada. Desactivar ambos controles a la vez produce **audio
idéntico bit por bit** en C4/C5/C6 a 60/110, cuatro segundos por par.
Esto no invalida los coeficientes de contacto y puente del ajuste físico:
son controles activos de una etapa diferente. Se aclaran sus comentarios.

El siguiente ajuste debe distinguir parámetros activos de legados, usar el
render final actual y establecer límites por registro y peor par para caída,
equilibrio armónico, ataque y modulación. Las métricas deben acompañarse de
audiciones de notas y piezas. Este diagnóstico no identifica un único cambio
de física que garantice resolver todos los casos.

## Archivos y reproducción

Audios e informes: `C:/SHARED/Brillo-graves-medios`.
`Cuerpo-agudos-actual-Steinway.wav` alterna, por altura, nuestro v60/v110 y
Steinway v60/v110, en C4/C5/F#5/C6/F#6/C7. `resumen.json` da los tiempos.
Las carpetas `diagnostico*` conservan las ablaciones completas.
`coeficientes-legacy.json` verifica la ausencia de influencia de los dos
controles antiguos. Los WAV y binarios están fuera de Git.

Herramienta: `tools/audit-concert-grand-lower-brilliance.py` (numpy, scipy,
soundfile; sampler SFZ local). Ejemplo:

```powershell
python tools/audit-concert-grand-lower-brilliance.py --binary local/concert-grand-duplex-fix/renderer.exe --work local/concert-grand-lower-brilliance/diagnostics --output C:/SHARED/Brillo-graves-medios/diagnostico-global --notes 36,42,48,54,60,66,72,78,84,90,96 --profiles local/concert-grand-lower-brilliance/global-profiles.json
```

Se guardan aquí los perfiles e informes para repetir el análisis. Las pruebas
de presión causal son exclusivamente de diagnóstico nativo. No hay commit,
paquete nuevo ni modificación de las instalaciones.
