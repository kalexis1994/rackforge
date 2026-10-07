# P6 — Resolución de cuerda y presión temprana con la tabla completa

Fecha: 2026-10-07. Auditor **ce94f3f**. Continúa el
[control de proyección del campo](RESULTADOS-P6-proyeccion-celdas.md).

## Qué falta y qué mide esta etapa

El banco de contacto/coast completo anterior conserva sólo tres modos
transversales por dirección y seis axiales de una cuerda C4 supuesta. Los
606 modos de tabla no recuperan los grados de libertad de cuerda omitidos.
Se amplía su base espacial manteniendo materiales, fieltro, puente, tabla
y carga acústica. No se modifica EQ, nivel ni velocidad para obtener brillo.

Son modos de una misma cuerda, no nuevas cuerdas de unísono. La dirección
horizontal permanece inicialmente sin excitación; este banco no introduce
imperfecciones, varias cuerdas ni identificación de un Steinway real.

El ensayo compara 3/6/12/24/48 modos transversales **por dirección**, con
2N axiales, a 768 kHz durante los mismos 3 ms de contacto. Continúa cada
extremo vivo otros 0,5 ms a 256 kHz mediante el coast AAVF ya existente.
La tabla conserva sus 606 coordenadas y todos los estados acústicos; se
reutiliza una única R autenticada. No se proyectan fuerzas ni trayectorias.

Controles separados:

- Contacto del banco más amplio a 768/1536 kHz, con idéntica continuación
  a 256 kHz. Mide sensibilidad temporal de la colisión en ese banco.
- 2N/4N axiales con N fijo, ambos a 1536 kHz. Separa resolución axial
  de resolución transversal y cambio de paso.
- Coast a 256/512 kHz desde **el mismo** extremo de contacto a 1536 kHz.
  No reinicia memoria ni cambia el impulso para comparar la propagación.

La deformación `u_x + (v_x² + w_x²)/2` tiene grado coseno máximo
`max(n_axial, 2*n_transverse)`, también con el levantamiento vertical
constante del puente. El kernel existente usa más nodos que ese grado,
conservando la integración de la energía cuártica y sus fuerzas. Para N
transversales, `v_x²+w_x²` sólo fuerza directamente hasta 2N axiales.
Los axiales adicionales, inicialmente nulos, quedan sin forzar dentro de
este Galerkin ideal; no significa que una cuerda real carezca de esos modos.

Cinco pruebas pasan: energía, gradiente y Hessiana contra cuadratura Gauss
independiente hasta 48 transversales/192 axiales, también con el extremo
móvil del puente; cierre axial por encima
de 2N; captura contra el integrador de referencia y su trabajo mecánico;
y unión de trayectorias que rechaza un extremo reiniciado.

## Presión y balances

Se captura cada extremo nativo de posición/velocidad **física** de tabla.
Así se compara la misma base de tabla, aunque cambie la normalización de
masa mixta cuerda/puente al aumentar N. Se exige continuidad exacta al unir
contacto y coast; no se comparan directamente coordenadas normalizadas de
cuerdas de distinto tamaño.

Se calcula la transformada compleja de aceleración mediante Hermite C1 y
sus momentos analíticos, con el mismo operador Green de dos cápsulas. La
grilla espectral es DC y 60–6000 Hz cada 60 Hz. Bandas comparadas:
60–480, 480–1980, 1980–6000 y 60–6000 Hz, con fronteras presentes en la
grilla. La integral espectral positiva duplicada se expresa en Pa²·s;
es un diagnóstico de esa ventana/banda, no energía acústica total en J.

La presión temporal usa la misma grilla de 256 kHz, kernel y tamaño FFT
en todas las resoluciones, limitada a DC–6 kHz. Capturar uno de cada dos
extremos mide la sensibilidad de observación sin cambiar la trayectoria.
Es la contribución de aceleración en **0–3,5 ms**, con propagación/padding
y sin impulsos delta artificiales por sus extremos. No es una nota completa,
su release ni una medición de niveles de parciales sostenidos.

El trabajo mecánico se verifica en cada paso de contacto:

`E_mec(t) + calor_fieltro + calor_material + trabajo_puerto_aire = E_mec(0)`.

El trabajo del puerto es medido por el integrador. Este control **no**
deduce radiación o almacenamiento acústico de ese balance y no reemplaza
su futuro certificado angular independiente. La relajación se registra por
separado. Impulso del fieltro y cambio de momento del martillo deben coincidir.

## Umbrales fijados antes de medir

| Control | Límite relativo |
| --- | ---: |
| Reproducción del banco anterior de tres modos | 1e−10 |
| Balance mecánico de cada paso | 1e−9 |
| Impulso/momento del martillo | 1e−10 |
| Par espacial más fino: presión compleja por banda y temporal | 1% |
| Par espacial más fino: posición/velocidad física de tabla | 1% |
| Control axial fijo: presión por banda y temporal | 1% |
| Par temporal de contacto y de coast: presión | 0,1% |
| Sensibilidad de captura | 0,1% |
| Refinamiento temporal de seis trabajos/energías de contacto | 0,2%, piso 1e−14 J |

Se requiere separación del martillo al acabar el contacto común y pérdidas
no negativas. El control de tres modos debe reproducir cuatro diagnósticos
de contacto, ocho de coast y los dos pares de pico/RMS de presión del coast
ya medido. Los SHA de fuentes, informes padres, tabla y R deben coincidir.
El límite de 1% espacial usa la escala del refinamiento físico de tabla;
el par temporal/captura conserva 0,1%. No se ajustan después de ver los datos.

El aire y la presión ya se validaron hasta **6 kHz**. Resoluciones mayores
incluyen frecuencias libres de cuerda por encima de esa banda: se registran
explícitamente, sin afirmar exactitud de la carga allí. Antes de integrar un
banco audible habrá que comprobar la banda relevante de la fuente y ampliar
la validación acústica si requiere esas frecuencias. La identificación de
geometría/materiales reales sigue pendiente.

## Resultados

El [ensayo completo](medidas-P6-resolucion-cuerda/resolucion-completa.json)
terminó en **2426,356 s (40,44 min)**, con pico de **3,153 GiB**. Pasan
**11 de 13 controles**. Fallan resolución espacial de presión y refinamiento
temporal de contacto; `resolution_gates_passed` y `all_gates_passed` quedan
falsos. No se aprueba todavía el banco de 48 modos ni se cambia el plugin.

### El banco anterior era demasiado pequeño para estudiar el ataque

Mismos 5 m/s iniciales y 0,025 J incidentes; contacto a 768 kHz y coast a
256 kHz en las cinco filas. No se igualan ganancias ni energías finales.

| Transversales/axiales | Mayor frecuencia transversal libre | Modos libres ≤6 kHz | Fuerza máxima centrada |
| --- | ---: | ---: | ---: |
| 3/6 | 785,53 Hz | 3 | 25,3525 N |
| 6/12 | 1581,84 Hz | 6 | 17,2500 N |
| 12/24 | 3248,40 Hz | 12 | 16,5334 N |
| 24/48 | 7134,49 Hz | 20 | 16,2057 N |
| 48/96 | 18511,72 Hz | 20 | 16,1850 N |

La separación pasa de aproximadamente 1,635 ms con tres modos a 1,824 ms
con 48. La fuerza máxima baja **36,16%** sin ablandar el fieltro: ampliar la
cuerda cambia la colisión. Los bancos 24 y 48 tienen los mismos 20 modos
libres bajo 6 kHz, pero los modos superiores también cambian el contacto y
la respuesta del puente dentro de la banda observada.

La diferencia compleja de presión de tres modos frente a 48 es **68,66%**
en 60–6000 Hz; la temporal es 66,83%. Es una comparación entre resoluciones
de nuestra cuerda supuesta, no un error medido contra el Steinway.

| Banda | Integral positiva duplicada: 3 modos, Pa²·s | 48 modos, Pa²·s | Cambio |
| --- | ---: | ---: | ---: |
| 60–480 Hz | 8,34314e−5 | 9,50460e−5 | +13,92% |
| 480–1980 Hz | 3,64863e−4 | 3,14357e−4 | −13,84% |
| 1980–6000 Hz | 3,96404e−6 | 6,38786e−6 | +61,15% |
| 60–6000 Hz | 4,52258e−4 | 4,15791e−4 | −8,06% |

Estas integrales describen sólo la contribución temprana de 3,5 ms. No
demuestran más cuerpo percibido, fidelidad al piano real ni niveles de una
nota sostenida; no corresponden a un cambio del sonido instalado.

### Dos fallos conservados con sus umbrales originales

| Control | Medición relativa | Límite | Resultado |
| --- | ---: | ---: | --- |
| 24→48 transversales, presión 1980–6000 Hz | **1,506794%** | 1% | Falla |
| 24→48, presión 60–6000 Hz | 0,660208% | 1% | Pasa |
| 24→48, presión temporal | 0,643372% | 1% | Pasa |
| 24→48, posición/velocidad física de tabla | 0,214041% / 0,443667% | 1% | Pasa |
| Contacto 768→1536 kHz, presión 1980–6000 Hz | **0,101373%** | 0,1% | Falla |
| Contacto 768→1536 kHz, presión 60–6000 Hz / temporal | 0,014456% / 0,014073% | 0,1% | Pasa |
| Axiales 96→192 con 48 transversales, máxima presión compleja | 6,77149e−8% | 1% | Pasa |
| Coast 256→512 kHz, máxima presión compleja | 0,001402% | 0,1% | Pasa |

La pareja axial usa contacto a 1536 kHz y coast a 256 kHz. El control de
coast usa exactamente el mismo contacto a 1536 kHz, sin repetir el golpe.
El fallo temporal es pequeño, pero no se redondea a aprobación ni se cambia
el límite. La coincidencia axial es consistente con el cierre de este
Galerkin ideal; no certifica todas las interacciones axiales de una cuerda real.

Los cuatro diagnósticos de contacto, ocho de coast y pico/RMS de ambas
cápsulas del banco anterior se reproducen **exactamente**. Pasan balance
mecánico por paso (máximo 3,24435e−12 frente a 1e−9), impulso/momento
(3,44310e−15 frente a 1e−10), pérdidas no negativas, separación, captura,
control axial, propagación temporal, refinamiento de trabajos y autenticación
de fuentes. La sensibilidad máxima de captura es 3,96824e−8 relativa;
el mayor cambio de los seis trabajos/energías al refinar contacto es
0,004934%, bajo 0,2%. La energía mecánica incluye martillo, cuerda y tabla;
no equivale a energía radiada.

## Siguiente control dirigido

1. Separar ambos fallos mediante contacto a **1536/3072 kHz** y bancos de
   **48/96 transversales**, con los mismos parámetros y límites. Una matriz
   dirigida de esas cuatro combinaciones evita repetir todo el barrido grueso.
   Las comparaciones complejas requieren reconstruir las historias de referencia:
   este informe conserva diagnósticos, no sus espectros complejos ni trayectorias.
2. Comprobar la banda relevante de la fuente y la carga por encima de 6 kHz
   antes de adoptar el banco, y certificar almacenamiento/radiación angular
   independientemente. Después ampliar la ventana para una escucha útil.
3. Extender notas y velocidades, acción/repetición/apagadores/release y unísono;
   identificar geometría y materiales reales. No retocar el fieltro para ocultar
   un error de resolución.

El certificado angular fino necesita presupuesto de memoria previo: 3 ms
a 3072 kHz con historia de 650 taps y rango 606 en float64 consumirían
**29.041.459.200 bytes (~27,0 GiB)** sólo para esas entradas. Supera el
presupuesto de 12 GiB del observador, incluso antes de sumar sus demás datos.
Una posible alternativa matemática es evaluar el campo lineal en frecuencia
con términos de estado **inicial y final** de cada sección Padé. Habría que
demostrar equivalencia del flujo finito y medir su coste. Al cerrar este
ensayo no estaba implementada. El [prototipo posterior](NOTA-P6-observador-ventana-finita.md)
ya comprueba la identidad en cascadas sintéticas; aún no es un certificado
completo ni una aceleración medida. No se infiere radiación del trabajo del puerto.

Faltan esos certificados y una ventana más larga, además de los controles
físicos posteriores. Las puertas generales y la aprobación del timbre siguen
falsas; este preflight identifica un límite del banco de prueba, no demuestra
que el sonido se haya acercado al Steinway.

## Reproducción

Python 3.12.14, NumPy 2.3.5, SciPy 1.18.1; BLAS de un hilo y sin bytecode.
Elegir un informe nuevo para conservar toda tentativa. Datos en RAM; R
única en local, informes sólo en el repo. Sin WAV/paquete ni copias en SHARED.

```powershell
python tools/audit-concert-grand-string-resolution.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --field-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-0p5ms.json --control-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-coast-campo/coast-proyeccion-celdas.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVA_RESOLUCION.json
```
