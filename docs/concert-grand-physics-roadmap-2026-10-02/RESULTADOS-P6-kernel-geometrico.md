# P6 — Kernel geométrico y corrección del comparador

2026-10-04. La etapa anterior quedó en **de2cb2e**, por pedido del usuario.
**Sin modificación del plugin ni selección para fábrica.** Hay una referencia
temporal directa y un error de comparación corregido. La mejor carga racional
se aproxima al límite de memoria, con poco margen ante refinamiento espacial.

## Corrección de la etapa anterior

`linear_drive` del baño devuelve **fuerza regular total** durante la excitación:
Gᵀp + M_tail·dv/dt. En cada nudo usa la derivada del segmento **izquierdo**,
incluido el último nudo a 4 ms. Sólo excluye la impulsión delta de la cola
inercial al imponer después velocidad cero. La fuerza inicial se fija a cero.

El comparador racional v1 interpretó esa referencia como no inercial,
descontó otra vez su cola y utilizó derivadas derechas. Además, el evaluador
racional omitía su propia fuerza inercial regular en el último nudo.
**Los RMS 5,29358%, 6,60586% y 8,76807% de v1 quedan invalidados como
comparaciones de fuerza equivalentes.** Los coeficientes, estados, masa,
pruebas de pasividad y verificaciones de frecuencia de esa etapa siguen válidos.
No se cambió la física de la carga racional para obtener la corrección.

Se corrigen ambos evaluadores a fuerza total regular con derivada izquierda.
El test del baño ahora comprueba explícitamente la fuerza en cada nudo contra
una ODE; el test de parada racional también incluye una cola positiva.
El script racional impide mezclar un informe de memoria v1 con datos v2:
permite leer coeficientes congelados y escribir nuevas medidas en otro destino.

La nueva comparación de la variante fina contra el mismo baño da **3,58061%**
RMS, todavía por encima de 2%. Es una corrección del diagnóstico, no una mejora
audible. Los NPZ, JSON y ZIP originales se conservan; este informe los sucede
para memoria/fuerza. Los archivos v2 tienen nueva procedencia y hashes.

## Referencia directa, sin polos ajustados

Se parte del mismo Rayleigh de la superficie constante por celdas:

Z(s) = ρs/(2π) ∬ φ(x)φ(y)ᵀ exp(−s|x−y|/c)/|x−y| dSx dSy.

Si Z(s)=s·Laplace[K](s), el cambio a coordenadas polares convierte K(t) en
ρc/(2π) por la integral, sobre un círculo de radio ct, del solapamiento de
dos celdas. Ese solapamiento es
(hx−|x−dx|)₊(hy−|y−dy|)₊. Se divide el círculo donde cambian los signos y
los bordes; en cada arco se integra una combinación de 1, cos, sin y cos·sin.
Así también se incluyen los términos locales y entre celdas que se tocan.

Las correlaciones modales espaciales se calculan por FFT y se agrupan para
los cuatro signos del desplazamiento; la integral temporal no usa IFFT de
una grilla de frecuencias. La convolución de la entrada usa padding suficiente.
No hay histograma de retardos, corte espectral, pérdida Γ ni masa de cola.
La primitiva se centra en cada arco y usa series para diferencias pequeñas:
evita cancelar términos grandes al integrar solapamientos muy estrechos.

La respuesta espacial y la convolución temporal tienen antecedentes en
[Stepanishen, Transient Radiation from Pistons](https://doi.org/10.1121/1.1912541)
y [Time-Dependent Force and Radiation Impedance](https://doi.org/10.1121/1.1912424).
La proyección modal y la implementación por solapamientos de esta etapa se
verifican independientemente contra la integral de Duffy del repositorio.

Para esta geometría, K(0)=ρc∫φφᵀdS, el límite resistivo local correcto.
El soporte termina en diagonal/c = **5,0159331 ms**. Tras 4 ms de velocidad
prescrita, la fuerza acaba antes de 9,0159331 ms. No se aplica una máscara ni
umbral de salida para hacerla desaparecer. Es el soporte físico de una cara
en bafle infinito, sin reflexiones, segunda cara, sala o borde real del piano.

Una entrada lineal por segmentos se integra con K; la parada exacta añade
−K(t−4 ms)·v_final. La fuerza en el nudo de parada es su límite izquierdo.
El modelo geométrico no tiene una delta inercial separada. Esto sigue siendo
un puerto prescrito, **no una nota libre acoplada ni un render de audio**.

## Validación numérica

Cinco tests nuevos verifican integral angular adaptativa, correlaciones contra
pares densos, invariancia por subdivisión de un panel uniforme, límite local,
masa/Laplace contra Duffy y fuerza de segmentos/parada contra integración
temporal independiente. Los tests de Laplace ejercitan matrices complejas.
Pasan los 22 tests de geometría, superficie, carga racional y reducción.

Con las mismas celdas 128×96 y 35 modos P2:

- Error máximo Z frente a los 187 puntos físicos bajo 800 Hz: <5,55e−9
  relativo; R/X <7,58e−9. Tres controles de Laplace también pasan.
- En seis frecuencias adicionales de 1–6 kHz, error complejo <7,11e−8
  relativo; Gauss 16 lo baja a <2,85e−8. La reactancia diminuta en alta
  frecuencia amplifica sus cocientes relativos; se publican también Z y R.
- Masa estática relativa: 1,52e−9 con Gauss 16. No se impone igualdad artificial.
- K(0) coincide con el límite local a 3,70e−16 relativo.
- Gauss 8→16 cambia la fuerza a 4,36e−9 del RMS común; 128→256 kHz,
  a 8,00e−10. La discrepancia transitoria no viene de ese integrador.

El criterio de cuadratura/referencia es 1e−5; el refinamiento temporal, 0,1%.
El criterio de masa exacta 1e−10 de la reducción corresponde a su ajuste DC,
no se usa para exigir igualdad de dos integraciones físicas independientes.
El kernel continuo es el Rayleigh pasivo conocido; **aún no se ha construido
un integrador acoplado con certificado energético para esta convolución**.

## Comparación de fuerza con convención v2

Todos los errores usan el **mismo RMS y pico inicial del baño congelado**.
No se normaliza cada variante ni se cambia el denominador al usar geometría.
Se conservan el límite RMS de 2% y el de pico tardío de 1%.

| Realización | RMS frente al baño, corregido | RMS frente al kernel, celdas 128×96 |
| --- | ---: | ---: |
| Baño completo, 352026 estados | — | 3,97163% |
| Racional 32 kHz, angular 24×48 | 5,33574% | 4,62459% |
| Racional 64 kHz, angular 24×48 | 7,89600% | 6,25710% |
| Racional 32 kHz, angular 48×96 | 3,58061% | **1,94783%** |

La última variante conserva sus 5740 estados y pasa memoria frente a la
superficie congelada. El pico después de 10 ms frente al kernel es sólo
roundoff. No pasa frente al baño anterior; el baño tampoco pasa como
referencia exacta de esta parada de banda ancha.

El aporte global del error del baño durante la excitación es 0,12104%; tras
la parada hasta 10 ms, 3,94905%. La variante racional fina aporta 0,27291%
durante la excitación y 1,92862% tras la parada. Estos aportes usan el RMS
global común y se combinan cuadráticamente; no son errores normalizados por
cada ventana. Las discrepancias máximas están cerca de 4,03125 ms.

La cola inercial del baño de 6 kHz aproxima frecuencias omitidas sólo en
primer orden. Una parada instantánea tiene banda ilimitada: su regularización
difiere del pulso geométrico continuo, y las deltas del baño no aparecen en
esta curva muestreada. Este resultado **no** invalida sus balances de energía
ni los contactos ya medidos; limita su uso como verdad transitoria de banda
ancha. Se conserva como control de su propio modelo, no como referencia única.

## Refinamiento espacial y margen

La superficie P2 continua se aproxima por promedios de celdas. Aumentar la
resolución cambia esa superficie, no sólo la cuadratura temporal. Se conservan
los mismos modos/velocidad/coordenadas y la misma escala del baño.

| Celdas del kernel directo | RMS de la variante fina frente al kernel |
| --- | ---: |
| 128×96 | 1,94783% |
| 192×144 | 1,98033% |
| 256×192 | 1,99520% |

El cambio de fuerza de la referencia es 0,15622% entre las primeras grillas
y 0,05533% entre las últimas; el segundo pasa un control de 0,1%.
Aunque las tres medidas nominales del candidato quedan bajo 2%, su margen
final es menor que el cambio espacial observado. **No se afirma un pase
robusto respecto de la superficie continua ni se selecciona para fábrica.**
No se siguen refinando grillas sólo para forzar un resultado de aceptación.

Los blancos bajos/Laplace de los controles espaciales siguen siendo los
congelados 128×96; sus diferencias allí son diferencias de discretización.
Los seis blancos adicionales altos se calculan independientemente en la
grilla de cada control. Los metadatos indican ambos orígenes.

## Decisión y siguiente paso

Se dispone de una referencia temporal directa que respeta propagación, término
local y límite alto. El comparador usa ahora fuerzas equivalentes y detecta
correctamente una limitación de la referencia finita. No se modificó la voz
instalada ni se atribuye a estas medidas una mejora del cuerpo del Steinway.

Siguiente: mejorar el margen de la representación compacta frente a este
kernel, incluyendo masa y límite resistivo alto, y repetir tabla/contacto
acoplados con estados persistentes. Su certificado de pasividad sigue siendo
necesario. Luego ampliar velocidades/notas/banda, identificar pérdidas y
retomar observación SI/escucha. Unísono, P4, acción/pedal y ambas caras/borde
continúan pendientes. El evaluador geométrico no es un motor en tiempo real.

## Datos y reproducción

`C:\SHARED\Fisica-pendiente-2026-10-04\P6-kernel-geometrico` contiene
coeficientes integrados, fuerzas totales, matrices complejas, controles
temporales y espaciales, fuentes, hashes, pruebas y `resumen.json`.
`rational-v2` conserva la comprobación independiente del script racional
corregido. Las entradas de las etapas anteriores permanecen congeladas.

```powershell
python -m unittest discover -s tools/tests -p test_concert_geometric_memory.py -v
python tools/audit-concert-grand-geometric-memory.py --asset C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz --bath C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos --contact C:/SHARED/Fisica-pendiente-2026-10-03/P6-contacto-aire/T72-768000-Air.npz --memory C:/SHARED/Fisica-pendiente-2026-10-03/P6-reduccion-aire/Grid2-Rank0.01-High40-Gamma2-Parts-memory.npz --rational C:/SHARED/Fisica-pendiente-2026-10-03/P6-carga-racional --output C:/SHARED/Fisica-pendiente-2026-10-04/P6-kernel-geometrico
```

Para los controles espaciales usar `--cells 192 144` o `--cells 256 192`,
`--rates 128000 --quadratures 16` y otro destino. En esta máquina la corrida
temporal y tres comparaciones racionales tardan unas decenas de segundos.
Esto no mide el coste de un solver acoplado ni de un plugin.
