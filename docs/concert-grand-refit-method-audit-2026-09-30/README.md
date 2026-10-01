# Auditoría del método de refit — 30 de septiembre de 2026

La escucha del usuario indica que sigue existiendo una diferencia importante
frente al Steinway reproducido por muestras. No hay una nueva mejora auditiva
aceptada ni evidencia suficiente para declarar un límite del modelado físico.

## Hallazgos comprobados

1. El objetivo de `tools/fit-concert-grand-sustained-body.py` sirve para estudiar
   el cuerpo sostenido: usa las primeras tres ventanas de
   `tools/audit-concert-grand-timbre-history.py`, de 50–200, 200–500 y 500–1500 ms.
   No mide los primeros 50 ms. Pondera caída relativa de H1–H6 (50 %), balance
   de esos parciales (35 %) y caída relativa de energía total (15 %).
2. Su puntuación es invariante al nivel de una nota. La prueba en
   `objective-probe.json` multiplica por 100 las potencias de las ventanas de
   C4/velocidad 60, equivalente a +20 dB de amplitud. Los cocientes derivados
   siguen siendo iguales y el error permanece exactamente en 8,712407358.
   Es una prueba sobre características medidas; no se alteró ni publicó audio.
   La ganancia común calculada después del ajuste no se incorpora como un error
   residual de nivel entre todos los pares nota/velocidad. Esta invariancia es
   útil para aislar decaimientos, pero deja incompleto un objetivo de timbre y
   respuesta dinámica de todo el piano.
3. En `local/concert-grand-coherent-unison/src/lib.rs`, tras calcular el contacto,
   cada golpe se ajusta al pico de la receta analítica (`peak / sim_peak`). Hay
   una normalización modal posterior y una curva de sonoridad MIDI. La energía
   mecánica calculada no determina por sí sola la presión del audio exportado.
   No se ha probado que este mecanismo sea la causa única del timbre percibido.
4. El 14B conserva el ajuste del 14 y añade reconstrucción de fase y entrada
   finita para nuevos ataques sobre cuerdas vivas. No es un refit integral nuevo;
   las notas aisladas verificadas permanecen idénticas al 14.
5. El ajuste 14 ya registra diferencias residuales importantes: por ejemplo,
   caída de H1 fuerte en C4 de -23,45 dB frente a -8,17 dB en la referencia stage,
   entre 0,5–1,5 s respecto a 0,05–0,2 s. Esa medida no identifica de forma única
   materiales, fuerza, micrófono, sala o instante de liberación.

## Siguiente experimento recomendado

Construir primero una evaluación integral y congelar el 14B como referencia:

- Espectros con varias resoluciones temporales para 0–50, 50–200, 200–500 ms,
  cuerpo sostenido, liberación y cola larga; conservar ambos canales.
- Error de nivel por nota/velocidad después de una sola ganancia común al
  conjunto. Medir la curva de intensidades y el balance entre registros.
- Separar bandas tonales, energía bajo el fundamental y ruido/transitorio;
  contar movimiento de envolvente sin premiar la desaparición de energía.
- Usar notas exactas grabadas, varias capas originales por raíz y condiciones
  de micrófono separadas. Reservar notas/intensidades para comprobación;
  reproducir piezas completas después. Los WAV editados de la biblioteca no
  establecen presión acústica absoluta ni velocidades reales del martillo.
- Registrar excitación, pérdidas y transferencia de salida para determinar
  dónde aparece el déficit. Comparar la cadena actual con una ruta de energía
  mecánica calibrada conjuntamente; evitar simplemente retirar la normalización
  y dar por físicamente correctas las unidades o la sonoridad resultantes.
- Ajustar conjuntamente los parámetros del subsistema que explique el error;
  declarar una mejora sólo cuando también pase la escucha, comparada con el
  Steinway y con el candidato congelado.

La posibilidad de mejorar con más coeficientes y la necesidad de cambiar la
estructura siguen abiertas. El primer resultado útil es distinguirlas mediante
esta evaluación, antes de generar otro candidato de piano completo.

## Alcance de esta revisión

Se revisaron fuentes y registros existentes y se probó la función de puntuación.
No se modificó el modelo, el plugin instalado ni sus programas. Copia de consulta
en `C:/SHARED/Diagnostico-refit/`.
