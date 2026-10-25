---
tipo: bloque
id: IA-8
carril: ia
estado: en-curso
prioridad: media
caja: Entrega de resultados; Política de datos
pedido_por: ERP-2
acuerdo: tecnico
depende_de: ERP-2
rama: ia/ia-8-campos-del-diccionario
adr:
creado: 2026-10-23
cerrado:
---

## Qué y por qué

Con el [[ERP-2 La clase de cada dato declarada una sola vez|ERP-2]], el núcleo tiene el diccionario de datos (K3 del [plano del ERP](../../plano-del-erp.md)). Cada módulo declara ahí sus entidades y sus campos, con su tipo, sus opciones, si es esperado u obligatorio, cuándo aplica y **su clase**. Un campo se nombra por su id completo (`muestra.note.phone`), y el núcleo lo encuentra con `Modules::field`. El núcleo también calcula qué falta de cualquier entidad (`Entity::missing`).

El riel ERP necesita dos cosas de la plataforma, que el [plano de la IA](../../plano-de-la-ia.md) ya acordó:

1. **Que «falta información de la persona» nombre campos del diccionario** (punto 9 de la §11 y la aclaración de la §6 del plano de la IA). Así el chat le pregunta a la persona con las opciones del campo, y no con texto libre.
2. **Que el tipo de salida de una capacidad nombre sus campos del diccionario** (A5, punto 5). Así la clase de un dato se declara una sola vez, en el diccionario, y la política (IA-3) filtra campo por campo. El `data` del contrato sigue siendo el tope.

Con eso, el ERP-1 publica las capacidades de Mi institución sin declarar la clase de sus datos dos veces.

## Terminado cuando

(Pedido técnico, acordado entre los rieles IA y ERP el 2026-10-24; ver §4 del modelo de trabajo.)

- [ ] Una capacidad puede decir «falta información de la persona» nombrando uno o más campos del diccionario por su id completo (`muestra.note.phone`), en vez de un texto libre.
- [ ] La aclaración que sale de la entrega trae, de cada campo, su tipo, sus opciones y su presencia, tomados del diccionario. El texto libre es opcional.
- [ ] La respuesta de la persona pasa por la validación del núcleo antes de que la tarea siga. Si no pasa, la aclaración vuelve como corrección del campo.
- [ ] El tipo de salida de una capacidad nombra sus campos del diccionario, y la clase de cada uno sale de ahí, sin declararla en la capacidad.
- [ ] Al arrancar, la plataforma rechaza una capacidad que nombra un campo que no existe, o cuya salida trae una clase que no está en su `data`, el tope.
- [ ] La plataforma deja la clase de cada campo de la salida lista para la política. El filtrado campo por campo es del IA-3.
- [ ] Con el módulo de muestra: una capacidad dice que falta `muestra.note.phone`, y su salida nombra campos `muestra.note.*` sin declarar su clase. `cargo test` lo prueba sin modelo.

## Plan

La rama nace de IA-7 (en revisión, PR #30) con los dos commits del ERP-2 (PR #29) encima, porque IA-8 toca los mismos archivos que IA-7. Cuando los dos se fusionen, se hace `rebase` sobre `main` y el PR queda solo con IA-8.

1. **«Falta información» con campos** (`capability`): `CapabilityError::NeedsPerson` nombra la entidad por su id (`muestra.note`) y los campos que faltan, tal como los da `Entity::missing`. Sin texto libre: las palabras las pone el operador del IA-4. El contrato sigue sin depender del núcleo.
2. **La aclaración** (`platform`): `Ask::Missing` lleva, de cada campo, su id completo, su descripción del diccionario (el `kernel::Field` en JSON: tipo, opciones, presencia, condición, límites) y, si la respuesta anterior no pasó, el código del núcleo. Si una capacidad pide un campo que no existe, o uno cuya clase no está en su `data`, la tarea se cierra con `bad_output`.
3. **La respuesta** (`platform`): llega con los ids completos de lo que se pidió; lo que no se pidió se ignora. El núcleo la valida con `Entity::validate`, con el resto de la entrada como contexto (las condiciones). Si no pasa, la aclaración vuelve con el código en cada campo y cuenta como corrección. Si pasa, se une a la entrada y la capacidad corre otra vez.
4. **La salida nombra sus campos**: cada propiedad del tipo de salida dice su campo con `#[schemars(extend("x-field" = "muestra.note.title"))]`. La plataforma recorre el esquema y saca la ruta, el campo y su clase, que salen del diccionario (`output_fields`). Lo usa la revisión al arrancar, y después la política del IA-3.
5. **Al arrancar** (`src/lib.rs`): la app no arranca con una capacidad cuya salida nombre un campo que no existe (`Fault::UnknownField`), o cuya clase no esté en su `data` (`Fault::ClassOutsideData`).
6. **El módulo de muestra**: la salida de `muestra.leer_nota` nombra sus campos, y una consulta nueva, `muestra.contacto_de_nota`, pide `muestra.note.phone` cuando la nota no lo tiene.
7. **Las pruebas**: el módulo de prueba de la plataforma declara su parte del diccionario. La lectura vieja de `NeedsPerson { what }` pasa a campos. `src/lib/types.ts` cambia `OutcomeAsk`.

Sin ADR: lo decidieron A5 y la §6 del plano de la IA.

## Cómo probarlo

**Cómo correrlo:**
- Carpeta y rama:
- Antes:
- Arrancar:
- Al terminar:

**Qué hacer y qué debe pasar:**

- [ ] 

## Hallazgos

- Cómo se dibuja en el chat la aclaración con las opciones del campo es del riel de diseño. IA-8 solo la entrega en el contrato común de resultados (§6 del plano de la IA).

## Resultado
