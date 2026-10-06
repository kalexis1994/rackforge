# P6: contacto completo en coordenadas y prefijos temporales

2026-10-06. La masa y transferencia completas, junto con las recurrencias de
dos estados, quedan congeladas en **6253412**, por pedido del usuario.
Esta etapa conecta esa carga con el contacto no lineal conservando los 606
modos de tabla, pérdida material completa y memoria acústica persistente.

## Alcance del banco

Una cuerda C4 supuesta: longitud 0,62 m, masa lineal 0,0081 kg/m y tensión
850 N. Tres modos transversales por dirección y los axiales del núcleo SI.
Martillo de 2 g, velocidad incidente 5 m/s, fieltro `7,5e8`, exponente 2,6,
posición relativa 0,125, huella 0,02 y pérdida dependiente de tasa 0,1.
Son los parámetros del control mecánico anterior, no un refit del Steinway.

Se conserva exactamente el cambio de coordenadas de la proyección y su
pérdida material entre modos. La cuerda recibe movimiento/reacción del puente
durante el contacto; la tabla recibe la carga acústica viva, sin masa de aire
instantánea duplicada. El soporte y el aire usan la misma base modal.

El banco mide **prefijos de 125 microsegundos** a 384, 768 y 1536 kHz:

- `Cold`: contacto desde reposo, con estados acústicos inicialmente nulos.
- `Continued`: prolongación desde el final del prefijo frío más fino. Todas
  las tasas parten del mismo estado completo, conservando cuerda, tabla,
  martillo, ambas coordenadas de onda y relajación. No se relanza el martillo.

Así se separa el error temporal de diferencias en la condición inicial.
Esto no es una nota completa, separación natural del martillo, segunda
pulsación, apagador/release ni comparación audible. Esas etapas quedan abiertas.

## Una única caché numérica

`local/concert-grand-physics/P6-carga-causal-2026-10-06/` conserva sólo las
correlaciones `R.npy` y su manifiesto. El factor angular completo no se guarda.
La identidad incluye SHA del tablero proyectado, informe armónico aprobado,
fuentes físicas, tasa, celdas, cuadratura, derivada y simetría. El archivo R
tiene su SHA; se rechaza un cambio de identidad, dimensiones o contenido.
No se repara ni se sobrescribe automáticamente una caché existente.

La recarga usa una vista de sólo lectura y conserva todos los taps/modos.
La construcción usa bloques de 32 direcciones frente a 8 en la corrida
anterior. Cambia la agrupación de sumas, no las direcciones ni el operador.
Antes de usarla se exige reproducir la masa y las medidas Z/R/X de 60,
1881,960712 y 6000 Hz del informe aprobado a < 1e−10.

Esta es una caché nueva y única, necesaria para evitar reconstruir la misma
carga en cada ensayo. No duplica otra R almacenada, los informes ni archivos
de `SHARED`. Su tamaño esperado es ~1,91 GB; los historiales del certificado
se mantienen en RAM y se liberan al terminar.

## Certificado independiente en una sola pasada

`certificate_batch` regenera cada bloque angular una vez y calcula:

```text
E_onda = 144 * E_base(x1) + 12 * E_base(x2)
E_base(x) = sum(norm(colas_futuras(H, x))²) / (2*rate)
P_radiada = norm(sum(H_j * u_j))² / 2
```

Los dos términos de energía, todos los estados iniciales/finales y todos
los inputs centrales se evalúan con el mismo factor positivo. No se infiere
la radiación como diferencia entre trabajo y almacenamiento. Se agrupan las
observaciones; no se truncan modos, direcciones ni colas. La matriz de
observación se prepara una vez por bloque y se reutiliza en los lotes.

Dos pruebas nuevas pasan: energía/radiación/trabajo contra el factor completo
almacenado y su certificado por convolución; caché contra transferencia y
midpoint con memoria viva, con rechazo de identidad alterada/corrupción.
La igualdad por recarga es exacta en esos controles pequeños; el certificado
por bloques exige error < 1e−11 y conserva radiación no negativa.

El banco completo acumula por separado pérdida de fieltro, material,
relajación y ondas, y trabajo del puerto. Mide almacenamiento inicial/final
de forma independiente. Exige error relativo < 1e−9 en balance total y
acústico, reacción < 1e−9 N y residuo de potencia < 1e−9 W.

El refinamiento compara la misma grilla de captura y las mismas condiciones
iniciales. El par más fino exige < 1% en trayectoria mecánica/estado final de
tabla y < 0,2% en impulso/fuerza máxima. La razón de errores contra la
referencia fina debe ser < 0,35 en las dos fases. No se denomina referencia
exacta a esa última tasa: es un control de refinamiento de paso finito.

## Presupuesto de esta corrida

El auditor declara por separado los arrays de construcción y certificado:
4.894.085.088 y 5.848.089.920 bytes, respectivamente, bajo 8 GiB. Los inputs
de todas las secciones ocupan 2.117.606.400 bytes. El working set real se mide
aparte porque el presupuesto de arrays no limita al intérprete/BLAS/SciPy.

`all_gates_passed` permanece falso incluso si pasan los controles del prefijo:
no aprueban la duración completa del contacto, repetición natural, release,
coste de audio, propiedades físicas identificadas ni timbre.

## Reproducción

Python/NumPy/SciPy, BLAS de un hilo:

```powershell
python -m unittest discover -s tools/tests -p test_concert_pade_certificate.py -v
python tools/audit-concert-grand-pade-contact-full.py --full-report docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-aire-bloques/carga128-pade2.json --board local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz --cache local/concert-grand-physics/P6-carga-causal-2026-10-06 --report NUEVO_INFORME.json
```

Se requiere una ruta libre para cada nuevo informe. La caché compatible se
reutiliza; su contenido se autentica. Los resultados permanecen en el repo,
sin WAVs ni paquete nuevo en `C:\SHARED`.
