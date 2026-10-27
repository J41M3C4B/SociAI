---
tipo: bloque
id: IA-4
carril: ia
estado: en-curso
prioridad: media
caja: Enrutador de tareas; Operadores de IA
pedido_por:
depende_de: IA-3, ERP-1, DIS-1
rama: ia/ia-4-enrutador-y-primer-operador
adr:
creado: 2026-10-20
cerrado:
---

## Qué y por qué

La primera petición escrita en el chat que recorre **todo** el plano operativo, con IA. El enrutador entiende la intención y propone capacidades del registro, y un operador resuelve una interpretación puntual. Con esto empieza a sustituirse el Asistente de hoy.

**Este bloque es solo preguntar** (decisión del líder, 2026-10-27). Llenar desde el chat va en su propio bloque, el [[IA-9 Llenar desde el chat|IA-9]], que se toma en cuanto este pase a revisión.

## Terminado cuando

Aprobado por el líder el 2026-10-27, con las decisiones E1 a E4 de abajo.

- [ ] **«¿Qué me falta por llenar?», escrita en el chat, recorre todo el plano operativo:**
  - la política previa (IA-3);
  - el enrutador elige `mi_institucion.que_falta`;
  - el ejecutor la corre con todos sus controles.

  Vuelve por los **dos caminos de la E1**:
  - **A:** el operador `ia.responder` escribe la respuesta, y la plataforma pone la evidencia (de qué capacidad y de qué paso sale cada dato);
  - **B:** los `Datos` de la capacidad, sin otra llamada al modelo.

  Los dos se prueban de punta a punta con un modelo simulado. La corrida real los pone lado a lado, y el líder elige con ella.
- [ ] **El enrutador solo elige de una lista ya filtrada:**
  - por módulo encendido (K12);
  - por permiso de la cuenta (K2);
  - por efecto: solo consultas, hasta el IA-9.

  Si el modelo nombra una capacidad fuera de la lista, la tarea se deniega con `not_offered`. Conocer la intención no da permisos: ni un texto que lo pida ni un modelo que se equivoque sacan una capacidad de la lista. Hay pruebas de las dos cosas.
- [ ] **Un cambio pedido en el chat no se bloquea en silencio:** el enrutador lo reconoce, y la tarea termina con `change_from_screen` y la capacidad que lo haría, hasta que entre el IA-9.
- [ ] **Los operadores son de la plataforma** (A6): `ia.enrutar` e `ia.responder`.
  - Cada uno lleva su *prompt* versionado, el esquema de su respuesta y su nivel de modelo.
  - Los invoca el mismo ejecutor.
  - El modelo solo recibe un `ToModel`.
  - La respuesta se valida con su esquema, y las cifras que escribe tienen que estar en lo que recibió.
- [ ] **La memoria (E4): la pregunta de ahora manda.**
  - Una continuación («¿y cuál va primero?») usa el turno anterior.
  - Un cambio de tema lo deja fuera: el enrutador dice si el texto continúa o empieza otra cosa, y solo lo que continúa llega a los operadores.
- [ ] **Cada llamada a un modelo es un paso de la tarea:**
  - cuenta contra sus límites;
  - reserva su gasto antes y lo ajusta después;
  - queda registrada con el id de la tarea, sus *tokens* y su costo;
  - respeta el ritmo de 14 por minuto.

  Los límites cortan una tarea que no avanza.
- [ ] **Una pregunta ambigua se aclara con opciones** (las capacidades candidatas), y la persona elige. Una fuera de alcance dice qué sí se puede hacer, sin llamar a ninguna capacidad.
- [ ] **El chat no congela la app:** el turno contesta «en curso» al instante, y el resultado llega por la entrega común. La base no queda tomada mientras el modelo piensa.
- [ ] **Una batería mide los aciertos con un modelo simulado,** sin red ni gasto. Sus categorías son: caso normal, datos incompletos, ambigüedad, seguridad, fuera de alcance, varios turnos y cambio de tema.
- [ ] **Una corrida real corta** con el plan gratuito de Gemini y los datos de `fixtures/` (E3). Da los aciertos y el costo por pregunta de cada camino de la E1, y su salida completa va a un archivo.
- [ ] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## Las decisiones del líder

Las notas del líder (2026-10-26) van tal como las escribió. La última columna dice cómo quedaron.

| | Decisión | Lo que recomendó el riel IA | Lo que dijo el líder | Cómo queda |
|---|---|---|---|---|
| **E1** | Quién escribe la respuesta del chat | Un operador, `ia.responder`, con la evidencia que pone la plataforma | «Probemos ambos caminos para ver cual da mejores resultados.» | **Se hicieron los dos,** y la primera corrida real los puso lado a lado. **Decisión del líder (2026-10-27): combinarlos.** El texto del modelo da el contexto y el orden, y cita cada dato como `[[ruta]]` en vez de escribirlo. La respuesta lleva los datos exactos de la capacidad. Si una cita no existe, si se copia un dato o si se inventa una cifra, la persona recibe solo los datos. El interruptor salió. |
| **E2** | Qué puede empezar el chat | Solo consultas; los cambios, con el bloque del agente | «No podemos ser bloqueantes en este aspecto por que el uso de la IA para rellenar no es un complemento, la IA debe hacerlo por el principio de eliminar o disminuir la frccion de la UX con personas no tecnicas. Hay que pensar bien esta funcionalidad.» | **Dos bloques seguidos** (2026-10-27): este, solo preguntar; llenar va en el [[IA-9 Llenar desde el chat\|IA-9]], con su propio ADR, y se toma en cuanto este pase a revisión. |
| **E3** | La corrida real | El plan gratuito de Gemini con `fixtures/` | «Vale continuamos con la API de google gratis, nada cambia, el entorno debe trabajar como el produccion pero con fixtures sinteticos.» | **Aprobada.** La corrida usa el mismo camino que producción: solo cambian la llave, el modelo y los datos, que son los ficticios de `fixtures/`. |
| **E4** | La memoria de la conversación | La tarea guarda sus turnos, y el enrutador recibe el anterior por la política | «Vale solo que si hay que tener presente la prioridad de la memoria de la conversacion, el contexto y la pregunta o interaccion actual, para evitar forzar la memoria en una pregunta o consulta donde el contexto ya no pertenece» | **Aprobada, con el orden que pide el líder:** primero la pregunta de ahora, después el contexto y al final la memoria. El enrutador dice si el texto continúa el turno anterior. Si no, el anterior no llega a nadie. La batería tiene casos de cambio de tema. |

## Plan

1. **La puerta al modelo** (P11), en la plataforma: `trait Models`.
   - **La llamada:** recibe el operador y un `ToModel`, nunca texto suelto. Devuelve el valor, el modelo que contestó, los *tokens* y el costo.
   - **El proveedor y sus condiciones vienen del mismo objeto que llama** (`Models::provider`): la política nunca decide con un proveedor y llama a otro. Reemplaza el `provider` que hoy recibe el `Runtime`.
   - **La app la implementa** con lo que ya existe en `ai/`: los proveedores, `paced` y los precios de `settings`. Es el mismo camino en desarrollo y en producción (E3).
   - **El modelo simulado de las pruebas** la implementa dentro de la plataforma, y responde por operador.
2. **Los operadores** (A6), en `crates/platform/src/operators/`. Cada uno declara su id `ia.…`, su *prompt* en un archivo versionado, el esquema de su respuesta, su nivel de modelo y su finalidad. El registro los lista, y no se ofrecen al enrutador.
   - **`ia.enrutar`** recibe el texto tapado, el turno anterior marcado como anterior (E4) y las candidatas, cada una con su id y su descripción. Devuelve:
     - `invocar`, con la capacidad;
     - `elegir`, con dos o tres opciones;
     - `cambio`, con la capacidad que lo haría (hasta el IA-9);
     - `fuera_de_alcance`;
     - y además si el texto continúa el turno anterior.
   - **`ia.responder`** (camino A de la E1) recibe la pregunta y la salida de la capacidad, armada por la política. Devuelve el texto.
3. **Las candidatas, por código,** antes de cualquier modelo:
   - del registro;
   - de un módulo encendido;
   - que la cuenta puede usar (K2).

   Las consultas se ofrecen para invocar. Los cambios solo se ofrecen para reconocerlos. Va un tope de candidatas en la configuración. Las descripciones son palabras del programa: entran al `ToModel` como un dato nuevo, `Datum::Program`, que siempre es `public`.
4. **El turno del chat,** en el ejecutor, después de la previa del IA-3:
   1. **Enrutar:** una capacidad fuera de la lista deniega con `not_offered`. Si va dentro, su entrada pasa por el esquema y por los controles de siempre.
   2. **Invocar:** con los mismos controles que una pantalla: límites, K2, clases, esquema y auditoría.
   3. **Responder:**
      - **camino A:** el gestor de contexto (P6) convierte la salida en datos con su campo y su clase (IA-8, y la regla 3 del ADR-004 para lo que no nombra campo). `policy::to_model` arma lo que recibe `ia.responder`. Su respuesta se valida con su esquema y con sus cifras: `ai/figures.rs` se muda a la plataforma, porque es la validación de la salida (P7). Lo tapado se restaura solo hacia la persona. La evidencia la pone la plataforma: la capacidad, el paso y `Calculation`;
      - **camino B:** se entregan los `Datos`, filtrados por la política de salida.

   Si la elección es `elegir`, la tarea se pausa con una aclaración de opciones. Con `cambio` o `fuera_de_alcance`, termina con su código y un camino.
5. **La aclaración con opciones:** el contrato de resultados gana `Ask::Choose`, con las capacidades y sus descripciones. La persona contesta con `Content::Answer` y la opción elegida, y la tarea sigue desde la invocación.
6. **El gasto y el registro:**
   - cada llamada reserva el gasto de la tarea (`Task::reserve`) con un tope por llamada, y lo ajusta con el costo real (`settle`);
   - queda en una tabla de la plataforma (`ai_model_call`, migración propia): tarea, paso, operador, modelo, *tokens* y costo;
   - el tope mensual de hoy sigue, y la app lo revisa antes de llamar;
   - la auditoría lleva el paso del operador, sin el texto.
7. **El turno sin congelar la app:**
   - `runtime_submit` con un texto abre la tarea, pasa la previa y contesta «en curso»;
   - el resto corre aparte: se toma la base para cada paso y se suelta mientras el modelo piensa;
   - el resultado llega por `task-update`, y se puede pedir con `runtime_task`.
8. **La memoria** (E4):
   - la tarea guarda sus turnos en una tabla de la plataforma (`ai_turn`);
   - el enrutador recibe primero la pregunta de ahora, y el turno anterior solo como anterior;
   - si dice que no continúa, el anterior no llega a ningún otro operador;
   - hay un tope de turnos en la configuración.
9. **La batería,** en la plataforma, con el modelo simulado: unos 18 casos en las categorías del «Terminado cuando», con la capacidad esperada. Mide los aciertos del enrutador y revisa los controles en cada caso. En seguridad van un texto que pide saltarse las reglas y uno que nombra una capacidad sin permiso.
10. **La corrida real** (E3): una prueba `_live`, ignorada por omisión, con los mismos casos y los dos caminos de la E1.
    - **Las llamadas:** unas 30 (18 del enrutador y unas 12 de `ia.responder`). El camino B no hace llamadas de más.
    - **El ritmo:** 12 por minuto, unos 3 minutos.
    - **La salida completa va a un archivo:** aciertos, *tokens* y costo por pregunta, y las respuestas de A y B lado a lado.

    Antes de lanzarla le confirmo al líder cuántas llamadas son.
11. **`src/lib/types.ts`** gana `Ask::Choose` y los códigos nuevos (`not_offered`, `out_of_scope`, `change_from_screen`). **La suite completa y el PR.**

**Lo que toca de otros rieles:**
- **ERP:** nada nuevo. Usa `mi_institucion.que_falta` tal como la publica el ERP-1. Declara `data: [Internal]`, así que va a un encargado.
- **Diseño (DIS-1):** que el chat dibuje:
  - la `Respuesta` con su evidencia;
  - los `Datos` (camino B);
  - la aclaración `Ask::Choose`;
  - el «en curso», escuchando `task-update`;
  - los textos de los códigos nuevos.

  Se acuerda con el riel de diseño cuando tenga una sesión abierta. Hasta que el DIS-1 entre, este bloque se prueba con la batería y la corrida real, no en la ventana.

## Cómo probarlo

**Cómo correrlo:**
- Carpeta y rama:
- Antes:
- Arrancar:
- Al terminar:

**Qué hacer y qué debe pasar:**

- [ ] 

## Hallazgos

- **Avance al 2026-10-27, para retomar.**
  - **Dónde está:** rama `ia/ia-4-enrutador-y-primer-operador`, con 4 commits arriba; todavía sin PR. `cargo test` y la batería simulada pasan.
  - **La primera corrida real** (dos caminos separados): 20 de 20 aciertos, $0.014 MXN por pregunta.
  - **La segunda corrida real** (combinada): 19 de 20 aciertos, pero solo 4 de 10 respuestas pasaron la revisión con su texto. Las 6 de «¿qué me falta?» cayeron al respaldo de solo datos.
    - **Causa probable:** la revisión de «valor copiado» tomaba la palabra «legal» por el código de ventana `legal`. Ya está corregida: solo revisa los datos de la institución, no los códigos del programa. Además, el modelo ya puede citar un elemento completo (`[[missing[0]]]`).
    - **«Enséñame el perfil» falló** con `not_offered`: el modelo nombró algo fuera de la lista, y la plataforma lo bloqueó como debe.
  - **Lo que sigue** (el líder lo dejó para el 2026-10-28):
    1. La tercera corrida, con el rastro encendido (`SOCIAI_AI_TRACE`) y el motivo de cada respaldo en la salida: unas 29 llamadas, una cada 5 segundos.
    2. Luego, el `rebase`, el «Cómo probarlo», el PR y el acuerdo con el riel de diseño (DIS-1).
- **Los nombres de los campos en las respuestas.** El chat necesita los nombres de pantalla de los campos para dibujar las citas, y hoy viven en `src/i18n/es-MX.ts`.
  - `mi_institucion.que_falta` da el id corto del campo (`populations`), no el id completo del diccionario. Así, la pantalla no puede nombrarlo igual que en todas partes.
  - Es un pedido técnico al riel ERP: que dé el id completo.
- **«¿Cuál va primero?»** solo puede contestarse con certeza si los datos dicen el orden: qué es indispensable o qué bloquea otra cosa. Eso es del riel ERP. Mientras tanto, el operador dice que no hay un orden indicado.

- **Escrito al planear (2026-10-26). «Pocas opciones por intención»** (P4) hoy se cumple solo: hay cuatro capacidades. Cuando haya decenas, hará falta un primer filtro por código (por módulo o por lugar) antes del enrutador. El tope de candidatas lo deja visible: si se pasa, la batería lo marca.
- **El Asistente de hoy** (`ai/agent.rs`) sigue vivo en este bloque. Se retira cuando el chat nuevo haga lo mismo, con el bloque del agente y el DIS-1.

## Resultado
