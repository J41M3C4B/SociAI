---
tipo: bloque
id: ERP-1
carril: erp
estado: en-curso
prioridad: alta
caja: Mi institución; Contratos institucionales
pedido_por:
depende_de: IA-1, IA-5, ERP-2, ERP-9, IA-8
rama: erp/erp-1-mi-institucion-modulo
adr:
creado: 2026-10-20
cerrado:
---

## Qué y por qué

Mi institución es el módulo más completo, pero sus operaciones no se pueden descubrir: son comandos sueltos y funciones de Rust. Este bloque publica sus primeras capacidades con el contrato del IA-1:
- las consultas: qué falta por llenar y los datos de una ventana;
- guardar un formulario, como modificación.

Es la primera capacidad real que corre por la plataforma.

**Cambio con el plano del ERP (ERP-7, 2026-10-20).** Mi institución ya no es el núcleo: es el **primer módulo con la forma nueva** ([plano del ERP](../../plano-del-erp.md), §4 y paso 5 de la §11). Este bloque no envuelve el código de `core/` en contratos: rehace el perfil y «qué me falta» como módulo sobre el núcleo (ERP-8, ERP-9 y ERP-2), y retira las piezas viejas que sustituye. Desbloquea el IA-4.

**Del [[IA-8 Los campos del diccionario en la plataforma|IA-8]] (acuerdo técnico con el riel IA, 2026-10-24):** «falta información de la persona» y la salida de cada capacidad nombran campos del diccionario de Mi institución, y la clase de cada dato no se declara en la capacidad.

## Terminado cuando

Aprobado por el líder el 2026-10-24, con las decisiones D1 a D4 del «Plan» como se recomendaban.

- [ ] Mi institución es un módulo con la forma de la §4 del plano y pasa la prueba de conformidad, incluidas sus capacidades.
- [ ] El registro descubre sus capacidades, cada una con su efecto, su permiso y las clases de datos que toca, sacadas del diccionario.
- [ ] «Qué me falta» lo calcula el núcleo con el diccionario de Mi institución, sin código escrito para este módulo.
- [ ] Guardar un formulario se prepara y se aplica (ERP-9), con la validación del diccionario y el historial genérico. Una base de hoy conserva sus datos y su historial.
- [ ] Ninguna de estas capacidades sabe que existe la IA, y el compilador lo impide.
- [ ] Las piezas viejas que esto sustituye salen del código (de las que lista la §10 del plano).
- [ ] La pantalla de Mi institución y el primer inicio funcionan igual que hoy.

## Plan

**Alcance (D1).** El perfil de la institución: las cuatro ventanas de hoy («Su institución», «Contacto y ubicación», «Datos legales y fiscales» y «Capacidad y cifras rápidas»). Se quedan como están, para bloques siguientes:
- el patronato y las cuentas bancarias (las listas protegidas, `core/protected`);
- el expediente legal (`core/records`), que espera a Documentos (K6);
- el primer inicio, que solo pasa a preguntarle al módulo qué falta;
- las líneas anónimas de personal y de población, que calculan módulos congelados;
- la pantalla, que es del riel de diseño.

1. **El módulo `mi_institucion`** en `crates/modules/mi_institucion`, con su `contract` y la forma de la muestra: manifiesto, catálogo, `storage`, migraciones, `service` y `capabilities`. Declara `mi_institucion.perfil.leer` y `mi_institucion.perfil.escribir`; el rol Dirección los recibe en el núcleo, para que pueda lo mismo que hoy.
2. **El catálogo: una sola entidad, `profile`,** con los campos de las cuatro ventanas. Así se resuelve el segundo hallazgo del ERP-2: las condiciones que hoy miran otra ventana miran la misma entidad. Lo de hoy pasa así:
   - `required` es «esperado»: cuenta en «qué falta» y nunca impide guardar, como hoy;
   - la clase: `Public` es `public`, `Internal` es `internal` e `InstitutionalPrivate` es `confidential`;
   - el nombre del representante legal es el dato de una persona: `personal` (D4);
   - `AiUse` y el uso «ai» no pasan;
   - «indispensable» (lo que pide el primer inicio) no es del diccionario: el módulo lo declara aparte, y el primer inicio lo lee de ahí.
3. **Las ventanas son vistas de la entidad:** cada una con sus secciones y sus campos, por id. Las declara el módulo, y la pantalla las dibuja como hoy.
4. **Su tabla, `mi_institucion_profile`:** una fila, con columnas reales y su versión.
5. **Adoptar una base de hoy, una vez, al abrirla.** La app lee el perfil de las tablas viejas y se lo da al módulo, que lo escribe. El historial viejo (`core_change` y `core_field`) pasa al historial del núcleo con su origen. Las columnas viejas se quedan sin uso: no se borra ningún dato. Se prueba con una base armada con el código de hoy y los datos de `fixtures/`.
6. **El servicio:**
   - leer el perfil o una ventana: sus valores, qué falta y el origen de cada dato, sacado del historial del núcleo;
   - guardar una ventana se prepara y se aplica con `kernel::propose` y `kernel::apply`. El borrador es el perfil guardado con los valores de la ventana encima. Se escribe lo que dice la propuesta, incluido el `null` de lo que no aplica (primer hallazgo del ERP-2). Su hecho es `mi_institucion.perfil_guardado`.
7. **Las capacidades** (IA-1 e IA-8):
   - `mi_institucion.que_falta` y `mi_institucion.ver_ventana`, consultas;
   - `mi_institucion.guardar_ventana`, una modificación con aprobación.

   La salida nombra sus campos del diccionario (`x-field`), y lo que falta se pide con `CapabilityError::needs`. Si una ventana, como conjunto de campos, necesita algo de la plataforma, se pide al riel IA como pedido técnico.
8. **La app usa el módulo.**
   - `form_get`, `form_save`, `profile_get`, `profile_save` y el primer inicio llaman a su servicio, y la pantalla recibe lo mismo que hoy.
   - Lo que hoy lee `ProfileInput` (el resumen, los puentes, la hoja de la IA y los ejemplos) lo arma un adaptador desde el módulo, sin reescribir lo que se va a retirar. `core::institution::kind` sale del módulo.
   - Pasar la pantalla al runtime es un pedido al riel de diseño.
   - El escáner sigue revisando el texto libre antes de guardar, desde la app (D2).
9. **Lo viejo que sale:**
   - las cuatro ventanas de `core/institution/forms.rs`;
   - el perfil de `core/profile`: sus versiones, sus formularios y su servicio;
   - `core/history.rs`, para el perfil;
   - el borrador y el perfil confirmado (D3);
   - de `common/forms.rs`, lo que ya nadie use.
10. **La prueba de conformidad** revisa también las capacidades de cada módulo registrado: la muestra y Mi institución.
11. **La suite completa y el PR.** El plano del ERP lleva su fila en «Ajustes» si algo de esto lo precisa.

**Las decisiones del líder** (2026-10-24: las cuatro, como se recomendaban):
- **D1 · Alcance:** solo el perfil, como arriba. Las listas protegidas y el expediente, en bloques propios.
- **D2 · El escáner al guardar:** hoy el texto libre del perfil pasa por el escáner antes de guardarse. El módulo no lo conoce, porque el escáner es de la plataforma de IA (P10). Se recomienda que la app lo siga pasando antes de llamar al módulo, hasta que el riel IA lo lleve a P10. Así no baja la protección.
- **D3 · Borrador y confirmado:** hoy cada confirmación congela una versión del perfil, y antes de terminar el primer inicio el perfil es un borrador. Con el núcleo, cada cambio aplicado ya es una versión con su historial. Se recomienda retirar el borrador y la confirmación: lo guardado vale. Quien hoy lee «confirmado» (la hoja de la IA, la etapa de un proyecto) lee «primer inicio terminado».
- **D4 · El representante legal:** su nombre pasa a la clase `personal`. Sube su protección.

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
