# Registro de decisiones de arquitectura (ADR)

Cada decisión importante se documenta aquí. Una decisión no se edita: si cambia, se crea un ADR nuevo que la reemplaza.

La numeración empieza de nuevo en el **ADR-001**. Los anteriores (001 a 048) están en [`docs/archivo/adr/`](../archivo/adr/README.md); si se cita uno, se escribe «ADR-0xx (archivado)». Cómo se numera y se aprueba: [modelo de trabajo](../agents/modelo-de-trabajo.md), §6.

| ADR | Decisión | Estado |
|---|---|---|
| [ADR-001](ADR-001-el-contrato-de-capacidad-y-el-registro.md) | El contrato de capacidad y el registro: qué declara una capacidad, cómo se registra sin lista y la frontera que impide a un módulo usar la IA | Aceptada (IA-1) |
| [ADR-002](ADR-002-el-nucleo-del-erp-y-la-forma-de-un-modulo.md) | El núcleo del ERP y la forma de un módulo: Mi institución es un módulo, el núcleo va debajo de todos, cómo se reparten los datos y todo cambio se prepara y se aplica | Aceptada (ERP-7) |
| [ADR-003](ADR-003-la-plataforma-de-ia-por-dentro.md) | La plataforma de IA por dentro y cómo se acopla al ERP: sus doce piezas, la tarea con sus límites, un solo ejecutor, la política en cuatro puntos, la entrega común y las decisiones A1 a A7 | Aceptada (IA-6) |
| [ADR-004](ADR-004-las-clases-de-datos-y-la-politica.md) | Las clases de datos y la política que les toca: cinco clases, los proveedores según sus condiciones (encargado o sin verificar), los destinos, las finalidades, la tabla y dónde se aplica | Propuesta (IA-3) |
