# P6 — Control cuadrático del puerto acoplado

Fecha: 2026-10-09. Sigue al [paso conjunto](RESULTADOS-P6-puerto-acoplado.md),
que conserva trabajo pero falla presión Hann a 128 kHz. Se prueba si su
único gradiente no lineal por paso explica por completo ese fallo.

## Comparación y procedencia

`tools/measure-concert-grand-port-quadratic.py` mantiene masas, rigidez,
modos, pérdidas, cabezas y el puerto acústico del mismo banco C4/G4/C5.
Desactiva solamente el residuo geométrico no lineal. Es una ablación
diagnóstica; nunca se propone como sonido del piano ni para el renderer.

Se reutilizan los cuatro arrays completos del estado C4/84 a 2 ms del
contacto no lineal fino. No se reinicia la resonancia ni se vuelve a
calcular el ataque. El cambio intencional de potencial al desactivar el
residuo es +2,249394e-7 J: energía mecánica 0,006758953172 → 0,006759178111 J.
Se registra separado de las pérdidas; la geometría del control físico
original sigue marcada como no lineal.

Ocho milisegundos, apagadores abiertos, micrófono fijo a 128 kHz. Referencia
cuadrática de presión a 512 kHz ya medida en el [control anterior](RESULTADOS-P6-coast-cuadratico.md).
El replay cuadrático de 128 kHz reproduce exactamente ambos espectros
anteriores. Se conservan ventanas rectangular/Hann, normas complejas,
ganancia y umbral de 1 %. Las cotas de estados usan el replay de 128 y
la diferencia 128→512 anterior mediante la misma desigualdad triangular;
no son errores exactos contra otros estados finales guardados.

## Resultado

Medición completa: **172,009 s (2,87 min)**. Los controles de grilla,
oráculo Hermite y trabajo pasan. Balance mecánico máximo 1,427e-13
relativo. Fuentes, caché y estado inicial permanecen intactos.

Error porcentual Hann frente al respectivo control cuadrático 512 kHz:

| Método / tasa | 60–800 Hz | 800–2500 Hz | 2500–6000 Hz | 6000–12000 Hz | Tiempo de 8 ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Splitting previo / 128 kHz | 0,000995 | 0,008474 | 0,051161 | 0,456273 | 18,946 s |
| Puerto conjunto / 32 kHz | 0,014199 | 0,125661 | **2,278454** | **22,961193** | 5,383 s |
| Puerto conjunto / 64 kHz | 0,003557 | 0,030916 | 0,581471 | **5,928047** | 10,699 s |
| Puerto conjunto / 128 kHz | 0,000882 | 0,007502 | 0,144894 | **1,481181** | 20,727 s |
| Puerto conjunto / 256 kHz | 0,000220 | 0,001715 | 0,035363 | 0,360808 | 41,896 s |

Sólo 256 kHz pasa todos los controles locales del método conjunto. La onda
a 32 kHz tiene cota 1,777960 %, también falla. A 64/128/256 sus cotas son
0,439962/0,107572/0,189778 %; la desigualdad conservadora puede aumentar
sin que aumente el error real. No usar esas cotas para inferir monotonicidad.

El método con física completa había dado 31,778702/6,957089/1,824584/0,377727 %
en la banda superior. Sus referencias físicas y cuadráticas son distintas:
**no se pueden restar estos porcentajes y llamar al resultado una fracción
del error no lineal**. Sí queda refutada la explicación exclusiva por el
residuo no lineal: la banda sigue fallando a 128 incluso al desactivarlo.
La banda tiene poca energía; estos porcentajes no estiman audibilidad.

## Verificación y decisión

Verificación independiente de fuentes e informes de origen por SHA256,
igualdad exacta de referencias, dimensiones/finitud, errores y energías
de bandas, grillas y cotas de extremos. Datos únicos en
`medidas-P6-puerto-cuadratico-2026-10-09/puerto.json` y `presion.npz`
(714.495 bytes; SHA256 `38bd59e229c8ea8383c7b6f9854e3828fa49eef9574ddb7e6e856f5bf45f8eca`).
No se duplican R, estado vivo, informe o WAV en SHARED.

**Decisión:** no seleccionar este control como piano. Probar dos intervalos
no lineales dentro de un solo paso de puerto para medir su contribución,
con energía y coste como controles. No esperar que eso cierre por sí solo
el error cuadrático restante. Después estudiar la discretización del
intercambio con el aire mediante un control de fase de mayor orden.
Siguen pendientes duración/release, ampliación con F4, gate acústico
(9,2021 % frente a 5 %), identificación y escucha. Gates globales falsos.
