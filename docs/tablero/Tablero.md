---
tipo: tablero
---

# Tablero

Cada bloque es una nota en `tablero/bloques/` hecha con la plantilla «bloque». Para moverlo de columna se cambia su propiedad `estado`. Las reglas están en el [[modelo-de-trabajo|modelo de trabajo]].

**Rieles:**
- `ia`: la plataforma de inteligencia;
- `erp`: los módulos, sus datos y sus contratos, todo determinista;
- `diseno`: las pantallas y el sistema visual.

**Límites:** a lo más un bloque en curso por riel, y a lo más 5 en Backlog por riel.

## En curso

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", rama AS "Rama", depende_de AS "Depende de"
FROM "tablero/bloques"
WHERE estado = "en-curso"
SORT carril ASC
```

## Revisión

Hecho por el agente; espera que el líder lo pruebe.

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", rama AS "Rama"
FROM "tablero/bloques"
WHERE estado = "revision"
SORT carril ASC
```

## Esperando a otro riel

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", depende_de AS "Espera a", estado AS "Estado"
FROM "tablero/bloques"
WHERE depende_de AND estado != "hecho"
SORT carril ASC
```

## Acuerdos técnicos entre rieles

Pedidos que los agentes acordaron entre ellos, sin esperar al líder (ver §4 del modelo de trabajo). El líder puede deshacer cualquiera al validar.

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", pedido_por AS "Pedido por", estado AS "Estado"
FROM "tablero/bloques"
WHERE acuerdo = "tecnico" AND estado != "hecho"
SORT carril ASC
```

## Listo

Definido y con «Terminado cuando» aprobado; el siguiente de cada riel se toma de aquí.

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", prioridad AS "Prioridad", pedido_por AS "Pedido por"
FROM "tablero/bloques"
WHERE estado = "listo"
SORT choice(prioridad = "alta", 1, choice(prioridad = "media", 2, 3)) ASC
```

## Backlog

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", prioridad AS "Prioridad", pedido_por AS "Pedido por"
FROM "tablero/bloques"
WHERE estado = "backlog"
SORT carril ASC, choice(prioridad = "alta", 1, choice(prioridad = "media", 2, 3)) ASC
```

## Hecho

```dataview
TABLE WITHOUT ID file.link AS "Bloque", carril AS "Riel", cerrado AS "Cerrado"
FROM "tablero/bloques"
WHERE estado = "hecho"
SORT cerrado DESC
```
