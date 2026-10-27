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
adr: ADR-005
creado: 2026-10-20
cerrado:
---

## Qué y por qué

La primera petición escrita en el chat que recorre **todo** el plano operativo, con IA. El enrutador entiende la intención y propone capacidades del registro, y un operador resuelve una interpretación puntual. Con esto empieza a sustituirse el Asistente de hoy.

**Nota del líder (2026-10-26, E2):** llenar con la IA no es un complemento. Por el principio de quitar o disminuir la fricción para personas no técnicas, el chat tiene que poder cambiar datos desde este bloque, y hay que pensar bien cómo. Por eso el bloque cubre las dos cosas: **preguntar** («¿qué me falta?») y **llenar** («el teléfono de la institución ahora es…»).

## Terminado cuando

(Segunda propuesta del riel IA, 2026-10-26, con las notas del líder a la E1–E4. La aprueba el líder junto con la E5.)

- [ ] **Preguntar.** «¿Qué me falta por llenar?», escrita en el chat, recorre todo el plano operativo:
  - la política previa;
  - el enrutador elige `mi_institucion.que_falta`;
  - el ejecutor la corre con todos sus controles.

  Vuelve por los **dos caminos de la E1**:
  - **A:** el operador `ia.responder` escribe la respuesta, y la plataforma pone la evidencia (de qué capacidad y de qué paso sale cada dato);
  - **B:** los `Datos` de la capacidad, sin otra llamada al modelo.

  Los dos se prueban de punta a punta con un modelo simulado. La corrida real los pone lado a lado, y el líder elige con ella.
- [ ] **Llenar.** «El teléfono de la institución ahora es 55 1234 5678», escrita en el chat, termina en una **propuesta para aprobar**:
  - el modelo nunca ve el número, solo su marca (`[TELEFONO_1]`);
  - la plataforma pone el valor en la entrada del cambio, y la propuesta lo muestra con su origen: «lo escribió usted»;
  - solo cambia lo que la persona dijo, y el resto de la ventana queda igual;
  - se aplica solo cuando la persona aprueba, con la tarea como origen (K5).

  **Ningún valor propuesto lo inventa el modelo.** Cada valor es una de tres cosas:
  - una marca;
  - un texto que está en lo que escribió la persona;
  - una opción de la lista del campo.

  Un valor que no pasa esa revisión no se propone: se le pregunta a la persona como campo del diccionario (IA-8).
- [ ] **El enrutador solo elige de una lista ya filtrada:**
  - por módulo encendido (K12);
  - por permiso de la cuenta (K2).

  Si el modelo nombra una capacidad fuera de la lista, la tarea se deniega con `not_offered`. Conocer la intención no da permisos: ni un texto que lo pida ni un modelo que se equivoque sacan una capacidad de la lista. **Desde el chat, todo cambio pide aprobación**, aunque su capacidad no la pida desde una pantalla. Hay pruebas de las tres cosas.
- [ ] **Los operadores son de la plataforma** (A6): `ia.enrutar`, `ia.responder` e `ia.llenar`.
  - Cada uno lleva su *prompt* versionado, el esquema de su respuesta y su nivel de modelo.
  - Los invoca el mismo ejecutor.
  - El modelo solo recibe un `ToModel`.
  - La respuesta se valida con su esquema, y las cifras que escribe tienen que estar en lo que recibió.
- [ ] **La memoria (E4): la pregunta de ahora manda.**
  - Una continuación («¿y cuál es la más urgente?») usa el turno anterior.
  - Un cambio de tema lo deja fuera: el enrutador dice si el texto continúa o empieza otra cosa, y solo lo que continúa llega a los operadores.

  La batería mide los dos casos.
- [ ] **Cada llamada a un modelo es un paso de la tarea:**
  - cuenta contra sus límites;
  - reserva su gasto antes y lo ajusta después;
  - queda registrada con el id de la tarea, sus *tokens* y su costo;
  - respeta el ritmo de 14 por minuto.
- [ ] **Una pregunta ambigua se aclara con opciones** (las capacidades candidatas), y la persona elige. Una fuera de alcance dice qué sí se puede hacer.
- [ ] **El chat no congela la app:** el turno contesta «en curso» al instante, y el resultado llega por la entrega común. La base no queda tomada mientras el modelo piensa.
- [ ] **Una batería mide los aciertos con un modelo simulado,** sin red ni gasto. Sus categorías son:
  - caso normal, datos incompletos, ambigüedad, seguridad, fuera de alcance y varios turnos;
  - **llenar:** un valor, varios, uno tapado, uno que el modelo inventa;
  - **cambio de tema.**
- [ ] **Una corrida real corta** con el plan gratuito de Gemini y los datos de `fixtures/` (E3). Da los aciertos y el costo por pregunta de cada camino de la E1, y su salida completa va a un archivo.
- [ ] **El ADR-005 fija cómo se llena desde el chat** (E5). El líder lo aprueba.
- [ ] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## Las decisiones del líder

Las notas del líder (2026-10-26) van tal como las escribió. La última columna dice cómo quedan en la propuesta.

| | Decisión | Lo que recomendó el riel IA | Lo que dijo el líder | Cómo queda |
|---|---|---|---|---|
| **E1** | Quién escribe la respuesta del chat | Un operador, `ia.responder`, con la evidencia que pone la plataforma | «Probemos ambos caminos para ver cual da mejores resultados.» | **Se hacen los dos** (A: el operador; B: los `Datos`, sin otra llamada), con un interruptor en la configuración de la plataforma. La corrida real los pone lado a lado. Con el resultado, el líder elige y el otro sale. |
| **E2** | Qué puede empezar el chat | Solo consultas; los cambios, con el bloque del agente | «No podemos ser bloqueantes en este aspecto por que el uso de la IA para rellenar no es un complemento, la IA debe hacerlo por el principio de eliminar o disminuir la frccion de la UX con personas no tecnicas. Hay que pensar bien esta funcionalidad.» | **El chat llena desde este bloque**, con el operador `ia.llenar` y la propuesta para aprobar. Cómo se hace sin que el modelo vea los valores ni los invente es la **E5**. |
| **E3** | La corrida real | El plan gratuito de Gemini con `fixtures/` | «Vale continuamos con la API de google gratis, nada cambia, el entorno debe trabajar como el produccion pero con fixtures sinteticos.» | **Aprobada.** La corrida usa el mismo camino que producción: solo cambian la llave, el modelo y los datos, que son los ficticios de `fixtures/`. |
| **E4** | La memoria de la conversación | La tarea guarda sus turnos, y el enrutador recibe el anterior por la política | «Vale solo que si hay que tener presente la prioridad de la memoria de la conversacion, el contexto y la pregunta o interaccion actual, para evitar forzar la memoria en una pregunta o consulta donde el contexto ya no pertenece» | **Aprobada, con el orden que pide el líder:** primero la pregunta de ahora, después el contexto y al final la memoria. El enrutador dice si el texto continúa el turno anterior. Si no, el anterior no llega a nadie. La batería tiene casos de cambio de tema. |

### La decisión nueva: E5, cómo se llena desde el chat (ADR-005)

Hoy, con la tabla del ADR-004, «mi RFC es…» detiene el chat (hallazgo del IA-3), y un teléfono escrito en el chat solo puede ir tapado para enrutar. Para llenar sin fricción, la propuesta es esta. **Es un cambio a la tabla, y por eso va en un ADR nuevo, el ADR-005.** El ADR-004 no se edita.

1. **Una finalidad nueva, `llenar`:** lo que la persona escribe para guardarlo en su propio ERP.
2. **Lo que la persona escribe en el chat** va tapado si es `personal` o `confidential`, para `enrutar` y para `llenar`, a un encargado.
   - El modelo ve `[RFC_1]` o `[TELEFONO_1]`, nunca el valor.
   - `sensitive_personal` sigue sin ir.
   - Con un proveedor sin condiciones verificadas, nada cambia: solo va lo público.
   - **Los datos guardados no cambian de regla:** un campo `confidential` del ERP sigue sin ir a ningún modelo. La regla nueva es solo para lo que la persona acaba de escribir.
3. **Lo tapado vuelve a la entrada de una capacidad solo en un cambio con aprobación,** y la propuesta le muestra el valor a la persona antes de aprobar. Nunca vuelve en una consulta, un aviso ni la auditoría.
4. **Ningún valor propuesto viene del modelo:** cada uno es una marca, un texto que está en lo que escribió la persona o una opción de la lista del campo. La plataforma lo revisa por código, como las cifras.

**Por qué:** la persona escribe su dato una vez y lo ve en la propuesta, el modelo solo decide en qué campo va, y nada se guarda sin su aprobación. La protección de lo guardado no baja.

## Plan

1. **El ADR-005** (E5), con su fila en `docs/adr/README.md` y en la §9 y la §13 del [plano de la IA](../../plano-de-la-ia.md). En la política:
   - la finalidad `Purpose::Fill`;
   - la regla del texto de la persona;
   - `ToModel::restore_into`, que solo acepta la entrada de un cambio con aprobación.

   Una prueba recorre la tabla nueva completa.
2. **La puerta al modelo** (P11), en la plataforma: `trait Models`.
   - **La llamada:** recibe el operador y un `ToModel`, nunca texto suelto. Devuelve el valor, el modelo que contestó, los *tokens* y el costo.
   - **El proveedor y sus condiciones vienen del mismo objeto que llama** (`Models::provider`): la política nunca decide con un proveedor y llama a otro. Reemplaza el `provider` que hoy recibe el `Runtime`.
   - **La app la implementa** con lo que ya existe en `ai/`: los proveedores, `paced` y los precios de `settings`. Es el mismo camino en desarrollo y en producción (E3).
   - **El modelo simulado de las pruebas** la implementa dentro de la plataforma, y responde por operador.
3. **Los operadores** (A6), en `crates/platform/src/operators/`. Cada uno declara su id `ia.…`, su *prompt* en un archivo versionado, el esquema de su respuesta, su nivel de modelo y su finalidad. El registro los lista, y no se ofrecen al enrutador.
   - **`ia.enrutar`** recibe el texto tapado, el turno anterior marcado como anterior (E4) y las candidatas, cada una con su id, su descripción y su efecto. Devuelve:
     - `invocar`, con la capacidad;
     - `elegir`, con dos o tres opciones;
     - `fuera_de_alcance`;
     - y además si el texto continúa el turno anterior.
   - **`ia.llenar`** recibe el texto tapado y los campos de la entrada del cambio elegido, cada uno con su id, su tipo y sus opciones, sin valores guardados. Devuelve los valores por campo.
   - **`ia.responder`** (camino A de la E1) recibe la pregunta y la salida de la capacidad, armada por la política. Devuelve el texto.
4. **Las candidatas, por código,** antes de cualquier modelo:
   - del registro;
   - de un módulo encendido;
   - que la cuenta puede usar (K2).

   Va un tope de candidatas en la configuración. Las descripciones y los campos son palabras del programa: entran al `ToModel` como un dato nuevo, `Datum::Program`, que siempre es `public`.
5. **Preguntar,** en el ejecutor, después de la previa:
   1. enrutar; una capacidad fuera de la lista deniega con `not_offered`;
   2. invocar, con los controles de una pantalla: límites, K2, clases, esquema y auditoría;
   3. responder:
      - **camino A:** el gestor de contexto (P6) convierte la salida en datos con su campo y su clase (IA-8, y la regla 3 del ADR-004 para lo que no nombra campo). `policy::to_model` arma lo que recibe `ia.responder`. Su respuesta se valida con su esquema y con sus cifras: `ai/figures.rs` se muda a la plataforma, porque es la validación de la salida (P7). La evidencia la pone la plataforma: la capacidad, el paso y `Calculation`;
      - **camino B:** se entregan los `Datos`, filtrados por la política de salida.
6. **Llenar:**
   1. enrutar a un cambio;
   2. `ia.llenar` da los valores por campo;
   3. la plataforma revisa cada valor (E5, punto 4). Lo que no pasa se quita y se pregunta con `Ask::Missing`;
   4. las marcas vuelven a su valor dentro de la entrada (`restore_into`);
   5. `prepare` da la propuesta, y la tarea se pausa esperando la aprobación, aunque la capacidad no la pida desde una pantalla. La evidencia de cada valor es `Person`;
   6. la persona aprueba, y se aplica con la huella (IA-7). Si rechaza, no se aplica nada.
7. **Cambiar solo lo que se dijo.** Hoy guardar una ventana vacía los campos que no trae, así que la plataforma tendría que mandar los valores guardados, y no debe leerlos para eso. **Pedido técnico al riel ERP:** que Mi institución publique un cambio que solo toque los campos que trae. Se acuerda por mensaje en cuanto el líder apruebe este bloque. Mientras tanto, las pruebas usan el módulo de muestra.
8. **La aclaración con opciones:** el contrato de resultados gana `Ask::Choose`, con las capacidades y sus descripciones. La persona contesta con `Content::Answer`, y la tarea sigue.
9. **El gasto y el registro:**
   - cada llamada reserva el gasto de la tarea (`Task::reserve`) y lo ajusta con el costo real (`settle`);
   - queda en una tabla de la plataforma (`ai_model_call`, migración propia): tarea, paso, operador, modelo, *tokens* y costo;
   - el tope mensual de hoy sigue, y la app lo revisa antes de llamar;
   - la auditoría lleva el paso del operador, sin el texto.
10. **El turno sin congelar la app:**
    - `runtime_submit` con un texto abre la tarea, pasa la previa y contesta «en curso»;
    - el resto corre aparte: se toma la base para cada paso y se suelta mientras el modelo piensa;
    - el resultado llega por `task-update`, y se puede pedir con `runtime_task`.
11. **La memoria** (E4):
    - la tarea guarda sus turnos en una tabla de la plataforma (`ai_turn`);
    - el enrutador recibe primero la pregunta de ahora, y el turno anterior solo como anterior;
    - si dice que no continúa, el anterior no llega a ningún otro operador;
    - hay un tope de turnos en la configuración.
12. **La batería,** en la plataforma, con el modelo simulado: unos 24 casos en las categorías del «Terminado cuando», con la capacidad y los valores esperados. Mide los aciertos y revisa los controles en cada caso.
13. **La corrida real** (E3): una prueba `_live`, ignorada por omisión, con los mismos casos y los dos caminos de la E1.
    - **Las llamadas:** unas 42 (24 del enrutador, unas 10 de `ia.responder` y unas 8 de `ia.llenar`). El camino B no hace llamadas de más.
    - **El ritmo:** 12 por minuto, unos 4 minutos.
    - **La salida completa va a un archivo:** aciertos, *tokens* y costo por pregunta, y las respuestas de A y B lado a lado.

    Antes de lanzarla te confirmo cuántas llamadas son.
14. **`src/lib/types.ts`** gana `Ask::Choose` y los códigos nuevos (`not_offered`, `out_of_scope`). **La suite completa y el PR.**

**Lo que toca de otros rieles:**
- **ERP:** el cambio que solo toca los campos que trae (paso 7), como pedido técnico. Por lo demás, usa `mi_institucion.que_falta` (`data: [Internal]`, va a un encargado) tal como la publica el ERP-1.
- **Diseño (DIS-1):** que el chat dibuje:
  - la `Respuesta` con su evidencia;
  - los `Datos` (camino B);
  - la `Propuesta` con su aprobación;
  - la aclaración `Ask::Choose`;
  - el «en curso», escuchando `task-update`;
  - los textos de los códigos nuevos.

  Se acuerda con el riel de diseño en cuanto el líder apruebe. Hasta que el DIS-1 entre, este bloque se prueba con la batería y la corrida real, no en la ventana.

**Lo que no entra:** llenar desde un documento (un acta, una constancia). Espera a que los documentos sean capacidades del núcleo (K6). El camino es el mismo: `ia.llenar`, con el documento en vez del texto.

**Orden:** el IA-3 y el ERP-1 ya están en `main`. La rama nace de `main` al día cuando el líder apruebe.

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
