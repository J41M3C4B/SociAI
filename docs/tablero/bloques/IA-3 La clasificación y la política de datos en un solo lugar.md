---
tipo: bloque
id: IA-3
carril: ia
estado: backlog
prioridad: media
caja: Clasificación de datos; Políticas de datos y PII
pedido_por:
depende_de: IA-2, IA-8
rama:
adr:
creado: 2026-10-20
cerrado:
---

## Qué y por qué

Hoy qué ve la IA se decide en varios lugares: la hoja de la IA, cada herramienta, el escáner y el oficio de cada agente. Este bloque define **las clases de datos** y **la política** que les toca, en el plano de control y una sola vez:
- la política previa al enrutador;
- el contexto mínimo;
- los controles de salida.

Incluye la regla del proveedor: los términos del plan gratuito de Gemini prohíben mandarle datos personales o confidenciales ([investigación externa](../../investigacion/investigacion-externa.md), §4.2), y hoy nada lo impide.

**SubQ como proveedor candidato (nota del líder, 2026-10-21).** El líder tiene una suscripción a SubQ (Subquadratic): un contexto de hasta 12 millones de tokens, con atención dispersa en vez de partir los documentos y recuperarlos por *embeddings*. Ofrece dos modelos, uno de decisión y uno más general, y el líder tiene 100 millones de tokens de regalo. Un documento de una institución no llega ni al millón de tokens, así que alcanzan para probar si sirve para el conocimiento documental (P6, [plano de la IA](../../plano-de-la-ia.md)). Un caso que encaja: las convocatorias, que son públicas y largas.

- **Se hará una investigación especializada** antes de decidir.
- **Lo que este bloque tiene que resolver para SubQ:**
  - sus condiciones (el contrato de API y el acuerdo de tratamiento de datos, que no son públicos): si entrena con lo que recibe, cuánto lo guarda, si lo borra y su confidencialidad (LFPDPPP, art. 52);
  - si los tokens de regalo cuentan como servicio pagado o como gratuito;
  - si vencen.

  Mientras no se sepa, la política solo le deja mandar lo público.
- **La comparación de calidad** (frente a la recuperación por fragmentos) va con la batería, cuando exista el IA-4, con una corrida real corta.

## Terminado cuando

(Propuesta; la aprueba el líder junto con las decisiones C1 a C5 de abajo.)

- [ ] **El ADR-004 fija las clases y su tabla:** las cinco clases que ya usa el diccionario, los destinos, las finalidades y qué permite cada combinación. El líder lo aprueba.
- [ ] **La política es una sola decisión, en la plataforma** (`platform::policy`). Dada la clase, el destino y la finalidad, dice si el dato va, va tapado, va solo como agregado o no va. Es una tabla de datos, y una prueba la recorre completa sin modelo.
- [ ] **Cada proveedor declara si es un encargado**, y la política lo lee. Ser encargado quiere decir que no entrena con lo que recibe, lo borra y lo guarda confidencial (LFPDPPP; Reglamento, art. 52).
  - Anthropic sí es encargado.
  - Gemini solo si la institución declaró que su proyecto tiene facturación.
  - SubQ no, mientras sus condiciones no se verifiquen.
- [ ] **Se aplica en los cuatro puntos del runtime:**
  - **Previa:** el texto del chat pasa por el detector local antes de ir a un modelo. Con un proveedor que no es encargado, un texto con datos de personas no sale.
  - **Contexto:** lo que recibe un modelo solo lo arma la política (`ToModel`). El IA-4 no puede llamar a un modelo con otra cosa: no compila.
  - **Ejecución:** se deniega una capacidad que toca clases que la cuenta no ve (K2).
  - **Salida:** la salida de una capacidad se filtra campo por campo, según su destino. Lo tapado se restaura solo hacia la persona.
- [ ] **Con un proveedor que no es encargado, no sale ningún dato interno, confidencial, personal ni sensible.** Hay pruebas en cada punto.
- [ ] **Ninguna regla de qué ve la IA queda en un *prompt*.** El detector (el escáner de hoy) solo encuentra datos; la política decide.
- [ ] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## La propuesta

### Las clases

Son las cinco que ya usa el diccionario del [[ERP-2 La clase de cada dato declarada una sola vez|ERP-2]]. El ERP declara la de cada campo; este bloque fija qué permite cada una.

| Clase | Qué es | Ejemplos |
|---|---|---|
| `public` | Lo que la institución publica, o lo que publica otro | El nombre legal, el objeto social, una convocatoria |
| `internal` | De la institución, sin publicar | Las notas, los conteos, los montos de un proyecto |
| `confidential` | Lo propio protegido: identificadores y cuentas de la institución | El RFC, la CLABE, los folios |
| `personal` | De una persona identificable que no es la institución | El nombre, el teléfono o el correo de una beneficiaria, de alguien del personal o del patronato |
| `sensitive_personal` | Lo del art. 2 fr. VI de la LFPDPPP | La salud, el origen étnico, las creencias |

### Los destinos y las finalidades

- **Los destinos:**
  - el modelo de un encargado;
  - el modelo de un proveedor que no es encargado;
  - la pantalla de la persona;
  - un aviso;
  - la auditoría.
- **Las finalidades,** que declara cada operador (A6):
  - `responder`, la de por omisión;
  - `enrutar`: el enrutador solo necesita la intención;
  - `redactar`: un texto donde aparece una persona, que necesita al personaje pero no sus datos.

### La tabla

| Clase | Modelo de un encargado | Modelo que no es encargado | Aviso |
|---|---|---|---|
| `public` | Va | Va | Va |
| `internal` | Va | **No va** | Va |
| `confidential` | No va: solo lo leen las reglas | No va | No va |
| `personal` | No va. Va tapado si la finalidad es `enrutar` o `redactar`, o como agregado | No va | No va |
| `sensitive_personal` | Solo como agregado | No va | No va |

**Las reglas que acompañan la tabla:**
1. **La pantalla:** un dato va si el rol de la persona ve su clase (K2). Hoy los dos roles ven todas, así que en las pantallas no cambia nada.
2. **Un agregado de 3 o más se trata como `internal`** («cuántos, nunca quiénes»). Ya está disociado (art. 9 fr. III). Uno de menos de 3 conserva su clase.
3. **Un dato de la salida que no nombra su campo** toma la clase más alta del `data` de su capacidad.
4. **Lo tapado solo vive en memoria,** mientras dura el paso. Se restaura solo hacia la persona, nunca en un aviso ni en la auditoría.
5. **La auditoría nunca lleva valores,** de ninguna clase, como hoy: solo ids y códigos.
6. **El texto que escribe la persona no trae clase:** lo clasifica el detector, y cada cosa que encuentra toma la suya.
   - Un nombre o un teléfono son `personal`.
   - La salud es `sensitive_personal`.
   - El RFC de una organización es `confidential`.

   Lo que el detector no encuentra va como texto de la persona.
7. **Un aviso dice qué pasó;** el detalle se ve al abrir la tarea.

### Las decisiones del líder

| | Decisión | Recomiendo | Por qué |
|---|---|---|---|
| **C1** | Cinco clases: las que ya usa el diccionario | Sí | La investigación proponía cuatro. `public` e `internal` se separan porque un proveedor que no es encargado solo puede recibir lo público. |
| **C2** | El destino es «encargado o no», en vez de «pagado o gratuito» | Sí | Lo que importa es qué hace el proveedor con los datos (art. 52), no si se le paga. Así SubQ cabe sin saber si sus tokens de regalo cuentan como pago. |
| **C3** | Lo interno no va a un proveedor que no es encargado | Sí | Google usa lo que recibe en el plan gratuito para entrenar, lo leen personas revisoras, y sus términos prohíben lo «confidential». **Consecuencia:** con el plan gratuito, la IA solo trabaja con lo público. Las corridas reales del IA-4 con Mi institución necesitan Anthropic, o Gemini con facturación. |
| **C4** | Gemini cuenta como encargado solo si la institución declara que su proyecto tiene facturación. Por omisión, no | Sí | La llave no dice si el proyecto paga. La casilla para declararlo en Ajustes es del riel de diseño; mientras no exista, Gemini cuenta como «no encargado». |
| **C5** | SubQ cuenta como «no encargado» hasta verificar sus condiciones | Sí | Su contrato de API y su acuerdo de datos no son públicos. Si nos los compartes, se verifican en un bloque aparte, y basta cambiar su declaración. La comparación de calidad sigue con la batería del IA-4. |

## Plan

1. **El ADR-004**, con la tabla aprobada:
   - su fila en `docs/adr/README.md`;
   - la §9 del [plano de la IA](../../plano-de-la-ia.md) remite a él, con su fila en la §13.
2. **El detector y la bóveda, a un *crate* propio** (`privacy`): el escáner de hoy (`scanner/`) y la bóveda (`ai/vault.rs`).
   - **Por qué:** la previa y lo tapado viven en la plataforma, y la plataforma no puede depender de la app.
   - **Lo que no se mueve:** `scanner/guard.rs`, la revisión de los formularios, se queda en la app, porque escribe en la auditoría de la app.
   - **Para el resto de la app no cambia nada:** `crate::scanner` vuelve a exportar el *crate*, así que el código del ERP no cambia, y lo viejo de `ai/` toma la bóveda de ahí.
   - **Las «frases seguras»:** el detector de la plataforma no las usa. La lectura de convocatorias, detenida, conserva su configuración como está.
3. **La decisión** (`platform::policy`):
   - `Class`, `Destination`, `Purpose` y `Verdict`;
   - `decide(clase, destino, finalidad)`, como una tabla constante;
   - las reglas 2 y 3.

   Una prueba recorre la tabla completa.
4. **Los proveedores declaran si son encargados.** El `Runtime` recibe el proveedor activo, con su id y si es encargado, o ninguno.
   - La app lo arma con la configuración de la IA: Anthropic es encargado; Gemini, solo si `gemini_billing` está declarado.
   - `gemini_billing` es una clave nueva de `app_settings`, apagada por omisión.
5. **Ejecución:** antes de invocar, el ejecutor revisa que cada clase del `data` de la capacidad esté en `kernel::classes_seen` de la cuenta. Si no, deniega con `class_not_seen`, y queda en la auditoría.
6. **Salida:** antes de entregar `Body::Data`, la plataforma filtra la salida campo por campo (`output_fields`, del IA-8) según su destino.
   - `Data` gana `withheld`: las rutas que se quitaron, para que la pantalla pueda decir que hay algo que no se muestra.
   - Lo tapado se restaura antes de entregar a la persona.
7. **Contexto:** `ToModel` solo se construye con `policy::to_model(proveedor, finalidad, datos)`.
   - **Lo que recibe:** los datos llegan con su campo y su clase, o como texto de la persona, o como agregado con su conteo.
   - **Cómo tapa:** usa la bóveda, con marcas consistentes.
   - **Para el IA-4:** la llamada al modelo que traiga solo aceptará un `ToModel`.
8. **Previa:** el texto del chat pasa por el detector y por la decisión con la finalidad `enrutar`, antes del `without_ai` de hoy, que el IA-4 cambia por el enrutador.
   - Si algo no va: la tarea termina con `not_for_this_provider` y sugiere la pantalla.
   - Si va: queda el `ToModel` tapado que recibirá el enrutador.
9. **Las pruebas:**
   - la tabla completa;
   - cada punto con un proveedor encargado, con uno que no lo es y sin proveedor;
   - lo tapado restaurado solo hacia la persona;
   - un agregado de menos de 3.

   Además, `src/lib/types.ts` gana `withheld` y los códigos nuevos.
10. **La suite completa y el PR.**

**Lo que toca de otros rieles:**
- **ERP:**
  - Sus pantallas usan el escáner por `crate::scanner`, y su código no cambia.
  - El comentario de `kernel::CLASSES` («las definitivas vienen con el ADR del IA-3») pasa a nombrar el ADR-004. Es un pedido técnico.
  - Si un rol llega a ver menos clases, eso es de K2.
- **Diseño:**
  - dibujar `withheld` en la pantalla;
  - la casilla de facturación de Gemini en Ajustes;
  - los textos de `class_not_seen` y `not_for_this_provider`.

  Hasta entonces, la pantalla muestra lo que sí llega, y Gemini cuenta como «no encargado».

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
