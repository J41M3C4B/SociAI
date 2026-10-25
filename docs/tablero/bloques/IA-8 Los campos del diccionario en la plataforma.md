---
tipo: bloque
id: IA-8
carril: ia
estado: listo
prioridad: media
caja: Entrega de resultados; Política de datos
pedido_por: ERP-2
acuerdo: tecnico
depende_de: ERP-2
rama:
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
