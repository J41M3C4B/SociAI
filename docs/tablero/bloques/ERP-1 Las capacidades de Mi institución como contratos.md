---
tipo: bloque
id: ERP-1
carril: erp
estado: revision
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

- [x] Mi institución es un módulo con la forma de la §4 del plano y pasa la prueba de conformidad, incluidas sus capacidades.
- [x] El registro descubre sus capacidades, cada una con su efecto, su permiso y las clases de datos que toca, sacadas del diccionario.
- [x] «Qué me falta» lo calcula el núcleo con el diccionario de Mi institución, sin código escrito para este módulo.
- [x] Guardar un formulario se prepara y se aplica (ERP-9), con la validación del diccionario y el historial genérico. Una base de hoy conserva sus datos y su historial.
- [x] Ninguna de estas capacidades sabe que existe la IA, y el compilador lo impide.
- [x] Las piezas viejas que esto sustituye salen del código (de las que lista la §10 del plano).
- [x] La pantalla de Mi institución y el primer inicio funcionan igual que hoy.

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

En el PR [#33](https://github.com/J41M3C4B/SociAI/pull/33). `cargo test` (697 pasan, 0 fallan) y `pnpm test` (224) pasan, sin red ni gasto; `tsc --noEmit` pasa.

**Cómo correrlo:**
- Carpeta y rama: `D:\Agente Proyectos\sociai-erp`, rama `erp/erp-1-mi-institucion-modulo`.
- Antes: copia la carpeta de datos `%APPDATA%\mx.sociai.app` a otro lugar. La primera vez que abre, la app pasa el perfil y su historial al módulo; no borra nada, pero así puedes volver atrás.
- Arrancar: `pnpm tauri dev` (recompila el núcleo, el módulo y la app, unos minutos).
- Al terminar: cerrar la app. Si algo salió mal, restaurar la copia de la carpeta.

**Qué hacer y qué debe pasar:**

- [ ] **Una base de hoy conserva su perfil.** En **Mi institución**, las cuatro ventanas («Su institución», «Contacto y ubicación», «Datos legales y fiscales» y «Capacidad y cifras rápidas») muestran los mismos datos que antes, y lo que falta en Inicio es lo mismo.
- [ ] **Cada ventana guarda lo suyo.** Cambia un dato en cada una y guarda. Cierra y vuelve a abrir la app: los cambios siguen y las otras ventanas no se movieron.
- [ ] **Valida como antes:**
  - un código postal de 4 dígitos (`0670`) marca error;
  - un RFC mal formado marca error, y uno en minúsculas se guarda en mayúsculas;
  - un municipio con números (`Zapopan 45010`) marca error;
  - sin el nombre de la institución no se guarda.
- [ ] **Nuevo: el teléfono y el correo de la institución se validan.** Un teléfono con letras o un correo sin `@` marcan error al guardar.
- [ ] **Cambio aceptado (hallazgo 1):** en «Su institución», deja «Cómo atiende» solo en «Comunitario» y guarda. En «Capacidad», los lugares y las personas de hoy ya no aparecen. Si vuelves a «Residencial», se piden de nuevo, vacíos.
- [ ] **El escáner sigue:** escribe un correo de una persona en «A qué se dedica» y guarda. Debe aparecer el aviso de datos personales.
- [ ] **Lo que no cambió sigue igual:** el patronato, las cuentas bancarias y el expediente.
- [ ] **Primer inicio desde cero:** arranca con otra carpeta, en PowerShell: `$env:SOCIAI_DATA_DIR="D:\sociai-prueba"; pnpm tauri dev`. Llena los pasos. Al poner el último dato indispensable, el primer inicio termina y entras a la app.

## Hallazgos

- **Lo que deja de aplicar se borra, venga de la ventana que venga.** Es la regla del diccionario (ERP-2) y estaba en el plan aprobado. Antes, una cifra de «Capacidad» que dejaba de aplicar se guardaba oculta y volvía si la institución regresaba a esa forma de atender; ahora se borra.
- **El teléfono y el correo de la institución se validan al guardar** con las reglas del núcleo. Antes solo daban un aviso.
- **Un guardado que toma datos de un documento deja dos cambios en el historial:** uno por cada documento y página, con el documento como origen, y otro con lo que escribió la persona. El núcleo guarda un origen por cambio. Para eso hace falta una sesión: sin persona, el origen es «de los ejemplos».
- **Marcar una sugerencia como aceptada va después del guardado**, en otra transacción. Si la app se cerrara justo entre los dos, la sugerencia de un campo ya lleno ya no se muestra, así que no se nota.
- **El contacto y la dirección de la institución son `confidential` a propósito.** Así se conserva la regla de hoy (ADR-033 §3: de dónde está, a la IA solo le llegan el municipio y el estado). No sale de la propuesta del IA-3, que pone en `confidential` el RFC, la CLABE y los folios. Consecuencia: con la tabla del IA-3, ningún modelo los ve, y un operador del IA-4 no podría proponerlos desde un acta; las reglas del lector sí. Pasarlos a `internal` bajaría su protección: eso lo decide el líder.
- **El perfil tiene un id fijo (`profile`):** una base es una institución (D4). Si un día hubiera varias, el id sería el de la organización (K1).
- **Las columnas viejas de `institution` y `institution_profile` se quedan con lo que tenían.** Nadie las lee ya para el perfil. La fila de `institution` sigue para el primer inicio y para atar los proyectos, y la de `institution_profile` para las líneas anónimas de personal y población.
- **Proyectos (congelado):** «perfil confirmado» ahora cuenta desde que terminó el primer inicio (D3).
- **Dirección tiene los permisos de todos los módulos** (`Scope::Except` en K2), menos los tres generales que no tenía. Cambió dos pruebas de la plataforma, que son del riel IA. Ya se lo avisé a ese agente.
- **Toqué archivos de otros rieles, con cambios mínimos:**
  - riel IA: `platform_tests.rs` y `ai/dry_run_tests.rs`;
  - riel de diseño: tres mensajes y el nombre de un campo en `es-MX.ts`, y quité dos funciones sin uso de `lib/tauri.ts`.

## Resultado
