# P7 — Eliminar incógnitas axiales sin eliminar su dinámica

2026-10-13. Continuación de optimización autorizada. Base **ac8d2dda**;
coro precedente **928cd20f**, fuera del paquete de fábrica.

## Identidad utilizada

En el potencial finito SI, el gradiente no lineal axial es
`g_u = EA D_uᵀ W η(v,w)`, con `η=(v_x²+w_x²)/2`. No depende de `u`.
Para un extremo transversal propuesto se puede evaluar `g_u` directamente,
componer todos los portadores axiales con ese extremo y volver a evaluar
las fuerzas transversales con la posición axial actualizada. Se conserva
`q0/v0`, masa, rigidez, fases rápidas y todos los modos 96/192 por cuerda.
No es una aproximación axial estática ni una reducción del espacio físico.

El trabajo axial y su desplazamiento ponderado se sustituyen también por
los de la nueva trayectoria. La referencia `--micro-reference` reproduce
esos mismos portadores paso a paso. `--uncondensed` conserva la iteración
axial completa. Tolerancias: fuerza/corrección 1e−11, trabajo del bloque
1e−13 de su energía; estado/fuerza temporal ≤1%, ledger ≤1e−9.

## Primer candidato: mejora de costo, equivalencia rechazada

[Primer costo](medidas-P7-cuerdas-nativas-2026-10-13/axial16-primer-costo.json):
C4 limpia, intensidad 84, 128 kHz/16, 128 buffers. Máximo de iteraciones
10→4; media **2,0725 ms**, P99 **3,1743 ms**, máximo/ataque **4,7852 ms**
frente a 2 ms. Cero asignaciones. Estado corto frente al monolítico
0,100696%, ledger 1,69e−14. Frente al anterior avance por bloques, cambio
de estado 7,23e−8 y de fuerza 1,12e−9.

[250 ms con reiteración](medidas-P7-cuerdas-nativas-2026-10-13/axial16-reiteracion250ms.json):
estado **0,324165%**, ledger **2,64e−12**, ambos pasan. Gauss compuesto
independiente confirma la fuerza axial a partir de las posiciones
transversales finales para 4/12/96 modos.

Eso no basta para aprobar equivalencia de solver. El control con ambos
campos perturbados y seed 83 da una diferencia de **1,4616e−5** en 4 modos
y **0,00607482** en 96: ambos fallan el límite adicional 1e−6, aun cerrando
trabajo. [Rechazo conservado](medidas-P7-cuerdas-nativas-2026-10-13/axial16-correction-ambiguity.json).
La prueba unitaria de equivalencia falla y este candidato no se selecciona.

La corrección anterior acumula incrementos en direcciones de desplazamiento
distintas durante las iteraciones. Al converger, el ledger impone un producto
escalar, pero deja indeterminado el componente perpendicular de esa fuerza.
Cambiar el camino de iteración puede cambiar ese componente y la trayectoria.
Sigue definir una corrección paralela al desplazamiento de las cuerdas para
que ambas rutas resuelvan un paso determinado, conservando el rechazo y el
control monolítico. El contacto y la cabeza quedan fuera de esa corrección.

## Corrección determinada

Se fija `c = lambda (q_end - q_start)` sobre las cuerdas. La corrección se
proyecta completa en esa dirección durante la solución, en lugar de
acumular incrementos que dejan una componente perpendicular indeterminada.
Las rutas con/sin eliminación axial vuelven a pasar la equivalencia ≤1e−6
en ambos controles perturbados, manteniendo los límites de fuerza/trabajo.
La referencia por micro-pasos y Gauss independiente también pasan.
`--legacy-correction` conserva el algoritmo rechazado para diagnóstico.
La comparación de equivalencia es entre las dos rutas del nuevo paso
determinado; no se presenta como identidad con el paso anterior ambiguo.

[Primer costo con dirección determinada](medidas-P7-cuerdas-nativas-2026-10-13/axial16-canonical-vector.json)
conserva el ataque y el fallo de presupuesto de 2 ms. Todavía se itera un
vector de corrección completo. Sigue aprovechar su único coeficiente
escalar en el vuelo certificado, sin dejar de verificar el contacto fino.
