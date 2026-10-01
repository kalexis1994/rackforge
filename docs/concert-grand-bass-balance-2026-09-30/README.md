# Graves dominantes y ataque débil

Después de comparar F#2 exacto, el usuario oyó el modelo gordo, sin ataque,
como un bass boost. Se mantuvo el modelo de contacto duplex corregido,
sin ruido de golpe añadido ni desafinación del unísono.

## Diagnóstico

`tools/audit-concert-grand-bass-balance.py` compara C2/D#2/F#2/A2/C3/F#3/C4
a velocidades 60/124 y F#2 también a 40/84. Todas son raíces grabadas
exactamente en BitKlavier Grand Stage ORTF. Mismo sampler, 44,1 kHz,
ganancia del modelo fijada antes de esta comparación: 26,401176937389373 dB.
Sin nivelar notas, bandas ni ventanas individualmente.

La mediana en C2–C3 a 50–200 ms muestra +13,28 dB en 25–180 Hz,
+6,02 dB en 180–600 Hz, −9,08 dB en 600–1800 Hz y −9,40 dB en
1800–8000 Hz. Quitar el pulso de contacto estructural o la rampa bloom
apenas cambia ese cuerpo. Endurecer el fieltro recupera parciales superiores,
pero reduce el exceso grave menos de un decibelio en la mediana. Son dos
problemas diferentes, no sólo una sensación causada por volumen.

El observador físico actual usa presión de placa con bafle, sin resolver
la radiación de la cara posterior ni la difracción en la caja. La referencia
de [KTH](https://www.speech.kth.se/music/5_lectures/introd/introd.html)
describe la ineficiencia grave por intercambio de aire entre ambas caras.
La [investigación de Russell](https://www.acs.psu.edu/drussell/publications/pianohammer.pdf)
también explica cómo el contacto y la radiación contribuyen a que dominen
parciales superiores en el bajo. Esto justifica estudiar ambas etapas;
no identifica numéricamente la radiación de este Steinway.

## Variante de escucha, no identificación física completa

Se ensaya una calibración residual continua de la fuerza modal de graves
`g = 1 + w*(f/sqrt(f²+fc²)-1)`, donde `w` es smoothstep entre C4 (cero)
y C3 (uno), con valor uno por debajo de C3. `fc=400 Hz` en la variante.
Se aplica después de normalizar el golpe, por lo que esa normalización
no vuelve a recuperar el exceso grave. El factor está entre cero y uno,
no añade ruido, estados de compresión, resonancias ni un ataque paralelo.
Es una corrección efectiva de la transferencia del modelo reducido,
**no** un solver de difracción de las dos caras ni una EQ identificada
físicamente. Sólo el camino de investigación de puente físico la usa.
Por defecto `BASS_BRIDGE_CORNER_HZ=0` deja el modelo anterior intacto.

La curva de rigidez del contacto aumenta ocho veces su coeficiente de
referencia A0, compensando las pendientes para conservar K desde C4.
El factor efectivo en F#2 es menor: unos 2,61. La transición de la tabla
de fieltro existente se conserva. No hay corrección específica de F#2.

Se compararon factores de referencia 4/8/16 y esquinas 400/550 Hz.
K16 mejora alguna medida agregada pero excede en unos 6 dB la banda
de 1800–8000 Hz de F#2 suave. Se eligió K8/400 para escuchar: ataque
superior moderado, sin el exceso brillante de K16 ni la mayor pérdida
de cuerpo de la esquina 550. La medida agregada no es un score de fidelidad.

## Comprobaciones y límites

- 114 pruebas nativas aprobadas; 75 pruebas de laboratorio ignoradas.
- Ganancia acotada y monotónica, identidad con control cero y desde C4.
- Control desactivado: audio idéntico al binario anterior en 14 pares de
  notas/velocidades del diagnóstico.
- C4 a 60/124: la variante produce audio idéntico muestra a muestra.
- F#2: reducción de 12,8–13,3 dB en 25–180 Hz a 50–200 ms, según
  velocidad; máximo de energía en ventanas de 10 ms pasa de 40 a 20 ms.
  La referencia alcanza ese máximo a 10 ms. No se ha igualado el ataque.
- Sigue faltando contenido en 600–1800 Hz en F#2; C3 conserva exceso
  en parte del cuerpo y F#3 ya tenía un hueco de medios que la variante
  no resuelve. Hace falta mejorar la transferencia espacial del modelo.
- Los controles de unísono, el ruido de golpe desactivado y la corrección
  del impulso duplex anterior permanecen activos como en el candidato.

La variante no reemplaza automáticamente el candidato ni se instala.
Resultados en `C:/SHARED/Graves-con-ataque`: comparación F#2 nuevo/Steinway,
antes/después, intensidades separadas, archivos individuales, configuración,
mediciones y patch de la copia de investigación. Sin transposición.

Binario congelado: `local/concert-grand-bass-transfer/renderer.exe`, SHA256
`ab4cfa0e43c2dcb0cd458631515fcc6fd2c9ded7478e554126f49ff957333a5a`.
Los diagnósticos completos están en `C:/SHARED/Graves-balance-diagnostico`
y `C:/SHARED/Graves-transferencia-diagnostico`.
