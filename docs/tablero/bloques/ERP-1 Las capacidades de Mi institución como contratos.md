---
tipo: bloque
id: ERP-1
carril: erp
estado: backlog
prioridad: alta
caja: Mi institución; Contratos institucionales
pedido_por:
depende_de: IA-1, IA-5, ERP-2, ERP-9, IA-8
rama:
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

(Propuesta; la aprueba el líder.)

- [ ] Mi institución es un módulo con la forma de la §4 del plano y pasa la prueba de conformidad, incluidas sus capacidades.
- [ ] El registro descubre sus capacidades, cada una con su efecto, su permiso y las clases de datos que toca, sacadas del diccionario.
- [ ] «Qué me falta» lo calcula el núcleo con el diccionario de Mi institución, sin código escrito para este módulo.
- [ ] Guardar un formulario se prepara y se aplica (ERP-9), con la validación del diccionario y el historial genérico. Una base de hoy conserva sus datos y su historial.
- [ ] Ninguna de estas capacidades sabe que existe la IA, y el compilador lo impide.
- [ ] Las piezas viejas que esto sustituye salen del código (de las que lista la §10 del plano).

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
