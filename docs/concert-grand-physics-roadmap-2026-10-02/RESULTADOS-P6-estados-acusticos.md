# P6 — Estados temporales pasivos de la carga acústica P2

Fecha: 2026-10-03. La matriz compleja anterior quedó en **ec03076**, commit
autorizado por el usuario. Continúa como control congelado de esta realización.
El banco se limita a los 35 modos P2 bajo 800 Hz y a un ensayo temporal de
24 ms. La fábrica sigue en 0.172.0; esta etapa no produce audio ni paquete.

## Representación y energía

La resistencia modal de Rayleigh se evalúa en 481 frecuencias hasta 6000 Hz,
con la misma superficie de celdas 128×96 y formas Ritz normalizadas por masa.
Se interpola R/Ω² por combinaciones positivas para conservar su límite
cuadrático a baja frecuencia. Sus pesos espectrales se factorizan como GᵀG,
reteniendo las proyecciones mutuas entre modos. La discretización genera
osciladores colocalizados y una masa inercial positiva para la cola espectral:

```text
Z(s) = s M_tail + Σ W_j s/(s² + 2 γ_j s + Ω_j²)
W_j = 4 R(f_j) Δf_j = G_jᵀ G_j
M_tail = M_static − Σ W_j/Ω_j²

Tabla: (I + M_tail) v_dot = F − K q − D v − Gᵀ p
Aire:  a_dot = p; p_dot = G v − Ω² a − 2 γ p

E = 0,5 qᵀKq + 0,5 vᵀ(I+M_tail)v
    + 0,5 pᵀp + 0,5 aᵀΩ²a
E_dot = Fᵀv − vᵀDv − Σ 2γ_j p_j²
```

La fuerza de interfaz y la excitación del aire usan la misma G y su
transpuesta. Con K positiva, D/M_tail semidefinidas positivas y γ≥0, la
identidad de energía establece pasividad de **este sistema realizado**,
además de las verificaciones numéricas. La integración por punto medio
conserva esa identidad de trabajo para cualquier paso positivo; su precisión
todavía requiere refinamiento. Un complemento de Schur evita resolver una
matriz densa con todos los estados de aire.

Las realizaciones temporales de impedancia necesitan causalidad, realidad y
pasividad; un buen ajuste de muestras de frecuencia no basta por sí solo.
[Documentación técnica de COMSOL](https://doc.comsol.com/6.3/doc/com.comsol.help.models.aco.wave_based_room/wave_based_room.html).
La necesidad de comprobar causalidad también aparece en aproximaciones de
radiación acústica. [Silva et al., 2008](https://arxiv.org/pdf/0811.3625).
La construcción de osciladores y la identidad anterior se verifican aquí
contra controles independientes; no se adopta una aproximación de tubo para P2.

## Aproximaciones explícitas

- Geometría P2 supuesta: una cara plana en baffle infinito, ρ=1,2 kg/m³ y
  c=343 m/s. No identifica la tabla del Steinway ni su segunda cara/borde.
- γ ensancha los polos del baño finito. Es **absorción numérica**, no una
  pérdida material, atmosférica ni radiativa identificada del piano.
- La energía del aire incluye campo cercano, ondas presentes en el baño y
  masa de cola. No debe llamarse sólo energía reactiva ni confundirse su
  absorción numérica con potencia radiada medida en un micrófono.
- La cola sobre 6000 Hz usa su primer término inercial. La equivalencia de
  transferencia se verifica en 60–800 Hz; no se reclama precisión global.
- La malla espectral finita puede producir recurrencias a tiempos largos.
  Su tramo alto tiene paso 10 Hz; no basta citar el período de 4 s del tramo
  fino para descartar todas las recurrencias. La validación temporal es 24 ms.
- Se rechazan pesos o masas significativamente indefinidos. La factorización
  elimina negativos de redondeo y rangos numéricos inferiores a 1e−13 del
  autovalor máximo. La corrección espectral máxima es 7,694e−13 relativo;
  no se corrige un error material de pasividad para aceptar el modelo.

## Precisión frente a la integral de Rayleigh

Se usan **187 frecuencias de control**: malla de 5 Hz en 60–800 Hz,
frecuencias propias de P2 dentro de banda y frecuencias adicionales no
alineadas. Se comparan R, X y Z completas; también la velocidad de tabla
bajo fuerza 1 N en el punto C4, con D=0,046·ω_j **supuesto material**.

| Ensanchamiento γ/(2π) | Error máximo Z | Error máximo R | Error máximo X | Error máximo velocidad cargada |
| --- | ---: | ---: | ---: | ---: |
| 2 Hz | 0,84836% | 2,13099% | 0,85321% | 0,81130% |
| 1 Hz | 0,43239% | 1,08619% | 0,43925% | 0,40569% |
| **0,5 Hz** | **0,22332%** | **0,54968%** | **0,23212%** | **0,20272%** |

Se fijaron antes del ensayo límites de 2% para Z/R/X y para tres controles
de Laplace, 5% para velocidad cargada y 1e−10 para masa estática/correcciones
de factorización. La variante 2 Hz falla el límite de R; el control fino pasa
todas las puertas. Su error de Laplace máximo es 0,20277%, y la masa estática
se conserva a 2,762e−15 relativo. Los errores son frente al integral de la
misma superficie 128×96; su aproximación de la superficie continua sigue
siendo una fuente separada de error, documentada en la etapa anterior.

El control fino tiene **5280 segmentos, 176013 canales oscilatorios y 352026
estados de aire**, más los estados de madera. La cola positiva tiene
autovalores 0,000717802–0,001713658 en coordenadas de masa normalizada.
Es un banco de referencia pesado: simular 24 ms a 384 kHz tomó 134,20 s
en este entorno con un hilo BLAS. Se necesita reducción de coste para el plugin.

## Dos golpes y convergencia temporal

Se aplican impulsos físicos prescritos de 0,002 y 0,0015 N·s en el punto C4
del puente, a 0 y 8 ms. Son excitaciones de diagnóstico de la tabla lineal;
no equivalen al contacto del martillo ni a una nota de fábrica. Se conserva
exactamente el estado a/p del aire al segundo impulso. D=0,046·ω_j es una
hipótesis material de este control, no una nueva partición de pérdidas de P2.

| Refinamiento | Cambio RMS de velocidad | Cambio RMS de desplazamiento |
| --- | ---: | ---: |
| 48→96 kHz | 0,56606% | 0,13890% |
| 96→192 kHz | 0,14140% | 0,03469% |
| **192→384 kHz** | **0,03535%** | **0,00867%** |

El criterio previo es 0,1% para cada estado; 96→192 falla velocidad y
192→384 pasa. La reducción cercana a cuatro por duplicación concuerda con
el orden dos del integrador. El control sin absorción del baño es estable
y conserva su energía acústica; γ=0,5 Hz cambia su velocidad sólo 0,03536%
RMS en 24 ms, y γ=1 Hz cambia 0,07060%. Esto no valida colas largas.

Presupuesto fino a 384 kHz, unidades μJ:

| Término | Energía |
| --- | ---: |
| Trabajo externo de ambos impulsos | 6,815858 |
| Energía final de madera | 0,666360 |
| Energía final de aire y cola | 0,975151 |
| Calor material | 5,064508 |
| Absorción numérica del baño | 0,109839 |

Balance total máximo: **6,168e−13 relativo**. Balance final de aire:
8,954e−14; de madera: 5,270e−13, referidos al trabajo externo. El presupuesto
de puerto incluye **0,005544 μJ** de trabajo instantáneo de la masa de cola
durante los impulsos, separado del trabajo continuo entre pasos.
2216 pasos tienen trabajo de puerto negativo: el aire devuelve energía;
la absorción numérica permanece no negativa. Todos los controles finales pasan.

## Pruebas y siguiente paso

Seis pruebas verifican: transferencia contra un sistema de estados denso
independiente; identidad de energía y devolución del aire; solución contra
exponencial matricial, estabilidad y orden dos; conservación del aire al
reiterar; espectro contra impedancia esférica analítica y masa estática;
y rechazo de parámetros no pasivos. Todas pasan.

Siguiente: acoplar este control al **contacto no lineal SI** con el puente,
incluyendo la masa de cola en la inercia compartida y conservando el aire
entre golpes. Después reducir coste preservando pasividad/transferencia y
validar horizontes más largos antes de escuchar. Separar pérdidas materiales
de radiación antes de cualquier promoción. Banco alto, unísono, geometría,
Timoshenko, graves, apagadores/backcheck y pedal siguen pendientes.

## Datos y reproducción

Todo queda en `C:\SHARED\Fisica-pendiente-2026-10-03\P6-estados-acusticos`:
`spectrum.npz`, curva de errores, parámetros `bath-fine.npz`, seis trayectorias
NPZ/CSV con estados finales de aire, informes JSON, fuentes y log de pruebas.
El resumen guarda entorno, criterios, hashes de entradas, dependencias y paquete.

SHA256 de `source-acoustic-bath.zip`:
`8fa8884c7e2679eb59874cb104522ffba0092741d26e6ac075bca29ab11559ce`.

SHA256 de parámetros `bath-fine.npz`:
`0c9948d8305bb5f61d8c75f19aff3a5ab8058176f6f1afc36ca0bd5e14d7a8d5`.

Con NumPy/SciPy y un hilo BLAS:

```powershell
$asset = 'C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-estructural/Panel-costillas-puente.npz'
$outputDir = 'C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos'
python -m unittest discover -s tools/tests -p test_concert_acoustic_bath.py -v
python tools/audit-concert-grand-acoustic-bath.py --phase spectrum --asset $asset --output $outputDir
python tools/audit-concert-grand-acoustic-bath.py --phase reference --asset $asset --output $outputDir
python tools/audit-concert-grand-acoustic-bath.py --phase realize --asset $asset --output $outputDir
python tools/audit-concert-grand-acoustic-bath.py --phase time --asset $asset --output $outputDir
python tools/audit-concert-grand-acoustic-bath.py --phase summary --asset $asset --output $outputDir
```

Paquete 0.172.0 preservado:
`ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb`.
