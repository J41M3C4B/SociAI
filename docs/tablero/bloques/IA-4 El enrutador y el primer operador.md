---
tipo: bloque
id: IA-4
carril: ia
estado: backlog
prioridad: media
caja: Enrutador de tareas; Operadores de IA
pedido_por:
depende_de: IA-3, ERP-1, DIS-1
rama:
adr:
creado: 2026-10-20
cerrado:
---

## Qué y por qué

La primera petición escrita en el chat que recorre **todo** el plano operativo, con IA. El enrutador entiende la intención y propone capacidades del registro, y un operador resuelve una interpretación puntual. Con esto empieza a sustituirse el Asistente de hoy.

## Terminado cuando

(Propuesta del riel IA, 2026-10-26; la aprueba el líder junto con las decisiones E1 a E4 de abajo.)

- [ ] **«¿Qué me falta por llenar?», escrita en el chat, recorre todo el plano operativo y vuelve con evidencia:**
  - la política previa (IA-3);
  - el enrutador elige `mi_institucion.que_falta`;
  - el ejecutor la corre con todos sus controles;
  - el operador `ia.responder` escribe la respuesta;
  - llega una `Respuesta` cuya evidencia dice de qué capacidad y de qué paso sale cada dato.

  Se prueba de punta a punta con un modelo simulado. En la app, cuando el DIS-1 dibuje la respuesta.
- [ ] **El enrutador solo elige de una lista ya filtrada:**
  - por módulo encendido (K12);
  - por permiso de la cuenta (K2);
  - por efecto (E2).

  Si el modelo nombra una capacidad fuera de la lista, la tarea se deniega con `not_offered`. Conocer la intención no da permisos: ni un texto que lo pida ni un modelo que se equivoque sacan una capacidad de la lista. Hay pruebas de las dos cosas.
- [ ] **Los operadores son de la plataforma** (A6): `ia.enrutar` e `ia.responder`.
  - Cada uno lleva su *prompt* versionado, el esquema de su respuesta y su nivel de modelo.
  - Los invoca el mismo ejecutor.
  - El modelo solo recibe un `ToModel`.
  - La respuesta se valida con su esquema, y las cifras que escribe tienen que estar en lo que recibió: la IA no inventa cifras.
- [ ] **Cada llamada a un modelo es un paso de la tarea:**
  - cuenta contra sus límites;
  - reserva su gasto antes y lo ajusta después;
  - queda registrada con el id de la tarea, sus *tokens* y su costo;
  - respeta el ritmo de 14 por minuto.

  Los límites cortan una tarea que no avanza.
- [ ] **Una pregunta ambigua se aclara con opciones** (las capacidades candidatas), y la persona elige. Una fuera de alcance dice qué sí se puede hacer, sin llamar a ninguna capacidad.
- [ ] **El chat no congela la app:** el turno contesta «en curso» al instante, y la respuesta llega por la entrega común. La base no queda tomada mientras el modelo piensa.
- [ ] **Una batería mide los aciertos con un modelo simulado,** sin red ni gasto. Va por categorías: caso normal, datos incompletos, ambigüedad, seguridad, fuera de alcance y varios turnos.
- [ ] **Una corrida real corta** (E3) da los aciertos y el costo por pregunta. Su salida completa va a un archivo.
- [ ] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## Las decisiones para el líder

| | Decisión | Se recomienda | Por qué |
|---|---|---|---|
| **E1** | Quién escribe la respuesta del chat | **Un operador, `ia.responder`:** escribe el texto, y la evidencia la pone la plataforma, nunca el modelo | El chat recibe una `Respuesta`, no una tabla (§6 del plano). La evidencia sale del paso que corrió, así que el modelo no puede inventar una fuente. Cuesta dos llamadas ligeras por pregunta. La alternativa es una llamada: entregar los `Datos` y que el chat los dibuje sin palabras. |
| **E2** | Qué puede empezar el chat en este bloque | **Solo consultas.** Si la intención es un cambio, la tarea termina con «hágalo desde su pantalla» y nombra la ventana | Preparar un cambio desde el chat necesita otro operador, el que saca los valores del texto. También decidir cómo vuelve un valor tapado a la entrada de una capacidad, que es una regla nueva de la política. Va con el bloque del agente (paso 6 del plano de la IA). |
| **E3** | La corrida real | **Con el plan gratuito de Gemini, en desarrollo, con los datos de `fixtures/`** (C4). Unas 18 preguntas, 2 llamadas cada una: **unas 36 llamadas, a 12 por minuto, unos 3 minutos.** El costo se calcula con los *tokens* reales y los precios de pago del modelo ligero | Encaja con la C4. El costo que importa es el de producción, y sale de los mismos *tokens*. Si prefieres el costo medido en una API de pago, se corre igual con tu llave de Anthropic, y gasta unos centavos. |
| **E4** | La memoria de la conversación | **La tarea guarda sus turnos,** y el enrutador recibe el anterior por la política, como cualquier dato | «¿Y cuál es el más urgente?» necesita el turno anterior. La conversación ya es parte de la tarea (§5 del plano), cifrada en la base como cualquier dato. |

## Plan

1. **La puerta al modelo** (P11), en la plataforma: `trait Models`.
   - **La llamada:** recibe el operador y un `ToModel`, nunca texto suelto. Devuelve el valor, el modelo que contestó, los *tokens* y el costo.
   - **El proveedor y sus condiciones vienen del mismo objeto que llama** (`Models::provider`). Así, la política nunca decide con un proveedor y llama a otro. Reemplaza el `provider` que hoy recibe el `Runtime`.
   - **La app la implementa** con lo que ya existe en `ai/`: los proveedores, `paced` y los precios de `settings`. Lo viejo no se mueve en este bloque: la plataforma solo conoce el *trait*.
   - **El modelo simulado de las pruebas** también lo implementa, dentro de la plataforma, y responde por operador.
2. **Los operadores** (A6), en `crates/platform/src/operators/`.
   - **Su declaración:** un id `ia.…`, su *prompt* en un archivo versionado, el esquema de su respuesta, su nivel de modelo y su finalidad (`enrutar` o `responder`).
   - **El registro los lista** junto a las capacidades. No se ofrecen al enrutador.
   - **`ia.enrutar`** recibe dos cosas:
     - el texto tapado;
     - las candidatas, cada una con su id, su descripción y el esquema de su entrada.

     Devuelve una de cuatro: `invocar` (con la capacidad y su entrada), `elegir` (con dos o tres opciones), `fuera_de_alcance` o `cambio` (E2).
   - **`ia.responder`** recibe la pregunta y la salida de la capacidad, armada por la política. Devuelve el texto.
3. **Las candidatas, por código,** antes de cualquier modelo:
   - del registro;
   - de un módulo encendido;
   - que la cuenta puede usar (K2);
   - con efecto de consulta (E2).

   Va un tope de candidatas en la configuración de la plataforma. Las descripciones y los esquemas son palabras del programa: entran al `ToModel` como un dato nuevo, `Datum::Program`, que siempre es `public`.
4. **El turno del chat,** en el ejecutor, después de la previa del IA-3:
   1. **Enrutar:** se llama a `ia.enrutar`. Una capacidad fuera de la lista deniega con `not_offered`. Si va dentro, su entrada pasa por el esquema y por los controles de siempre.
   2. **Invocar:** con los mismos controles que una pantalla: límites, K2, clases, esquema y auditoría.
   3. **Responder:**
      - el gestor de contexto (P6) convierte la salida en datos con su campo y su clase (IA-8), con la regla 3 del ADR-004 para lo que no nombra campo;
      - `policy::to_model` arma lo que recibe `ia.responder`;
      - la respuesta se valida con su esquema y con sus cifras (`ai/figures.rs` se muda a la plataforma: es la validación de la salida, P7);
      - lo tapado se restaura solo hacia la persona.
   4. **Entregar:** una `Respuesta` con su evidencia: la capacidad, el paso y `Calculation`.

   Si la elección es `elegir`, la tarea se pausa con una aclaración de opciones. Con `fuera_de_alcance` o `cambio`, termina con su código y un camino.
5. **La aclaración con opciones:** el contrato de resultados gana `Ask::Choose`, con las capacidades y sus descripciones. La respuesta de la persona es `Content::Answer` con la opción elegida, y la tarea sigue desde el paso 2.
6. **El gasto y el registro:**
   - cada llamada reserva el gasto de la tarea (`Task::reserve`) con un tope por llamada, y lo ajusta con el costo real (`settle`);
   - queda en una tabla de la plataforma (`ai_model_call`, migración propia): tarea, paso, operador, modelo, *tokens* y costo;
   - el tope mensual de hoy sigue: la app lo revisa antes de llamar;
   - la auditoría lleva el paso del operador, sin el texto.
7. **El turno sin congelar la app:**
   - `runtime_submit` con un texto abre la tarea, pasa la previa y contesta «en curso»;
   - el resto del turno corre aparte, sin la base tomada mientras el modelo piensa: se toma la base para cada paso y se suelta para llamar al modelo;
   - la respuesta llega por `task-update`, y se puede pedir con `runtime_task`.
8. **La conversación** (E4): la tarea guarda sus turnos en una tabla de la plataforma (`ai_turn`). El enrutador recibe el turno anterior por `to_model`.
9. **La batería,** en la plataforma, con el modelo simulado: unos 18 casos por categoría, con la capacidad esperada. Mide los aciertos del enrutador y revisa los controles en cada uno:
   - **seguridad:** un texto que pide saltarse las reglas o nombra una capacidad sin permiso;
   - **fuera de alcance:** algo que ningún módulo hace;
   - **ambigüedad, datos incompletos y varios turnos.**
10. **La corrida real** (E3): una prueba `_live`, ignorada por omisión, con los mismos casos. Usa el ritmo de `ai::paced`, a 12 por minuto. Su salida completa va a un archivo: aciertos, *tokens* y costo por pregunta. Antes de lanzarla te digo cuántas llamadas son.
11. **`src/lib/types.ts`** gana `Ask::Choose` y los códigos nuevos (`not_offered`, `out_of_scope`, `change_from_screen`). **La suite completa y el PR.**

**Lo que toca de otros rieles:**
- **ERP:** nada nuevo. Usa `mi_institucion.que_falta` tal como la publica el ERP-1, que declara `data: [Internal]`, así que va a un encargado.
- **Diseño (DIS-1):**
  - dibujar la `Respuesta` con su evidencia y la aclaración `Ask::Choose`;
  - los textos de los códigos nuevos;
  - que el chat muestre «en curso» y escuche `task-update`.

  Se acuerda con el riel de diseño como pedido técnico en cuanto el líder apruebe este bloque. Hasta que el DIS-1 entre, lo de este bloque se prueba con la batería y con la corrida real, no en la ventana.

**Orden:** se empieza cuando el IA-3 (#34) y el ERP-1 (#33) estén en `main`. La rama nace de `main` al día.

## Cómo probarlo

**Cómo correrlo:**
- Carpeta y rama:
- Antes:
- Arrancar:
- Al terminar:

**Qué hacer y qué debe pasar:**

- [ ] 

## Hallazgos

- **Escrito al planear (2026-10-26). «Pocas opciones por intención»** (P4) hoy se cumple solo: hay cuatro capacidades. Cuando haya decenas, hará falta un primer filtro por código (por módulo o por lugar) antes del enrutador. El tope de candidatas lo deja visible: si se pasa, la batería lo marca.
- **El Asistente de hoy** (`ai/agent.rs`) sigue vivo en este bloque. Se retira cuando el chat nuevo haga lo mismo, con el bloque del agente y el DIS-1.

## Resultado
