---
tipo: bloque
id: IA-7
carril: ia
estado: hecho
prioridad: alta
caja: Contratos de capacidades; Registro común de capacidades; Ejecutor; Aprobación
pedido_por: ERP-9
depende_de: IA-5, ERP-9
rama: ia/ia-7-aprobacion-atada-a-la-propuesta
adr:
creado: 2026-10-23
cerrado: 2026-10-24
---

## Qué y por qué

**Pedido por el riel ERP.** El [[ERP-9 Preparar y aplicar, con historial y origen|ERP-9]] puso en el núcleo la propuesta, la huella y la unidad de trabajo: todo cambio de datos se prepara con `kernel::propose` y se aplica con `kernel::apply`, en una sola transacción, con su historial, su origen y su hecho. El riel IA aceptó el formato, y su «Resultado» deja una lista de lo que pone el riel IA. Hoy la plataforma todavía no lo usa:

- **La aprobación se ata a la huella del ejecutor**, la de la capacidad y su entrada, y no a la `proposal.fingerprint` del núcleo, que también incluye la versión del dato.
- **La `Call` no lleva la huella aprobada**, y el contrato no tiene un error para «el dato cambió desde que se aprobó» (`prepare_again`).
- **Ninguna capacidad puede aplicar en el núcleo.** `kernel::apply` abre su transacción con la conexión en exclusiva (`&mut Connection`), pero la `Call` y todo el ejecutor trabajan con la conexión compartida. Por eso, en el `rebase` del [[IA-5 Las capacidades de un módulo fuera de la app|IA-5]], la capacidad de la muestra quedó de lectura (aprobado por el líder el 2026-10-23).
- **Un `apply` que no contestó** termina hoy en `uncertain_change` sin preguntar al núcleo. `kernel::applied` ya puede decir si esa huella se aplicó.

Para qué: el ERP-1 publica las capacidades de cambio de Mi institución, y cada una debe pasar por el núcleo con la aprobación atada a lo que la persona vio.

**Lo que dejó escrito el ERP-9 para el riel IA:**
1. `Call.fingerprint` y `CapabilityError::Stale` (en la pantalla, con el motivo `data_changed`).
2. Que el ejecutor ate la aprobación a `proposal.fingerprint`.
3. Que el registro exija que el `prepare` de toda capacidad de cambio devuelva una `kernel::Proposal`.
4. Cómo traduce el contrato los demás resultados de `apply`: `invalid` como `Invalid`; `bad_origin`, `bad_fact`, `version_not_raised` y `database` como `Failed`.
5. Que una propuesta sin cambios no pida aprobación.
6. Que un `apply` incierto se consulte con `kernel::applied` y nunca se aplique por su cuenta.

## Terminado cuando

Aprobado por el líder el 2026-10-23. Las casillas se marcan cuando cada punto queda hecho.

- [x] **Una capacidad de cambio aplica en el núcleo.** La muestra publica `muestra.guardar_nota`, con su permiso `muestra.notas.escribir`:
  - su `prepare` devuelve la `kernel::Proposal`;
  - su `apply` llama a `kernel::apply`, que guarda la nota con su historial, su hecho y el origen de la tarea (el id de la tarea y la cuenta que aprobó).

  Una prueba de la app la recorre de punta a punta: a la Dirección se le niega, y la Administración prepara, aprueba y guarda. Nada escribe saltándose el núcleo.
- [x] **La aprobación se ata a la huella del núcleo.** La persona aprueba `proposal.fingerprint`, y el `apply` la recibe en `Call.fingerprint`. Si el dato cambió después de aprobar, el contrato responde `CapabilityError::Stale`, y el ejecutor prepara de nuevo y pide otra aprobación, con el motivo `data_changed`. Una prueba lo demuestra.
- [x] **El registro lo hace cumplir.** Rechaza una capacidad de cambio cuyo `prepare` no devuelve una `kernel::Proposal`.
- [x] **Cada resultado de `apply` tiene su traducción fija:**
  - `invalid` se vuelve `Invalid`, con su campo y su código;
  - `already_applied` termina bien, con el mismo cambio;
  - `bad_origin`, `bad_fact`, `version_not_raised` y `database` se vuelven `Failed`.
- [x] **Una propuesta sin cambios no pide aprobación:** la tarea termina y dice que no había nada que cambiar.
- [x] **Un `apply` incierto se consulta, no se repite.** Si la app se cerró a media aplicación, al volver el ejecutor pregunta a `kernel::applied` por la huella aprobada:
  - si se aplicó, la tarea termina bien;
  - si no, la tarea lo dice y no aplica sola.

  Una prueba lo demuestra.
- [x] `cargo test` y `pnpm test` pasan, sin red ni gasto.

## Plan

La rama sale de `main` con el IA-5 ya fusionado.

1. **La conexión exclusiva, de raíz.** Solo el `apply` de una capacidad la recibe; leer y preparar siguen con la compartida.
   - `Call.conn` pasa a ser `&mut Connection`. Las funciones de lectura y de `prepare` reciben `&Call`, y las de `apply` reciben `&mut Call`. El macro `capability!` lo revisa al compilar, como ya revisa que las dos tomen la misma entrada.
   - El runtime recibe la conexión exclusiva: `Runtime::submit(&mut Connection, …)`.
   - `Accounts::now` recibe la conexión como argumento, para que `commands/runtime.rs` no tenga que prestarla dos veces.
2. **El contrato gana lo del ERP-9** (`crates/capability`):
   - `Call.fingerprint: Option<&str>`: la huella que aprobó la persona, solo en el `apply`;
   - `Call::origin()`: el id de la tarea y la cuenta, para armar el origen del núcleo;
   - `CapabilityError::Stale`: lo preparado ya no vale, hay que preparar de nuevo;
   - `CapabilityError::from_apply(code)`: la traducción fija de los códigos de `kernel::ApplyError`, para que todos los módulos respondan igual.
     - `prepare_again`, `invalid` y `nothing_to_change` se vuelven `Stale`;
     - `bad_origin`, `bad_fact`, `version_not_raised` y `database` se vuelven `Failed`, con su código.

     El contrato no depende del núcleo (A2): traduce por el código, no por el tipo.
3. **El ejecutor ata la aprobación a la propuesta** (`crates/platform`). Para una capacidad `Modify`:
   - Lee la salida del `prepare` como `kernel::Proposal`:
     - si trae `invalid`, pausa y pide corregir el primero (`Ask::Correct`), con su campo y su código;
     - si no trae cambios, termina con `nothing_to_change` y no pide aprobación;
     - si trae cambios, pide la aprobación con `proposal.fingerprint`.
   - Al aplicar, pone esa huella en `Call.fingerprint`. Un cambio sin aprobación (`Never`) también lleva la huella que acaba de preparar.
   - Si el `apply` responde `Stale`, prepara de nuevo y pide otra aprobación con `again: data_changed` (`Again::DataChanged`). La protección contra pasos repetidos no lo cuenta como una repetición.
   - La huella del ejecutor (capacidad más entrada) sigue sirviendo para reconocer un paso repetido y una entrada que cambió. Una capacidad `Sensitive` sigue como hoy, con la aprobación atada a esa huella.
4. **El `apply` incierto.** Cada paso de `apply` guarda la huella con la que aplica. Si la app se cerró a media aplicación, al volver el ejecutor pregunta a `kernel::applied`:
   - si se aplicó, cierra el paso y la tarea termina bien, con el cambio;
   - si no, la tarea termina con `not_applied` y propone empezar de nuevo. No aplica sola.

   La transacción del núcleo no deja un cambio a medias, así que «no se aplicó» es seguro.
5. **La migración 3 de la plataforma:** `ai_step` gana la huella del `apply`, y `ai_approval` la huella de la entrada. Una migración nunca se edita.
6. **La revisión al arrancar:** una capacidad `Modify` cuyo `prepare` no da el esquema de `kernel::Proposal` detiene la app, junto a la revisión de permisos del IA-5. Una prueba la revisa sobre el registro real. La hace la plataforma, porque el registro vive en el contrato y no conoce el núcleo.
7. **La muestra publica `muestra.guardar_nota`** con `muestra.notas.escribir`:
   - `prepare` usa `service::prepare`, y `apply` usa `service::apply` con el origen de la tarea;
   - `NoteInput` deriva `JsonSchema`.

   La prueba de la app la recorre de punta a punta:
   - a la Dirección se le niega;
   - la Administración prepara, aprueba y guarda;
   - el historial del núcleo muestra el origen de la tarea.
8. **Las pruebas de la plataforma:**
   - la aprobación atada a la huella;
   - el dato que cambia después de aprobar (`data_changed`);
   - lo inválido;
   - la propuesta sin cambios;
   - el `apply` incierto, aplicado y no aplicado.

   Las capacidades de prueba de la plataforma pasan a usar la `Proposal` del núcleo.
9. **El frontend:** `Again` gana `data_changed` en `src/lib/types.ts`.
10. **La suite completa y el PR.**

**Precisiones al acuerdo del ERP-9** (no cambian el formato de la propuesta ni de la huella):
- **Un cambio sin aprobación también lleva la huella.** Así, `kernel::applied` sirve igual para los dos casos, y lo que se aplica es lo que se acaba de preparar.
- **`invalid` en el `apply` prepara de nuevo, en vez de traducirse directo.** `CapabilityError` solo lleva textos fijos del programa, y la lista de `invalid` del núcleo es dinámica. La persona ve el campo y el código en la propuesta nueva, como una corrección.

**Lo que toca de otros rieles:**
- **El módulo de muestra:** su capacidad de cambio, y `schemars` en su contrato (`muestra-contract`).
  - El ERP-2 está en curso y puede tocar la validación de la muestra; si choca, el `rebase` se hace del lado del IA-7.
- **El núcleo:** nada. Se usan `propose`, `apply`, `applied` y `Proposal` como los dejó el ERP-9.

## Cómo probarlo

PR: https://github.com/J41M3C4B/SociAI/pull/30

**Pruebas:** `cargo test`: 670 pasan y 0 fallan (contrato 14, núcleo 27, muestra 7, plataforma 32, app 582, documentación 8); `tsc`: sin errores; `pnpm test`: 224 pasan.

**Cómo correrlo:**
- **Carpeta y rama:** `D:\Agente Proyectos\sociai-ia`, rama `ia/ia-7-aprobacion-atada-a-la-propuesta`.
- **Antes:** compilar desde **PowerShell**; recompila unos minutos. Al abrirse, la base aplica la migración 3 de la plataforma (la huella de cada `apply` y de cada aprobación).
- **Arrancar:**
  1. Las pruebas que cuentan lo que pasa:
     ```
     cd src-tauri
     cargo test -- --nocapture --test-threads=1 walks_the_whole_path goes_through_the_kernel data_changed
     ```
  2. La app: `pnpm tauri dev`, desde `D:\Agente Proyectos\sociai-ia`.
- **Al terminar:** cerrar la app. No queda nada que limpiar.

**Qué hacer y qué debe pasar:**

- [ ] **Las tres pruebas pasan.** El recorrido muestra la propuesta del núcleo (antes y después, versión base y huella), la aprobación y las seis líneas de la auditoría.
- [ ] **La nota de la muestra se guarda por el núcleo** (`a_change_of_a_module_goes_through_the_kernel`):
  - a la Dirección se le niega;
  - la Administración ve la propuesta, la aprueba y la nota queda guardada;
  - el historial dice la tarea y la cuenta, y que el teléfono cambió, sin su valor.
- [ ] **Un dato que cambió después de aprobar** (`…_prepared_and_approved_again`) no se aplica: la persona ve la propuesta nueva con `data_changed` y la aprueba otra vez.
- [ ] **La app abre igual que siempre:** la revisión al arrancar no encuentra ningún cambio sin propuesta. En la consola, este comando sigue regresando `without_ai`:
  ```js
  await window.__TAURI_INTERNALS__.invoke("runtime_submit", { request: { channel: { kind: "chat" }, content: { kind: "text", text: "¿Qué me falta?" } } })
  ```

## Hallazgos

- **Con el ERP-2 (PR #29):** `kernel::propose` recibe el diccionario y `Draft` pierde `protected`. La forma de `Proposal` no cambia, así que el IA-7 sigue valiendo. Si el ERP-2 entra primero, el `rebase` ajusta el módulo de prueba de la plataforma y la documentación de `muestra/src/lib.rs`.
- **El plan decía `Call::origin()`, y no se hizo:** el contrato vive debajo del núcleo (A2) y no conoce `kernel::Origin`. El módulo arma el origen con `call.task` y `call.by`, en una línea.
- **La protección contra pasos repetidos** se precisó en el camino: un `apply` solo es repetido si aplica la misma huella. Sin eso, aplicar después de preparar de nuevo se cortaba como repetición.
- **La fila 7 del [plano de la IA](../../plano-de-la-ia.md)** dice que la `Call` también lleva la organización. Hoy hay una base por institución y ningún módulo la pide; no entró en este bloque.

## Resultado

Validado por el líder el 2026-10-24, al probar el [[IA-8 Los campos del diccionario en la plataforma|IA-8]], cuya rama lo traía dentro. Fusionado a `main` en el PR [#30](https://github.com/J41M3C4B/SociAI/pull/30) (`b4f1a17`), después del ERP-2.

- **El `rebase` sobre el ERP-2** chocó donde se esperaba. `kernel::propose` ahora recibe la entidad, y `Draft` ya no lleva `protected`. El módulo de prueba de la plataforma declara su nombre en el diccionario, y el diccionario revisa el largo, que antes revisaba el módulo. Con eso, `cargo test` dio 682 pruebas que pasan y `pnpm test` 224.
- **Lo que queda:** toda capacidad de cambio pasa por `kernel::propose` y `kernel::apply`. La aprobación se ata a `proposal.fingerprint`, un dato que cambió después de aprobar se prepara y se aprueba otra vez (`data_changed`), y un `apply` incierto se consulta con `kernel::applied`. La muestra publica `muestra.guardar_nota`.
- **Lo que sigue:** el ERP-1 publica las capacidades de cambio de Mi institución con esta forma.
