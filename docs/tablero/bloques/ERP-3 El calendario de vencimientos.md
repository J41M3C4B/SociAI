---
tipo: bloque
id: ERP-3
carril: erp
estado: listo
prioridad: media
caja: Eventos y tareas automáticas; Servicios deterministas
pedido_por:
depende_de: IA-2, ERP-1
rama:
adr:
creado: 2026-10-20
cerrado:
---

## Qué y por qué

Según la investigación externa (§1.2), el beneficio que cualquier dirección entiende es **no perder la autorización del SAT ni la CLUNI**: en 2025 se revocaron 270 donatarias. Este bloque calcula, sin IA, las fechas que vencen:
- el informe CLUNI, el 31 de enero;
- la declaración anual, el 15 de febrero;
- la transparencia, en mayo;
- la vigencia de la constancia de actividades.

Los avisos salen por la entrega común de resultados.

**Cambio con el plano del ERP (ERP-7, 2026-10-20).** El calendario es del **núcleo** (K8, [plano del ERP](../../plano-del-erp.md), §5 y paso 7 de la §11), y las obligaciones las declara el módulo dueño de sus datos: estas cuatro, Mi institución. Cumplimiento no es un módulo. Por eso va después del ERP-1.

**Del [[ERP-10 Lo que pide el plano de la IA al núcleo|ERP-10]] (punto 6, ADR-003 A7):** un actor del sistema en K8, con permisos mínimos y sin poder aprobar nada, para que las reacciones a hechos y los vencimientos corran como tareas.

## Terminado cuando

Aprobado por el líder el 2026-10-27, con las decisiones D1 a D5 del «Plan» como se recomendaban. El líder pidió hacer antes el [[ERP-11 Cambiar solo los datos que se dicen|ERP-11]], para que el IA-9 no espere; este bloque se toma en cuanto el ERP-11 pase a revisión.

- [ ] Un módulo declara sus obligaciones como reglas, y el núcleo calcula sin IA qué vence, cuándo y qué falta para cumplirlo.
- [ ] Con los datos de Mi institución, el programa sabe las cuatro fechas de arriba.
- [ ] Los avisos salen como eventos de la plataforma, no desde una pantalla.

## Plan

**Lo que hay hoy.** Mi institución ya tiene si es donataria (`authorized_donee`), si tiene CLUNI (`cluni`), su clave, la fecha del oficio de donataria y la vigencia del poder del representante legal (`legal_rep_valid_until`). **No tiene la vigencia de la constancia de acreditación de actividades** (D1). El núcleo ya escribe un hecho en la bandeja (`kernel_outbox`) con cada cambio aplicado (ERP-9), y la plataforma ya tiene el canal «evento» (`Channel::Event`), pero nadie lo usa todavía.

1. **Las reglas, en el manifiesto.** Un módulo declara sus obligaciones junto a su catálogo. Cada una lleva:
   - su id (`mi_institucion.informe_cluni`) y su nombre;
   - la entidad sobre la que se calcula;
   - cuándo aplica, con la misma `Condition` del diccionario (por ejemplo, «es donataria»);
   - cuándo vence, de una de tres formas:
     - cada año en una fecha (el 31 de enero);
     - cada año en un periodo (del 1 al 31 de mayo);
     - en la fecha que guarda un campo de la entidad (la vigencia de la constancia);
   - los campos que necesita para cumplirse (la clave CLUNI, por ejemplo).

   El registro y la prueba de conformidad revisan que cada campo nombrado exista en la entidad y que el de la fecha sea una fecha.
2. **El cálculo, en el núcleo (K8).** El núcleo no lee las tablas de un módulo: el módulo le da, con su manifiesto, una función que le pasa los valores de la entidad, como ya hace con `Changeable` al aplicar un cambio. Con eso y con la fecha de hoy, el núcleo calcula, para cada obligación:
   - si aplica;
   - su fecha y el año al que corresponde;
   - su estado: por venir, por vencer (dentro del aviso), vencida, cumplida o sin fecha (falta el dato);
   - qué falta para cumplirla, como campos del diccionario (IA-8).

   Es una función pura de las reglas, los valores y el día. No usa la IA, y el compilador lo impide como hoy.
3. **Cumplida (D3).** La persona marca una obligación como cumplida para su año. Es un cambio como cualquier otro: se prepara y se aplica (ERP-9), con su historial, su origen y su hecho. Lo guarda el núcleo, en una migración suya, porque sirve igual para las obligaciones de cualquier módulo.
4. **Los avisos (D4).** Una revisión del calendario escribe un hecho en la bandeja cuando una obligación entra en el aviso y cuando se vence. Cada aviso se escribe **una sola vez**, aunque la revisión corra muchas veces. El hecho lleva solo ids y códigos: el módulo, la obligación, la entidad y la fecha, nunca un dato de una persona. Para eso, la bandeja acepta hechos que no vienen de un cambio (ninguno la lee todavía). El núcleo da la forma de leer los hechos pendientes y marcarlos entregados, para la plataforma.
5. **El actor del sistema (K2 y K8, A7).** Una cuenta del sistema con un rol propio, que solo recibe los permisos que un módulo le da expresamente; hoy, leer las obligaciones. **Nunca aprueba:** el núcleo rechaza que aplique un cambio con aprobación o que se presente como persona. Es quien corre la revisión del calendario.
6. **Mi institución declara sus obligaciones** (D2), cada una con cuándo aplica:

   | Obligación | Vence | Aplica si |
   |---|---|---|
   | Informe anual de la CLUNI | 31 de enero | tiene CLUNI |
   | Declaración anual | 15 de febrero | es donataria |
   | Declaración de transparencia | del 1 al 31 de mayo | es donataria |
   | Vigencia de la constancia de acreditación de actividades | la fecha del campo nuevo (D1) | es donataria |
   | Vigencia del poder del representante legal | `legal_rep_valid_until` | la fecha está llena |

   Publica dos capacidades:
   - `mi_institucion.obligaciones`, una consulta: su calendario, con el estado y lo que falta de cada una;
   - `mi_institucion.cumplir_obligacion`, una modificación con aprobación.

   Las dos con permisos propios. Dirección los recibe en el núcleo, como los demás, y el actor del sistema solo el de leer.
7. **La app corre la revisión** como el actor del sistema: al desbloquear la base y una vez al día mientras está abierta. No hace nada más con los avisos: convertirlos en una tarea y en un aviso para la persona es de la plataforma (paso 7 del plano de la IA) y de la pantalla.
8. **Verlo sin pantalla.** Un ejemplo de Rust que imprime el calendario de una base de `fixtures/` para la fecha que se le dé (`cargo run --example calendario -- 2027-01-20`), para que el líder lo pruebe en su máquina.
9. **El plano del ERP** lleva su fila en la §13 (K8 con las reglas y la función del módulo, y el actor del sistema en K2). **La prueba de conformidad, la suite completa y el PR.**

**Lo que no entra:**
- correr la fecha al siguiente día hábil cuando cae en fin de semana o día inhábil: necesita los días inhábiles del SAT (K11). Mientras tanto se avisa con la fecha de la ley, que es igual o anterior, así que no se pierde ningún plazo;
- la opinión de cumplimiento, que no tiene una fecha fija;
- la vigencia que trae un documento (K6, Documentos): cuando llegue, la constancia dará su fecha y el campo de D1 se llenará de ahí.

**Las decisiones del líder** (2026-10-27: las cinco, como se recomendaban):

| | Decisión | Se recomienda | Por qué |
|---|---|---|---|
| **D1** | Dónde se guarda la vigencia de la constancia de acreditación de actividades | **Un campo de fecha nuevo en «Datos legales y fiscales»**, que aplica si es donataria, esperado (cuenta en «qué falta») y de clase `internal` | Hoy no existe ese dato, y sin él no se puede calcular la obligación que más pesa (si vence, el SAT puede iniciar la revocación). La investigación dice que su vigencia «comúnmente» es de un año, así que no se puede suponer: se pregunta. Una fecha de vigencia no es un dato sensible. Cuando llegue Documentos (K6), saldrá de la propia constancia. |
| **D2** | Qué obligaciones entran | **Las cuatro del bloque, más la vigencia del poder del representante legal** | El dato ya existe y no cuesta nada más. Un poder vencido impide firmar convenios y declaraciones. Si prefieres solo las cuatro, la quinta se agrega después con una línea. |
| **D3** | Marcar una obligación como cumplida | **Sí: la persona la marca, para su año** | Sin eso, el programa no distingue una declaración presentada de una vencida: seguiría avisando de lo ya hecho y no podría avisar de lo vencido. La alternativa es mostrar solo las fechas, sin estado. |
| **D4** | Cuándo se avisa | **30 días antes, 7 días antes y el día siguiente al vencimiento,** si no está cumplida. Cada aviso, una sola vez | Da tiempo para juntar papeles y un recordatorio final. Se ajusta cambiando dos números. |
| **D5** | Hasta dónde llega este bloque | **Hasta el hecho en la bandeja, listo para la plataforma.** El aviso que ve la persona es del paso 7 del plano de la IA («Reacciones a hechos y avisos») y del riel de diseño | Es lo que dice el tercer criterio: el aviso sale como evento, no desde una pantalla. Consecuencia: en este bloque el aviso no se ve en la app; se prueba con las pruebas y con el ejemplo del paso 8. Si quieres verlo en la app ya, hay que pedir ese bloque al riel IA y una pantalla al de diseño. |

**Con los otros rieles:**
- **Riel IA:** la forma del hecho de un vencimiento (sus códigos y sus campos) y el actor del sistema se le avisan cuando el PR esté en revisión. Su paso 7 los usa.
- **Riel de diseño:** el campo de D1 necesita su etiqueta y su lugar en la vista del perfil. Es un pedido técnico, si el líder aprueba D1.
- **Pedido técnico del IA-9:** va en un bloque propio, el [[ERP-11 Cambiar solo los datos que se dicen|ERP-11]], porque no cabe aquí.

## Cómo probarlo

**Cómo correrlo:**
- Carpeta y rama:
- Antes:
- Arrancar:
- Al terminar:

**Qué hacer y qué debe pasar:**

- [ ] 

## Hallazgos

## Resultado
