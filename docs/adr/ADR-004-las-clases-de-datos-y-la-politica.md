---
adr: 4
estado: propuesta
fecha: 2026-10-25
tema: [ia, plataforma, politica, datos, privacidad, proveedores]
bloque: IA-3
tags: [adr]
---
# ADR-004 · Las clases de datos y la política que les toca

**Estado:** propuesta. El líder aprobó las decisiones C1 a C5 el 2026-10-25, con sus cambios a la C4 y a la C5. El ADR queda aceptado cuando el líder valide el bloque IA-3.

## Contexto

El [ADR-003](ADR-003-la-plataforma-de-ia-por-dentro.md) fija que la política de datos (P10) es **una sola decisión**, sobre la clase del dato, el destino y la finalidad. También fija que se aplica en cuatro puntos: antes del enrutador, al armar el contexto, en el ejecutor y en la entrega. Deja para este ADR cuáles son las clases y qué permite cada una.

El diccionario del núcleo (K3, ERP-2) ya declara la clase de cada campo, con las cinco de `kernel::CLASSES`. El tipo de salida de una capacidad nombra sus campos (A5, IA-8), y K2 dice qué clases ve cada rol.

Hasta hoy, qué ve la IA se decidía en varios lugares:
- la hoja de la IA;
- cada herramienta;
- el escáner y sus «frases seguras»;
- el oficio de cada agente.

Además, nada impedía mandarle datos personales al plan gratuito de Gemini, cuyos términos lo prohíben ([investigación externa](../investigacion/investigacion-externa.md), §4.2).

## Decisión

### 1. Las clases (C1)

Son cinco, las mismas de `kernel::CLASSES`. El ERP declara la de cada campo en su diccionario, y este ADR fija qué permite cada una.

| Clase | Qué es | Ejemplos |
|---|---|---|
| `public` | Lo que la institución publica, o lo que publica otro | El nombre legal, el objeto social, una convocatoria |
| `internal` | De la institución, sin publicar | Las notas, los conteos, los montos de un proyecto |
| `confidential` | Lo propio protegido: identificadores y cuentas de la institución | El RFC, la CLABE, los folios |
| `personal` | De una persona identificable que no es la institución | El nombre, el teléfono o el correo de una beneficiaria, de alguien del personal o del patronato |
| `sensitive_personal` | Lo del art. 2 fr. VI de la LFPDPPP | La salud, el origen étnico, las creencias |

La investigación proponía cuatro (§4.4). `public` e `internal` se separan porque no van a los mismos destinos.

### 2. Los proveedores (C2, C4 y C5)

Un proveedor de modelos es de una de dos formas:

- **Un encargado** (LFPDPPP, art. 2 fr. XII; Reglamento, art. 52): no entrena con lo que recibe, lo borra y lo guarda confidencial.
- **Un proveedor sin condiciones verificadas:** así entra todo proveedor nuevo, hasta que alguien revise sus condiciones.

Lo que importa es qué hace el proveedor con los datos, no si se le paga (C2). Hoy son encargados:
- **Anthropic.** Borra en 30 días y no entrena con la API.
- **Gemini** (C4). En producción solo existen las APIs de pago, que administra el líder, y no se entra al sistema sin pagar. El plan gratuito existe solo en desarrollo: sirve para probar sin gasto con el modelo más básico, y solo con los datos ficticios de `fixtures/`.
- **SubQ** (C5). El líder revisó sus [términos](https://subq.ai/terms), actualizados el 2026-10-16:
  - no usa el contenido del cliente para entrenar (§3), y no hay excepción para los créditos de regalo;
  - guarda las entradas y salidas a lo más 60 días, y ofrece retención cero si se le pide (§4);
  - trata ese contenido como confidencial del cliente (§5);
  - cuando trata datos personales como encargado, lo rige su acuerdo de tratamiento de datos (§4).

### 3. Los destinos y las finalidades

- **Los destinos:**
  - el modelo de un encargado;
  - el modelo de un proveedor sin condiciones verificadas;
  - la pantalla de la persona;
  - un aviso;
  - la auditoría.
- **Las finalidades,** que declara cada operador (A6):
  - `responder`, la de por omisión;
  - `enrutar`: el enrutador solo necesita la intención;
  - `redactar`: un texto donde aparece una persona, que necesita al personaje pero no sus datos.

### 4. La tabla

| Clase | Modelo de un encargado | Modelo sin condiciones verificadas | Aviso |
|---|---|---|---|
| `public` | Va | Va | Va |
| `internal` | Va | No va (C3) | Va |
| `confidential` | No va: solo lo leen las reglas | No va | No va |
| `personal` | No va. Va tapado si la finalidad es `enrutar` o `redactar`, o como agregado | No va | No va |
| `sensitive_personal` | Solo como agregado | No va | No va |

### 5. Las reglas que acompañan la tabla

1. **La pantalla:** un dato va si el rol de la persona ve su clase (K2).
2. **Un agregado de 3 o más se trata como `internal`** («cuántos, nunca quiénes»). Ya está disociado (art. 9 fr. III). Uno de menos de 3 conserva su clase.
3. **Un dato de la salida que no nombra su campo** se decide con la más estricta de las clases del `data` de su capacidad: va solo si todas lo dejan ir.
4. **Lo tapado solo vive en memoria,** mientras dura el paso. Se restaura solo hacia la persona, nunca en un aviso ni en la auditoría.
5. **La auditoría nunca lleva valores,** de ninguna clase: solo ids y códigos.
6. **El texto que escribe la persona no trae clase:** lo clasifica el detector local, y cada cosa que encuentra toma la suya.
   - Un nombre, un teléfono, un correo, la CURP o el RFC de una persona son `personal`.
   - La salud es `sensitive_personal`.
   - El RFC de una organización y la CLABE son `confidential`.

   Lo que el detector no encuentra va como texto de la persona. El detector es la segunda capa, detrás de la clase declarada: encuentra datos, no decide.
7. **Un aviso dice qué pasó;** el detalle se ve al abrir la tarea.
8. **Ninguna regla de qué ve la IA se escribe en un *prompt*,** en una herramienta ni en una expresión regular de un agente.

### 6. Dónde se aplica

Es la §9 del [plano de la IA](../plano-de-la-ia.md), con esta tabla:

| Punto | Qué decide | Si no pasa |
|---|---|---|
| **Previa** | El texto del chat, por lo que encuentra el detector, con la finalidad `enrutar` y el proveedor activo | La tarea termina con `not_for_this_provider` y sugiere la pantalla |
| **Contexto** | Lo que recibe un modelo. Solo lo arma la política (`ToModel`), dato por dato | El dato no entra, o entra tapado |
| **Ejecución** | Que la cuenta vea cada clase del `data` de la capacidad | Se deniega con `class_not_seen` |
| **Salida** | Cada campo de la salida, según su destino | Se quita, y la salida dice qué se quitó (`withheld`) |

## Lo que este ADR no decide

- **La clase de cada campo:** la declara el ERP en su diccionario (K3).
- **Qué clases ve cada rol:** es de K2.
- **Cómo se dibuja lo que no se muestra:** es del riel de diseño.
- **El aviso de privacidad:** los proveedores piden lo de siempre, que diga qué hacemos con los datos, incluido que un proveedor de IA los trata como encargado. Es del líder.

## Consecuencias

- **La política vive en la plataforma** (`platform::policy`), como una tabla de datos que se prueba completa sin modelo.
- **El detector y la bóveda se mudan a un *crate* propio,** para que la plataforma los use sin depender de la app. El escáner deja de decidir: sus «frases seguras» no entran a la plataforma. La lectura de convocatorias, detenida, conserva su configuración.
- **Ningún modelo recibe otra cosa que un `ToModel`.** La llamada al modelo que traiga el IA-4 no compila con otra cosa.
- **Para SubQ:** cuando entre al código como proveedor, se declara encargado y se pide su retención cero.
- **En las pantallas no cambia nada hoy:** los dos roles ven todas las clases.

## Alternativas descartadas

- **«Pagado o gratuito» como destino:** un plan pagado sin las condiciones de un encargado no basta. Además, el plan gratuito no es una forma de operar del sistema.
- **Las cuatro clases de la investigación:** juntar `public` e `internal` dejaría pasar lo interno a un proveedor sin condiciones verificadas.
- **Que el detector decida,** como el escáner de hoy con sus «frases seguras»: la clase declarada es la primera capa, y el detector tiene fallas y falsas alarmas que se miden.
- **Tapar todo lo personal siempre:** seudonimizar daña el resumen más que la clasificación (investigación, §4.3). Se tapa solo donde la tarea necesita al personaje.
