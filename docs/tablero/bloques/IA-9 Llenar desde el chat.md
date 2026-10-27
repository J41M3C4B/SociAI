---
tipo: bloque
id: IA-9
carril: ia
estado: backlog
prioridad: alta
caja: Operadores de IA; Políticas de datos y PII; Confirmaciones humanas
pedido_por:
acuerdo:
depende_de: IA-4
rama:
adr: ADR-005
creado: 2026-10-27
cerrado:
---

## Qué y por qué

**Decisión del líder (2026-10-26 y 2026-10-27, E2 del [[IA-4 El enrutador y el primer operador|IA-4]]):** llenar con la IA no es un complemento. Por el principio de quitar o disminuir la fricción para personas no técnicas, la persona tiene que poder escribir en el chat «el teléfono de la institución ahora es…» y recibir el cambio listo para aprobar. Hay que pensar bien cómo. Por eso va en su propio bloque, justo detrás del IA-4, que deja el enrutador, la puerta al modelo y el turno sin congelar la app.

Hoy, con la tabla del ADR-004, «mi RFC es…» detiene el chat (hallazgo del IA-3), y un teléfono escrito en el chat solo puede ir tapado para enrutar. Llenar necesita una regla nueva de la política: el **ADR-005** (E5).

## Terminado cuando

(Propuesta del riel IA, 2026-10-27. La aprueba el líder junto con la E5; entonces pasa a «Listo».)

- [ ] **«El teléfono de la institución ahora es 55 1234 5678», escrita en el chat, termina en una propuesta para aprobar:**
  - el modelo nunca ve el número, solo su marca (`[TELEFONO_1]`);
  - la plataforma pone el valor en la entrada del cambio, y la propuesta lo muestra con su origen: «lo escribió usted»;
  - solo cambia lo que la persona dijo, y el resto de la ventana queda igual;
  - se aplica solo cuando la persona aprueba, con la tarea como origen (K5).
- [ ] **Ningún valor propuesto lo inventa el modelo.** Cada valor es una de tres cosas:
  - una marca;
  - un texto que está en lo que escribió la persona;
  - una opción de la lista del campo.

  Un valor que no pasa esa revisión no se propone: se le pregunta a la persona como campo del diccionario (IA-8).
- [ ] **Desde el chat, todo cambio pide aprobación,** aunque su capacidad no la pida desde una pantalla.
- [ ] **El operador `ia.llenar` es de la plataforma** (A6), con su *prompt* versionado, el esquema de su respuesta y su nivel de modelo. Solo recibe los campos de la entrada del cambio, sin los valores guardados.
- [ ] **El ADR-005 fija cómo se llena desde el chat** (E5). El líder lo aprueba.
- [ ] **La batería del IA-4 gana la categoría «llenar»:** un valor, varios, uno tapado, uno que el modelo inventa y uno sensible que no va. La corrida real la incluye.
- [ ] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## La decisión para el líder: E5, cómo se llena desde el chat (ADR-005)

**Es un cambio a la tabla, y por eso va en un ADR nuevo.** El ADR-004 no se edita.

1. **Una finalidad nueva, `llenar`:** lo que la persona escribe para guardarlo en su propio ERP.
2. **Lo que la persona escribe en el chat** va tapado si es `personal` o `confidential`, para `enrutar` y para `llenar`, a un encargado.
   - El modelo ve `[RFC_1]` o `[TELEFONO_1]`, nunca el valor.
   - `sensitive_personal` sigue sin ir.
   - Con un proveedor sin condiciones verificadas, nada cambia: solo va lo público.
   - **Los datos guardados no cambian de regla:** un campo `confidential` del ERP sigue sin ir a ningún modelo. La regla nueva es solo para lo que la persona acaba de escribir.
3. **Lo tapado vuelve a la entrada de una capacidad solo en un cambio con aprobación,** y la propuesta le muestra el valor a la persona antes de aprobar. Nunca vuelve en una consulta, un aviso ni la auditoría.
4. **Ningún valor propuesto viene del modelo:** cada uno es una marca, un texto que está en lo que escribió la persona o una opción de la lista del campo. La plataforma lo revisa por código, como las cifras.

**Por qué:** la persona escribe su dato una vez y lo ve en la propuesta, el modelo solo decide en qué campo va, y nada se guarda sin su aprobación. La protección de lo guardado no baja.

## Plan

(Borrador; se completa al tomar el bloque.)

1. **El ADR-005**, con su fila en `docs/adr/README.md` y en la §9 y la §13 del plano de la IA. En la política: la finalidad `Purpose::Fill`, la regla del texto de la persona y `ToModel::restore_into`, que solo acepta la entrada de un cambio con aprobación.
2. **El operador `ia.llenar`:** recibe el texto tapado y los campos de la entrada del cambio elegido, cada uno con su id, su tipo y sus opciones. Devuelve los valores por campo.
3. **El turno de llenar:**
   1. el enrutador del IA-4 elige un cambio;
   2. `ia.llenar` da los valores;
   3. la plataforma revisa cada valor, y lo que no pasa se pregunta con `Ask::Missing`;
   4. las marcas vuelven a su valor dentro de la entrada;
   5. `prepare` da la propuesta, con evidencia `Person`, y la tarea espera la aprobación;
   6. con la aprobación se aplica con la huella (IA-7).
4. **Cambiar solo lo que se dijo.** Hoy guardar una ventana de Mi institución vacía los campos que no trae. Para llenar un dato, la plataforma tendría que leer y mandar los demás, y no debe. **Pedido técnico al riel ERP:** que Mi institución publique un cambio que solo toque los campos que trae. Se acuerda por mensaje mientras se hace el IA-4, para que llegue listo.
5. **La batería y la corrida real** ganan los casos de llenar.

**Lo que no entra:** llenar desde un documento (un acta, una constancia). Espera a que los documentos sean capacidades del núcleo (K6). El camino será el mismo operador, con el documento en vez del texto.

**Lo que toca de otros rieles:**
- **ERP:** el cambio que solo toca los campos que trae (paso 4).
- **Diseño:** que el chat dibuje la `Propuesta` con su aprobación y el origen de cada valor.

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
