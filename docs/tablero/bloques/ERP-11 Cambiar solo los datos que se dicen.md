---
tipo: bloque
id: ERP-11
carril: erp
estado: en-curso
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

- [ ] Mi institución publica `mi_institucion.cambiar_datos`, una modificación con aprobación, con el permiso `mi_institucion.perfil.escribir`. Sus clases salen del diccionario, y su entrada nombra cada campo (`x-field`).
- [ ] Solo toca los campos que trae, de cualquier ventana. Un campo que no viene no se mueve. Un `null` explícito vacía el campo.
- [ ] Lo que deja de aplicar por un cambio se vacía, como en `guardar_ventana` (la regla del diccionario), y aparece en la propuesta, para que la persona lo vea antes de aprobar.
- [ ] Un campo que no es del perfil no pasa la validación. Una entrada sin campos no propone nada.
- [ ] Se prepara y se aplica con el ERP-9, con la misma validación, el mismo historial y el mismo hecho (`mi_institucion.perfil_guardado`) que `guardar_ventana`. Pasa la prueba de conformidad.
- [ ] La pantalla y `guardar_ventana` no cambian.

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

**Cómo correrlo:**
- Carpeta y rama:
- Antes:
- Arrancar:
- Al terminar:

**Qué hacer y qué debe pasar:**

- [ ] 

## Hallazgos

## Resultado
