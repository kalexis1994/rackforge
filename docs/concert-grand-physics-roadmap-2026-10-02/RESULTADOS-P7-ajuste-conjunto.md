# P7 — Ajuste conjunto de movilidad y presión

2026-10-12. La etapa anterior quedó guardada en **6b5cb0a8**. Esta prueba es
un diagnóstico del cuerpo cargado de 161 modos, fuera del plugin. La
referencia sigue siendo el maestro físico de P6, con sus límites anteriores;
estos errores no son medidas de similitud con el Steinway.

## Hipótesis y control

El ajuste anterior optimizaba movilidad y luego identificaba el observador.
El banco de 161 modos dio 4,0738 % RMS en movilidad y 13,7633 % en presión.
Ahora la dinámica recibe ambos errores desde la misma evaluación:

```
Q(s) = [K + s C + s² I]⁻¹ B
Y(s) = s Bᵀ Q(s)
P(s) = s² O(s) Q(s)
O(s) = Σ_g exp(-s τ_g) O_g, O_g real
```

Se mantienen 161 modos, masas unitarias, B, mapa espacial de 88→20 canales
y cuatro grupos de retardos por cada uno de los dos micrófonos. El observador
real del control permanece fijo durante el ajuste dinámico. Sus retardos
siguen dentro del kernel original, 1,914–3,469 ms; la presión no realimenta
los puertos mecánicos y mantiene la escala de Pa del maestro.

K y C usan la misma familia positiva de la etapa anterior. La rigidez se
modifica en el complemento ortogonal a Ω⁻¹ B, conservando flexibilidad
estática; masas/B mantienen la inercia a alta frecuencia. La pérdida es una
congruencia positiva del control más L Lᵀ. No se reparan polos ni se reajusta
fieltro, teclas o ganancias por nota.

Los dos objetivos tienen el mismo peso, con cuadratura de frecuencia y
norma relativa del operador correspondiente. Ese escalado sólo define el
error de identificación; no normaliza audio o notas. Se añaden derivadas de
presión a las de movilidad usando el solve adjunto con dos columnas de
micrófono. El contraste con diferencias finitas pasa.

## Preparación y precisión

- 226 frecuencias de entrenamiento: sondas geométricas y vecindades de picos
  de movilidad **y presión**, dentro de las 1189 del maestro.
- Validación: las mismas 111 frecuencias excluidas, frente a fuerzas exactas
  en 11 puntos. Es validación usada en el desarrollo, no un test estadístico
  nuevo e independiente de toda selección anterior.
- Los solves se escalan con Ω⁻¹ para reducir el rango entre frecuencias
  audibles y terminaciones de varios MHz. El contraste con solves sin escalar
  pasa, también en el ejemplo con terminación de MHz.
- La rigidez ajustada se diagonaliza separada de la terminación rígida
  intacta. Error de transformación: movilidad **2,51e−12**, presión **3,79e−12**.
- Se comparan un hilo y seis BLAS sobre el mismo objetivo/gradiente inicial.
  Medianas 0,8210/1,2767 s por evaluación; se elige un hilo. Esta comparación
  es de preparación, no de callback ni una aceleración del render completo.
- Se guarda un único checkpoint de parámetros/basis que se actualiza durante
  el ajuste; el modelo final conserva K/C/B antes del cambio modal. No hay
  nuevas R, historias de desplazamiento ni WAVs.

El ajuste termina en **224,97 s**, 400 iteraciones. Alcanzó el límite de
iteraciones; no se afirma convergencia. El informe es
[conjunto-161.json](medidas-P7-cuerpo-cargado-2026-10-12/conjunto-161.json).

## Resultados reservados

| Variante | Movilidad RMS / máximo | Presión RMS / máximo |
| --- | ---: | ---: |
| Control de 161 modos | 4,0738 % / 13,0557 % | 13,7633 % / 28,6114 % |
| Dinámica conjunta, observador fijo | 4,7021 % / 14,8962 % | 11,1979 % / 27,0476 % |
| Misma dinámica conjunta, observador recalculado | 4,7021 % / 14,8962 % | **9,9881 % / 24,0357 %** |

El último control recalcula únicamente los coeficientes reales del observador
con la dinámica ya congelada. Conserva los mismos retardos, ganancia física,
eventos de validación y matriz mecánica. No normaliza cada salida.
[presion-conjunto-161-refit.json](medidas-P7-cuerpo-cargado-2026-10-12/presion-conjunto-161-refit.json).

La presión mejora un 27,4 % relativo en RMS frente al control inicial, pero
**ninguna variante pasa todos los gates**: RMS ≤5 %, máximo ≤10 %. La
movilidad empeora algo al incorporar la presión; el RMS pasa pero el máximo
no. Se conservan estas regresiones. No se promueve un score combinado como
sustituto de las dos validaciones independientes.

La flexibilidad estática final difiere 3,51e−9 del maestro, la inercia 1,28e−15
y la pérdida mínima es positiva, 7,2876. El verificador reconstruye K/C desde
el checkpoint, comprueba el complemento de almacenamiento y vuelve a calcular
las respuestas con solves **sin escalar**, independientes del solver ajustador.
Reproduce movilidad y los dos observadores, y conserva los gates fallidos.
[verificacion-conjunto-161.json](medidas-P7-cuerpo-cargado-2026-10-12/verificacion-conjunto-161.json).

## Ejecución nativa y variación de costo

Los 161 modos mecánicos se realizan con 176 resonadores del núcleo recíproco,
incluyendo polos reales sobreamortiguados. La realización coincide con la
respuesta continua a 1,43e−11. Los dos controles Rust/Python de estado final,
checksum/trabajo pasan; la energía por paso se comprueba en la trayectoria
Python reconstruida. No se verificó presión muestreada en el callback.

| Corrida | Tasa / 256 frames | Media | P99 | Máximo observado |
| --- | --- | ---: | ---: | ---: |
| Inicial | 44,1 kHz | 0,8792 ms | 1,8741 ms | 3,5785 ms |
| Inicial | 48 kHz | 0,7963 ms | 0,9785 ms | 1,4022 ms |
| Repetición, mismos bytes | 44,1 kHz | 0,8270 ms | 1,2244 ms | 4,4763 ms |
| Repetición, mismos bytes | 48 kHz | 0,8004 ms | 1,0098 ms | 1,2545 ms |

Ambas corridas registran cero asignaciones durante el callback. A 48 kHz
cumplen la reserva de cuerpo del 20 %; a 44,1 kHz **no la cumplen**. Los
máximos observados quedan dentro del plazo aislado, pero no certifican la
reserva ni el instrumento. No se elige sólo la mejor corrida para aprobar.
La medición excluye presión, cuerdas, martillo, pedales y cierre de puertos;
compilación específica del host, sin WASM. Ver `runtime-conjunto-161.json` y
`runtime-conjunto-161-repeticion.json` en la carpeta de medidas.

Se inspeccionaron los coeficientes de ambos bundles: **cero subnormales**;
transición real mínima no nula 1,71e−13/1,87e−12. Ese control no traza estados
o aritmética intermedia y **no explica la variación de tiempos**. No se
introduce FTZ, recorte de coeficientes o cambios de física sin evidencia.
Informe: `denormales-conjunto-161.json`.

## Conclusión y siguiente prueba

Incluir presión en la dinámica mejora la radiación modelada, con algo de
pérdida en movilidad. Tres pruebas nuevas pasan: gradiente conjunto, solve
escalado y transformación por bloques con conservación de presión/movilidad.
La etapa está implementada y verificada como experimento; el cuerpo todavía
no es una sustitución aprobada del maestro.

El siguiente ajuste debe resolver juntos **dinámica y observador óptimo**,
sin congelar éste durante todas las iteraciones: proyección variable del
problema lineal real del observador, regularización fija y derivada por la
condición de mínimo, contrastada por diferencias finitas. Mantener los dos
gates separados. Si los 161 modos no alcanzan, seleccionar modos adicionales
por el residual conjunto de movilidad/presión, dentro de un presupuesto
medido; no repetir ajustes sólo de picos del puente.

Después siguen transitorios/retardos digitales/antialiasing y costo de cuerpo
**más observación**, antes de cerrar con cuerdas/contacto/pedales y escuchar.
Los límites globales de P6 continúan abiertos. No se instala ni renderiza una
variante que aún falla estos controles como si fuera el modelo completo.
