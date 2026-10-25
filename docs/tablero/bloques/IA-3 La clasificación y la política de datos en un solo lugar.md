---
tipo: bloque
id: IA-3
carril: ia
estado: revision
prioridad: media
caja: Clasificación de datos; Políticas de datos y PII
pedido_por:
depende_de: IA-2, IA-8
rama: ia/ia-3-clases-y-politica
adr: ADR-004
creado: 2026-10-20
cerrado:
---

## Qué y por qué

Hoy qué ve la IA se decide en varios lugares: la hoja de la IA, cada herramienta, el escáner y el oficio de cada agente. Este bloque define **las clases de datos** y **la política** que les toca, en el plano de control y una sola vez:
- la política previa al enrutador;
- el contexto mínimo;
- los controles de salida.

Incluye la regla del proveedor: los términos del plan gratuito de Gemini prohíben mandarle datos personales o confidenciales ([investigación externa](../../investigacion/investigacion-externa.md), §4.2), y hoy nada lo impide.

**Decisión del líder (2026-10-25): el plan gratuito de Gemini es solo para desarrollo.** Sirve para probar sin gasto, con el modelo más básico y con los datos ficticios de `fixtures/`; las pruebas y los flujos reales se hacen con esos datos. En producción ese plan no existe: el líder administra las APIs de pago, y no se entra al sistema sin pagar.

**SubQ como proveedor candidato (nota del líder, 2026-10-21).** El líder tiene una suscripción a SubQ (Subquadratic): un contexto de hasta 12 millones de tokens, con atención dispersa en vez de partir los documentos y recuperarlos por *embeddings*. Ofrece dos modelos, uno de decisión y uno más general, y el líder tiene 100 millones de tokens de regalo. Un documento de una institución no llega ni al millón de tokens, así que alcanzan para probar si sirve para el conocimiento documental (P6, [plano de la IA](../../plano-de-la-ia.md)). Un caso que encaja: las convocatorias, que son públicas y largas.

- **Se hará una investigación especializada** antes de decidir.
- **Lo que este bloque tiene que resolver para SubQ:**
  - sus condiciones (el contrato de API y el acuerdo de tratamiento de datos, que no son públicos): si entrena con lo que recibe, cuánto lo guarda, si lo borra y su confidencialidad (LFPDPPP, art. 52);
  - si los tokens de regalo cuentan como servicio pagado o como gratuito;
  - si vencen.

  Mientras no se sepa, la política solo le deja mandar lo público.

  **Resuelto (2026-10-25):** el líder revisó sus términos sobre el control y el tratamiento de los datos. SubQ no entrena con las entradas ni con las salidas, el cliente controla el periodo de retención y no comparte los datos con terceros. Es encargado (C5). Si los tokens de regalo cuentan como pago ya no importa, porque el destino depende de las condiciones (C2).
- **La comparación de calidad** (frente a la recuperación por fragmentos) va con la batería, cuando exista el IA-4, con una corrida real corta.

## Terminado cuando

Aprobado por el líder el 2026-10-25, con las decisiones C1 a C5 de abajo y su cambio a la C4. Las casillas se marcan cuando cada punto queda hecho.

- [ ] **El ADR-004 fija las clases y su tabla:** las cinco clases que ya usa el diccionario, los destinos, las finalidades y qué permite cada combinación. El líder lo aprueba.
- [x] **La política es una sola decisión, en la plataforma** (`platform::policy`). Dada la clase, el destino y la finalidad, dice si el dato va, va tapado, va solo como agregado o no va. Es una tabla de datos, y una prueba la recorre completa sin modelo.
- [x] **Cada proveedor declara sus condiciones**, y la política las lee. Puede ser un encargado, o un proveedor sin condiciones verificadas. Ser encargado quiere decir que no entrena con lo que recibe, lo borra y lo guarda confidencial (LFPDPPP; Reglamento, art. 52).
  - Anthropic y Gemini son encargados: en producción solo existen sus APIs de pago, que administra el líder.
  - SubQ es encargado, porque el líder revisó sus condiciones (C5). Su retención se configura al mínimo.
  - Un proveedor nuevo entra sin condiciones verificadas hasta que se revisen.
- [x] **Se aplica en los cuatro puntos del runtime:**
  - **Previa:** el texto del chat pasa por el detector local antes de ir a un modelo. Con un proveedor sin condiciones verificadas, un texto con datos de personas no sale.
  - **Contexto:** lo que recibe un modelo solo lo arma la política (`ToModel`). El IA-4 no puede llamar a un modelo con otra cosa: no compila.
  - **Ejecución:** se deniega una capacidad que toca clases que la cuenta no ve (K2).
  - **Salida:** la salida de una capacidad se filtra campo por campo, según su destino. Lo tapado se restaura solo hacia la persona.
- [x] **Con un proveedor sin condiciones verificadas, no sale ningún dato interno, confidencial, personal ni sensible.** Hay pruebas en cada punto.
- [x] **Ninguna regla de qué ve la IA queda en un *prompt*.** El detector (el escáner de hoy) solo encuentra datos; la política decide.
- [x] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## La propuesta

### Las clases

Son las cinco que ya usa el diccionario del [[ERP-2 La clase de cada dato declarada una sola vez|ERP-2]]. El ERP declara la de cada campo; este bloque fija qué permite cada una.

| Clase                | Qué es                                                           | Ejemplos                                                                                        |
| -------------------- | ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `public`             | Lo que la institución publica, o lo que publica otro             | El nombre legal, el objeto social, una convocatoria                                             |
| `internal`           | De la institución, sin publicar                                  | Las notas, los conteos, los montos de un proyecto                                               |
| `confidential`       | Lo propio protegido: identificadores y cuentas de la institución | El RFC, la CLABE, los folios                                                                    |
| `personal`           | De una persona identificable que no es la institución            | El nombre, el teléfono o el correo de una beneficiaria, de alguien del personal o del patronato |
| `sensitive_personal` | Lo del art. 2 fr. VI de la LFPDPPP                               | La salud, el origen étnico, las creencias                                                       |

### Los destinos y las finalidades

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

### La tabla

| Clase | Modelo de un encargado | Modelo sin condiciones verificadas | Aviso |
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

Aprobadas el 2026-10-25, con dos cambios del líder:
- **C4:** la propuesta trataba el plan gratuito de Gemini como una forma de operar, y no lo es.
- **C5:** el líder revisó los términos de SubQ, que es encargado.

La columna «sin condiciones verificadas» queda para un proveedor nuevo, hasta que se revisen sus condiciones.

**Lo que sí piden los proveedores es lo de siempre:** el aviso de privacidad debe decir qué hacemos con los datos, incluido que un proveedor de IA los trata como encargado. Va como hallazgo para el líder.

**Dos matices de SubQ, para el líder** (no cambian la C5):
- **La retención cero no es automática:** hay que pedirla, y SubQ la aprueba. Sin ella, guarda a lo más 60 días, como Anthropic guarda 30.
- **Los terceros:** la página no habla de subencargados. Solo dice que comparte el contenido con su personal y sus asesores bajo confidencialidad. Eso lo regula su acuerdo de tratamiento de datos, que no está publicado en esa página.

| | Decisión | Por qué |
|---|---|---|
| **C1** | Cinco clases: las que ya usa el diccionario | La investigación proponía cuatro. `public` e `internal` se separan porque un proveedor sin condiciones verificadas solo puede recibir lo público. |
| **C2** | El destino depende de las condiciones del proveedor (encargado o sin verificar), no de si se le paga | Lo que importa es qué hace el proveedor con los datos (art. 52). |
| **C3** | Lo interno no va a un proveedor sin condiciones verificadas | Mientras no se sepa qué hace con lo que recibe, solo puede recibir lo público. |
| **C4** | **El plan gratuito de Gemini solo existe en desarrollo**, con los datos ficticios de `fixtures/`. En producción solo hay APIs de pago, que administra el líder, y no se entra al sistema sin pagar | Para la política, Gemini es encargado: en producción solo existe de pago, y en desarrollo solo ve datos ficticios. No hace falta ninguna casilla de facturación. |
| **C5** | **SubQ es encargado.** El líder revisó sus términos sobre el control y el tratamiento de los datos (2026-10-25) | Sus [términos](https://subq.ai/terms) (actualizados el 2026-10-16) dicen:<ul><li>No usa el contenido del cliente para entrenar (§3), y no hay excepción para los créditos de regalo.</li><li>Guarda las entradas y salidas a lo más 60 días, y ofrece retención cero si se le pide (§4).</li><li>Trata ese contenido como confidencial del cliente (§5).</li><li>Como encargado, lo rige su acuerdo de tratamiento de datos (§4).</li></ul>Lo que pide es lo de siempre: tener los permisos para mandar los datos, y que nuestro aviso de privacidad diga qué hacemos con ellos. La comparación de calidad sigue con la batería del IA-4. |

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
4. **Los proveedores declaran sus condiciones.** El `Runtime` recibe el proveedor activo, con su id y si es encargado, o ninguno.
   - La app lo arma con la configuración de la IA. Anthropic y Gemini son encargados (C4).
   - SubQ también es encargado (C5). Todavía no está en el código: cuando entre como proveedor, lo declara así, con su retención al mínimo.
   - Un proveedor nuevo entra sin condiciones verificadas, hasta que se revisen.
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
   - cada punto con un proveedor encargado, con uno sin condiciones verificadas y sin proveedor;
   - lo tapado restaurado solo hacia la persona;
   - un agregado de menos de 3.

   Además, `src/lib/types.ts` gana `withheld` y los códigos nuevos.
10. **La suite completa y el PR.**

**Lo que toca de otros rieles:**
- **ERP:**
  - Sus pantallas usan el escáner por `crate::scanner`, y su código no cambia.
  - El comentario de `kernel::CLASSES` («las definitivas vienen con el ADR del IA-3») pasa a nombrar el ADR-004. Lo cambia el riel IA en este PR: es un pedido técnico del riel ERP, acordado el 2026-10-25 (§4 del modelo de trabajo).
  - Si un rol llega a ver menos clases, eso es de K2.
- **Diseño:**
  - dibujar `withheld` en la pantalla;
  - los textos de `class_not_seen` y `not_for_this_provider`.

  Hasta entonces, la pantalla muestra lo que sí llega.

## Cómo probarlo

PR [#34](https://github.com/J41M3C4B/SociAI/pull/34). Si el #33 (ERP-1) entra antes, el riel IA hace el `rebase`.

**Cómo correrlo:**
- **Carpeta y rama:** `D:\Agente Proyectos\sociai-ia`, `ia/ia-3-clases-y-politica`.
- **Antes:** nada. La primera compilación tarda unos minutos más por el *crate* nuevo, `privacy`.
- **Arrancar:**
  1. `cargo test` desde `src-tauri`, en PowerShell.
  2. `pnpm tauri dev` desde la carpeta.
- **Al terminar:** cerrar la app.

**Qué hacer y qué debe pasar:**

- [ ] **`cargo test` pasa completo** (704 pruebas). Las del IA-3 están en tres lugares:
  - `crates/platform/src/policy.rs`: la tabla del ADR, los dos tipos de proveedor, lo tapado que vuelve solo hacia la persona y el filtro de la salida;
  - `crates/platform/src/tests.rs`: `the_chat_meets_the_policy_before_any_model` y `a_notice_hands_over_only_what_may_go_in_one`;
  - `crates/privacy/src/vault.rs`: `a_value_of_the_dictionary_is_covered_whole`.
- [ ] **`pnpm tauri dev` arranca igual que antes.** En las pantallas no cambia nada, porque los dos roles ven todas las clases.
- [ ] **El ADR-004** (`docs/adr/ADR-004-las-clases-de-datos-y-la-politica.md`) dice lo que se aprobó: las cinco clases, los proveedores según sus condiciones, la tabla y sus reglas.

## Hallazgos

- **La negativa por clase (`class_not_seen`) no se puede probar de punta a punta:** hoy ningún rol deja de ver una clase. Se prueba en la política (`may_touch`). Si K2 agrega un rol así, se agrega la prueba con él.
- **Un RFC de una organización o una CLABE escritos en el chat detienen el texto,** también con un encargado. Es la tabla: lo `confidential` nunca va a un modelo, ni tapado. La persona usa la pantalla. Si se quiere que el chat los acepte tapados, es un cambio a la tabla, y por lo tanto un ADR nuevo.
- **El detector de la plataforma no usa «frases seguras».** Un lugar con nombre de persona («Casa Hogar San José») se tapa como un nombre, o detiene el texto con un proveedor sin condiciones verificadas. Esas falsas alarmas se miden con la batería del IA-4.
- **SubQ:** cuando entre al código como proveedor, se pide su retención cero (sin ella guarda a lo más 60 días) y se declara encargado.
- **El aviso de privacidad** debe decir qué hacemos con los datos, incluido que un proveedor de IA los trata como encargado. Es del líder.
- **El contacto de la institución como `confidential`** fue una decisión del ERP-1, no de esta propuesta. Con la tabla del ADR-004, ningún modelo lo verá. Lo decide el líder en el ERP-1.

## Resultado
