---
tipo: bloque
id: ERP-11
carril: erp
estado: revision
prioridad: alta
caja: Mi institución; Contratos institucionales
pedido_por: IA-9
acuerdo: tecnico
depende_de: ERP-1
rama: erp/erp-11-cambiar-datos
adr:
creado: 2026-10-27
cerrado:
---

## Qué y por qué

**Pedido técnico del [[IA-9 Llenar desde el chat|IA-9]]** (acordado por mensaje con el riel IA el 2026-10-27). En el chat, la persona escribe «el teléfono de la institución ahora es 55 1234 5678» y recibe una propuesta para aprobar. Solo debe cambiar lo que dijo.

Hoy `mi_institucion.guardar_ventana` vacía los campos de la ventana que no trae, porque la pantalla siempre manda la ventana completa. Para cambiar un solo dato, la plataforma tendría que leer los valores guardados (algunos `confidential`) y reenviarlos, y no debe hacerlo.

Es técnico: cabe en el plano del ERP (§7, preparar y aplicar), no baja ninguna protección, no gasta y no retira nada.

## Terminado cuando

Acordado entre los rieles ERP e IA (§4 del modelo de trabajo). El líder puede deshacerlo al validar.

- [x] Mi institución publica `mi_institucion.cambiar_datos`, una modificación con aprobación, con el permiso `mi_institucion.perfil.escribir`. Sus clases salen del diccionario, y su entrada nombra cada campo (`x-field`).
- [x] Solo toca los campos que trae, de cualquier ventana. Un campo que no viene no se mueve. Un `null` explícito vacía el campo.
- [x] Lo que deja de aplicar por un cambio se vacía, como en `guardar_ventana` (la regla del diccionario), y aparece en la propuesta, para que la persona lo vea antes de aprobar.
- [x] Un campo que no es del perfil no pasa la validación. Una entrada sin campos no propone nada.
- [x] Se prepara y se aplica con el ERP-9, con la misma validación, el mismo historial y el mismo hecho (`mi_institucion.perfil_guardado`) que `guardar_ventana`. Pasa la prueba de conformidad.
- [x] La pantalla y `guardar_ventana` no cambian.

## Plan

El líder pidió hacerlo antes que el ERP-3 (2026-10-27), para que el IA-9 no espere.

1. **La entrada, en el contrato:** `ChangeInput`, los campos que cambia por su id. Un campo ausente no se toca; un `null` lo vacía.
2. **El servicio:** `prepare_change` y `apply_change`, con su propio `Changeable` sobre el núcleo.
   - El borrador es el perfil guardado con lo que trae encima.
   - Un campo que no es del perfil da `unknown_field`.
   - Tiene las mismas reglas propias (las edades y el año de fundación) y la misma propuesta del núcleo que guardar una ventana. Las dos comparten cómo se arma la propuesta.
3. **La capacidad `mi_institucion.cambiar_datos`:**
   - su entrada es tipada: cada campo del perfil con su `x-field`, y el esquema no admite otros;
   - lo indispensable que falta (el nombre, en un perfil nuevo) se pide a la persona con `CapabilityError::needs`, como en `guardar_ventana`;
   - una entrada sin campos es `Invalid`, para que no llegue a la persona una propuesta vacía;
   - `Profile` (la salida de hoy) y la entrada nueva salen de una sola lista de campos, para que no se desfasen. Una prueba revisa que esa lista sea el catálogo.
4. **Las pruebas:**
   - del servicio: solo cambia lo que trae, el `null` vacía, lo que deja de aplicar se vacía y se ve en la propuesta, y el campo desconocido no pasa;
   - de la capacidad, por el registro, sin la plataforma;
   - la prueba de conformidad la revisa sola, y `workspace_tests` la cuenta entre las de Mi institución.
5. **La suite completa y el PR.** Al riel IA se le avisa en cuanto esté en revisión, porque el PR fija el contrato.

## Cómo probarlo

En el PR [#35](https://github.com/J41M3C4B/SociAI/pull/35). `cargo test` (717 pasan, 0 fallan) y `pnpm test` (224) pasan, sin red ni gasto. La capacidad no tiene pantalla: la usará el chat en el IA-9. Por eso se prueba con las pruebas, y en la app solo se revisa que nada cambió.

**Cómo correrlo:**
- Carpeta y rama: `D:\Agente Proyectos\sociai-erp`, rama `erp/erp-11-cambiar-datos`.
- Antes: nada. No hay migraciones; la base no cambia.
- Arrancar: en PowerShell, desde `src-tauri\`, `cargo test` (unos 5 minutos). Después, desde la carpeta, `pnpm tauri dev`.
- Al terminar: cerrar la app.

**Qué hacer y qué debe pasar:**

- [ ] **`cargo test` pasa completo.** Las pruebas nuevas están en `crates/modules/mi_institucion`:
  - `service_tests.rs`: `a_change_touches_only_the_fields_it_names`, `a_null_empties_its_field_and_what_no_longer_applies_shows_in_the_proposal` y `a_change_passes_the_same_validation_and_knows_only_the_fields_of_the_profile`;
  - `capabilities.rs`: la capacidad por el registro, como la corre la plataforma, y que su entrada nombra cada campo del catálogo.
- [ ] **Mi institución funciona igual:** guarda un cambio en cualquier ventana, cierra y vuelve a abrir. El cambio sigue y lo demás no se movió.

## Hallazgos

- **Una entrada sin campos es un error (`no_fields`), no una propuesta vacía:** así a la persona nunca le llega «aprobar nada».
- **Un campo que no es del perfil no cabe en el esquema de la capacidad,** así que la plataforma lo rechaza antes de llegar al módulo. El servicio también lo rechaza (`unknown_field`) si alguien lo llama sin pasar por la capacidad.
- **`Profile` y la entrada nueva salen de una sola lista de campos.** Antes, `Profile` estaba escrito a mano. Agregar un campo al catálogo sin agregarlo a la lista hace fallar una prueba.
- **El texto libre que entra por el chat** lo revisa la plataforma con su escáner (acordado con el riel IA). Desde la pantalla, la app lo sigue revisando antes de llamar al módulo, como en el ERP-1 (D2).

## Resultado
