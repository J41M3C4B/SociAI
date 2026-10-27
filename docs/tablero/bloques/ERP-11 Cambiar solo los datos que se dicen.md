---
tipo: bloque
id: ERP-11
carril: erp
estado: listo
prioridad: alta
caja: Mi institución; Contratos institucionales
pedido_por: IA-9
acuerdo: tecnico
depende_de: ERP-1
rama:
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
