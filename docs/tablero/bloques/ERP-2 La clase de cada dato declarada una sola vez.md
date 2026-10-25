---
tipo: bloque
id: ERP-2
carril: erp
estado: hecho
prioridad: media
caja: Clasificación de datos (lo declara el ERP); Datos operativos del ERP
pedido_por: IA-3
depende_de: ERP-8
rama: erp/erp-2-diccionario-de-datos
adr:
creado: 2026-10-20
cerrado: 2026-10-24
---

## Qué y por qué

La política de datos (IA-3) necesita saber de qué clase es cada dato, y solo el módulo conoce sus datos. Hoy cada campo trae una sensibilidad, pero hay datos que no la tienen o que se tratan distinto en otro lado. Este bloque hace que **cada dato del ERP declare su clase una sola vez**, con las clases que fije el ADR del IA-3.

**Cambio con el plano del ERP (ERP-7, 2026-10-20).** El lugar único es el **diccionario de datos del núcleo** (K3, [plano del ERP](../../plano-del-erp.md), §5 y paso 4 de la §11). Cada módulo declara ahí sus entidades y sus campos, y de ahí salen la validación, «qué falta» y la clase que usa la política. `AiUse` sale del dato: qué ve la IA es política del IA-3, no una propiedad del campo. Al terminar se pide al riel IA que «falta información de la persona» pueda nombrar campos del diccionario.

**Del [[ERP-10 Lo que pide el plano de la IA al núcleo|ERP-10]] (punto 7, ADR-003 A5):** que los tipos de salida de una capacidad nombren sus campos del diccionario, para que la clase se declare una sola vez y la política filtre campo por campo.

## Terminado cuando

Aprobado por el líder el 2026-10-23, **sin esperar al IA-3:** el diccionario usa las clases provisionales del núcleo (`kernel::CLASSES`, las mismas que `DataClass` del contrato), y el ADR del IA-3 las sustituye sin cambiar la forma del diccionario.

- [x] El núcleo tiene el diccionario: cada módulo declara sus entidades y campos, con su tipo, su regla, cuándo aplica, si es obligatorio y su clase.
- [x] El núcleo valida una entrada y calcula «qué falta» de cualquier entidad con el diccionario, sin código del módulo.
- [x] La prueba de conformidad falla si un campo no tiene clase.
- [x] El pedido al riel IA está en el tablero: [[IA-8 Los campos del diccionario en la plataforma|IA-8]].

## Plan

Todo va en el núcleo (`crates/kernel`), y el módulo de muestra lo usa de punta a punta. El código viejo (`common/forms.rs`) se queda para Mi institución hasta el ERP-1, sin duplicar sus reglas.

1. **El diccionario (K3), en `kernel::dictionary`.**
   - Una `Entity` tiene un id (`note`) y sus campos.
   - Cada `Field` lleva:
     - su id;
     - su tipo: texto, texto largo, número, dinero en centavos (D3), año, fecha, una opción, varias opciones, sí o no, correo o teléfono;
     - sus opciones;
     - su presencia: opcional, esperado (cuenta en «qué falta», pero nunca impide guardar) u obligatorio (sin él no se guarda);
     - cuándo aplica: siempre, si otro campo de la misma entidad está lleno, o si tiene alguno de ciertos valores;
     - sus límites (rango de un número o largo de un texto);
     - su regla (código postal, RFC de una organización, CLABE, nombre de un lugar);
     - **su clase**, por id de `kernel::CLASSES`.
   - Las palabras no van ahí: la etiqueta de un campo sigue en `es-MX.ts`, bajo su id.
   - `AiUse` no pasa al diccionario: qué ve la IA es política (IA-3).
2. **El manifiesto lleva el catálogo del módulo** (`entities`). El registro (K12) lo revisa con lo demás del manifiesto, y un módulo que falla no entra. Las faltas que detecta:
   - un campo sin clase o con una clase que no existe;
   - ids mal formados o repetidos;
   - una opción o varias opciones sin opciones;
   - un rango al revés;
   - una condición que mira un campo que la entidad no tiene.
3. **Lo que el núcleo calcula con el diccionario,** sin código del módulo:
   - `validate`: lo que no pasa, con códigos fijos (`required`, `too_long`, `code_unknown`, `not_a_phone`…). Un valor de un campo que no aplica no se revisa y no se guarda.
   - `missing`: los campos esperados u obligatorios que aplican y están vacíos.
   - Dónde está cada campo, por su id completo (`muestra.note.phone`), y su clase. Con esto la política (IA-3) y la salida de una capacidad (A5) toman la clase del diccionario.
4. **Preparar un cambio usa el diccionario** (ERP-9). `kernel::propose` recibe la entidad del diccionario. De ahí salen la validación y qué campos son protegidos en el historial, y el módulo ya no los repite. **Protegido es una regla del núcleo:** todo campo cuya clase no sea `public` ni `internal`. El módulo puede agregar sus reglas propias (que un nombre no se repita, por ejemplo). La forma de la `Proposal` que aceptó el riel IA no cambia.
5. **Las reglas en un solo lugar.** Las comprobaciones de fecha, código postal, RFC de una organización, CLABE y nombre de un lugar pasan al núcleo. `common/forms.rs` y `common/ids.rs` las llaman de ahí, sin cambiar a quien ya las usa.
6. **El módulo de muestra** declara su nota en el diccionario:
   - el título, obligatorio, de hasta 120 caracteres, de clase `internal`;
   - el texto, de clase `internal`;
   - el teléfono, de clase `personal`.

   Su validación propia y su lista de protegidos salen del servicio.
7. **El pedido al riel IA**, como bloque en su Backlog: que «falta información de la persona» (`NeedsPerson`) nombre campos del diccionario, y que el tipo de salida de una capacidad nombre los suyos (A5). Así la política filtra campo por campo.
8. **La suite completa y el PR.** El plano del ERP lleva su fila en «Ajustes» si algo de esto lo precisa.

## Cómo probarlo

En el PR [#29](https://github.com/J41M3C4B/SociAI/pull/29). `cargo test` (674 pasan, 0 fallan) y `pnpm test` (224) pasan, sin red ni gasto.

**Cómo correrlo:**
- Carpeta y rama: `D:\Agente Proyectos\sociai-erp`, rama `erp/erp-2-diccionario-de-datos`.
- Antes: nada; la base no cambia.
- Arrancar: `pnpm tauri dev` (recompila el núcleo y la app, unos minutos).
- Al terminar: cerrar la app.

**Qué hacer y qué debe pasar:**

- [x] Desde `src-tauri/`, en PowerShell: `cargo test dictionary`. Pasan:
  - las pruebas del diccionario;
  - la del registro que rechaza un catálogo mal hecho;
  - la de la muestra (`the_kernel_knows_the_note_by_its_dictionary`).
- [x] En la app, **Mi institución** valida igual que antes, ahora con las reglas del núcleo:
  - un código postal de 4 dígitos (`0670`) marca error, y `06700` pasa;
  - un RFC de organización mal formado marca error;
  - un municipio con números (`Zapopan 45010`) marca error, y `Benito Juárez` pasa;
  - una CLABE con el dígito verificador mal (`002010077777777772`) marca error, y `002010077777777771` pasa.

## Hallazgos

- **Lo que no aplica se vacía en la propuesta, pero lo escribe el módulo.** `propose` pone en `null` un campo cuya condición no se cumple, y el módulo tiene que escribir lo que dice la propuesta. La muestra no tiene campos con condición, así que hoy no se nota. Mi institución sí tiene campos así (ERP-1).
- **Las condiciones miran solo campos de la misma entidad.** Mi institución tiene formularios cuyas condiciones miran otro formulario (`looks_outside`). Con el ERP-1 se decide si su perfil es una sola entidad o si el diccionario necesita mirar afuera.

## Resultado

Validado por el líder el 2026-10-24: Mi institución valida igual que antes con las reglas del núcleo, y las pruebas del diccionario pasan. Pasó por `rebase` sobre `main` sin conflictos; desde la suite completa (674 pruebas de Rust y 224 del frontend) `main` solo cambió documentos. Fusionado a `main` en el PR [#29](https://github.com/J41M3C4B/SociAI/pull/29) (`ac5f0f2`).

- **K3:** el diccionario vive en `kernel::dictionary`. Cada módulo declara sus entidades en su manifiesto (`entities`), y el registro rechaza un catálogo mal hecho, incluido un campo sin clase.
- **Lo que el núcleo calcula:** `Entity::validate`, `Entity::missing` y `Modules::field` por id completo, con su clase. Protegido es toda clase que no sea `public` ni `internal`, y `kernel::propose` lo toma del diccionario.
- **Las reglas** de fecha, código postal, RFC de una organización, CLABE y nombre de un lugar están en el núcleo; `common/forms.rs` y `common/ids.rs` las llaman de ahí.
- **Lo que sigue:** el [[IA-8 Los campos del diccionario en la plataforma|IA-8]] (PR #32, acuerdo técnico) lleva el diccionario a la plataforma, y el ERP-1 lo usa para Mi institución, con los dos hallazgos de arriba.
