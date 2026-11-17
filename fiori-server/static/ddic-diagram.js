"use strict";
// Generated JavaScript: edit fiori-server/frontend/ddic-diagram.ts and run npm run build.
(function () {
    "use strict";
    function required(value, description) {
        if (value === null || value === undefined) {
            throw new Error("Missing " + description);
        }
        return value;
    }
    function requiredElement(id) {
        return required(document.querySelector("#" + id), "element #" + id);
    }
    const svgNS = "http://www.w3.org/2000/svg";
    const svg = requiredElement("diagram");
    const scene = requiredElement("scene");
    const edgesLayer = requiredElement("edges");
    const nodesLayer = requiredElement("nodes");
    const message = requiredElement("message");
    const search = requiredElement("search");
    const packageFilter = requiredElement("package");
    const stats = requiredElement("stats");
    const cardWidth = 300;
    const columnGap = 90;
    const rowGap = 76;
    let dictionary;
    let selectedEntity = null;
    let scale = 1;
    let panX = 0;
    let panY = 0;
    const positions = new Map();
    let drag = null;
    let suppressClick = false;
    function element(name, attributes, text) {
        const node = document.createElementNS(svgNS, name);
        Object.keys(attributes || {}).forEach(function (key) {
            node.setAttribute(key, String(attributes[key]));
        });
        if (text !== undefined)
            node.textContent = text;
        return node;
    }
    function fieldData(field) {
        if (field.Atom)
            return { kind: "Atom", value: field.Atom };
        if (field.Lookup)
            return { kind: "Lookup", value: field.Lookup };
        if (field.Measure)
            return { kind: "Measure", value: field.Measure };
        return { kind: "Atom", value: {} };
    }
    function typeLabel(field) {
        const data = field.value;
        if (field.kind === "Lookup")
            return "→ " + (data.entity || "");
        if (field.kind === "Measure") {
            return "Edm.Decimal" + (data.precision ? "(" + data.precision + "," + (data.scale || 0) + ")" : "");
        }
        let type = data.edm_type || "Edm.String";
        if (type === "Edm.String" && data.max_length)
            return "String(" + data.max_length + ")";
        if (type === "Edm.Decimal" && data.precision)
            return type + "(" + data.precision + "," + (data.scale || 0) + ")";
        return type.replace(/^Edm\./, "");
    }
    function keyName(entity) {
        return entity.primary_key || "ID";
    }
    function keyType(entity) {
        const key = keyName(entity);
        const field = (entity.fields || []).map(fieldData).find(function (f) { return f.value.name === key; });
        return field ? typeLabel(field) : "Guid";
    }
    function foreignKeyName(relation, target) {
        const property = relation.many_property || "ID";
        return property + (target && target.primary_key ? "_code" : "_ID");
    }
    function fieldsFor(entity, relationships, entitiesByName) {
        const fields = (entity.fields || []).map(fieldData);
        const names = new Set(fields.map(function (f) { return f.value.name; }));
        const key = keyName(entity);
        if (!names.has(key)) {
            fields.unshift({ kind: "Atom", value: { name: key, edm_type: keyType(entity) }, syntheticKey: true });
            names.add(key);
        }
        relationships.forEach(function (relation) {
            if (relation.many !== entity.name || !relation.many_property)
                return;
            const target = relation.one ? entitiesByName.get(relation.one) : undefined;
            const name = foreignKeyName(relation, target);
            if (names.has(name))
                return;
            fields.push({
                kind: "Lookup",
                value: { name: name, entity: relation.one },
                foreignKey: true
            });
        });
        return fields;
    }
    function displayRows(entity, relationships, entitiesByName) {
        const key = keyName(entity);
        return fieldsFor(entity, relationships, entitiesByName).map(function (field) {
            return {
                name: field.value.name || "unbenannt",
                type: typeLabel(field),
                primaryKey: field.value.name === key,
                foreignKey: Boolean(field.foreignKey) || field.kind === "Lookup"
            };
        });
    }
    function entityMatches(entity, query, selectedPackage) {
        if (selectedPackage && (entity.package || "") !== selectedPackage)
            return false;
        if (!query)
            return true;
        const text = [entity.name, entity.type_name, entity.package]
            .concat((entity.fields || []).map(function (field) {
            const data = fieldData(field).value;
            return [data.name, data.label, data.edm_type].join(" ");
        })).join(" ").toLocaleLowerCase();
        return text.includes(query);
    }
    function filteredEntities() {
        const query = search.value.trim().toLocaleLowerCase();
        const selectedPackage = packageFilter.value;
        return required(dictionary, "DDIC").entities.filter(function (entity) {
            return entityMatches(entity, query, selectedPackage);
        }).sort(compareEntities);
    }
    function compareEntities(a, b) {
        return (a.package || "").localeCompare(b.package || "") || a.name.localeCompare(b.name);
    }
    function svgPoint(event) {
        const point = svg.createSVGPoint();
        point.x = event.clientX;
        point.y = event.clientY;
        return point.matrixTransform(required(svg.getScreenCTM(), "SVG transformation matrix").inverse());
    }
    function applyTransform() {
        scene.setAttribute("transform", "translate(" + panX + " " + panY + ") scale(" + scale + ")");
    }
    function fitView(visiblePositions) {
        const view = svg.viewBox.baseVal;
        const cards = Array.from(visiblePositions.values());
        const left = cards.length ? Math.min(...cards.map(function (p) { return p.x; })) : 0;
        const top = cards.length ? Math.min(...cards.map(function (p) { return p.y; })) : 0;
        const right = Math.max(...cards.map(function (p) { return p.x + p.width; }), left + 500);
        const bottom = Math.max(...cards.map(function (p) { return p.y + p.height; }), top + 350);
        const width = right - left;
        const height = bottom - top;
        scale = Math.min((view.width - 80) / (width + 80), (view.height - 80) / (height + 80), 1);
        panX = (view.width - width * scale) / 2 - left * scale;
        panY = (view.height - height * scale) / 2 - top * scale;
        applyTransform();
    }
    function arrangeEntities(entities, relationships, rowsByEntity) {
        const remaining = new Set(entities.map(function (entity) { return entity.name; }));
        const incoming = new Map(entities.map(function (entity) { return [entity.name, new Set()]; }));
        const outgoing = new Map(entities.map(function (entity) { return [entity.name, new Set()]; }));
        const levels = new Map(entities.map(function (entity) { return [entity.name, 0]; }));
        relationships.forEach(function (relation) {
            if (!relation.one || !relation.many || relation.one === relation.many ||
                !remaining.has(relation.one) || !remaining.has(relation.many))
                return;
            required(incoming.get(relation.many), "incoming relationships").add(relation.one);
            required(outgoing.get(relation.one), "outgoing relationships").add(relation.many);
        });
        const layers = [];
        while (remaining.size) {
            // Bei Zyklen den Knoten mit den wenigsten verbleibenden Eltern zuerst platzieren.
            const name = Array.from(remaining).reduce(function (best, candidate) {
                return required(incoming.get(candidate), "incoming relationships").size <
                    required(incoming.get(best), "incoming relationships").size ? candidate : best;
            });
            remaining.delete(name);
            const level = required(levels.get(name), "entity level");
            if (!layers[level])
                layers[level] = [];
            layers[level].push(name);
            required(outgoing.get(name), "outgoing relationships").forEach(function (child) {
                if (!remaining.has(child))
                    return;
                required(incoming.get(child), "incoming relationships").delete(name);
                levels.set(child, Math.max(required(levels.get(child), "entity level"), level + 1));
            });
        }
        positions.clear();
        const maxColumns = Math.max(1, Math.min(4, Math.ceil(Math.sqrt(entities.length))));
        let y = 40;
        layers.forEach(function (names) {
            for (let start = 0; start < names.length; start += maxColumns) {
                const row = names.slice(start, start + maxColumns);
                let height = 0;
                row.forEach(function (name, column) {
                    const cardHeight = 68 + Math.max(required(rowsByEntity.get(name), "entity fields").length, 1) * 29;
                    positions.set(name, {
                        x: 40 + column * (cardWidth + columnGap),
                        y: y,
                        width: cardWidth,
                        height: cardHeight
                    });
                    height = Math.max(height, cardHeight);
                });
                y += height + rowGap;
            }
        });
    }
    function addCard(entity, rows, position) {
        const headerHeight = 58;
        const rowHeight = 29;
        const cardHeight = headerHeight + Math.max(rows.length, 1) * rowHeight + 10;
        position.width = cardWidth;
        position.height = cardHeight;
        const card = element("g", {
            class: "entity-card" + (selectedEntity === entity.name ? " selected" : ""),
            transform: "translate(" + position.x + " " + position.y + ")",
            "data-entity": entity.name
        });
        card.appendChild(element("rect", { class: "card-bg", x: 0, y: 0, width: cardWidth, height: cardHeight, rx: 8 }));
        card.appendChild(element("path", {
            class: "card-header",
            d: "M8 0 H" + (cardWidth - 8) + " Q" + cardWidth + " 0 " + cardWidth + " 8 V" + headerHeight + " H0 V8 Q0 0 8 0Z"
        }));
        card.appendChild(element("text", { class: "entity-name", x: 14, y: 24 }, entity.name));
        const subtitle = [entity.type_name, entity.package].filter(Boolean).join(" · ");
        card.appendChild(element("text", { class: "entity-package", x: 14, y: 44 }, subtitle));
        if (!rows.length) {
            card.appendChild(element("text", { class: "muted", x: 14, y: headerHeight + 27 }, "Keine Felder"));
        }
        rows.forEach(function (row, index) {
            const y = headerHeight + index * rowHeight;
            card.appendChild(element("line", { class: "field-row", x1: 0, y1: y, x2: cardWidth, y2: y }));
            if (row.primaryKey || row.foreignKey) {
                const badge = row.primaryKey ? "PK" : "FK";
                const badgeWidth = 25;
                card.appendChild(element("rect", {
                    class: "badge" + (row.foreignKey && !row.primaryKey ? " fk" : ""),
                    x: 10, y: y + 7, width: badgeWidth, height: 15, rx: 4
                }));
                card.appendChild(element("text", {
                    class: "badge-text" + (row.foreignKey && !row.primaryKey ? " fk" : ""),
                    x: 22.5, y: y + 18
                }, badge));
            }
            const fieldX = row.primaryKey || row.foreignKey ? 43 : 14;
            const fieldText = element("text", { class: "field-name", x: fieldX, y: y + 19 }, row.name);
            fieldText.appendChild(element("title", {}, row.name));
            card.appendChild(fieldText);
            const typeText = element("text", { class: "field-type", x: cardWidth - 12, y: y + 19 }, row.type);
            typeText.appendChild(element("title", {}, row.type));
            card.appendChild(typeText);
        });
        card.addEventListener("click", function (event) {
            event.stopPropagation();
            selectedEntity = selectedEntity === entity.name ? null : entity.name;
            render();
        });
        nodesLayer.appendChild(card);
        return position;
    }
    function edgePath(source, target) {
        const sourceX = source.x + source.width / 2;
        const sourceY = source.y + source.height / 2;
        const targetX = target.x + target.width / 2;
        const targetY = target.y + target.height / 2;
        if (target.y < source.y + source.height && source.y < target.y + target.height) {
            const fromX = targetX > sourceX ? source.x + source.width : source.x;
            const toX = targetX > sourceX ? target.x : target.x + target.width;
            const bend = (toX - fromX) / 2;
            return {
                d: "M" + fromX + " " + sourceY + " C" + (fromX + bend) + " " + sourceY + " " + (toX - bend) + " " + targetY + " " + toX + " " + targetY,
                labelX: (fromX + toX) / 2,
                labelY: (sourceY + targetY) / 2 - 7,
                oneX: fromX + 8,
                oneY: sourceY - 5,
                manyX: toX - 18,
                manyY: targetY - 5
            };
        }
        const fromY = targetY > sourceY ? source.y + source.height : source.y;
        const toY = targetY > sourceY ? target.y : target.y + target.height;
        const bendY = (toY - fromY) / 2;
        return {
            d: "M" + sourceX + " " + fromY + " C" + sourceX + " " + (fromY + bendY) + " " + targetX + " " + (toY - bendY) + " " + targetX + " " + toY,
            labelX: (sourceX + targetX) / 2 + 8,
            labelY: (fromY + toY) / 2,
            oneX: sourceX + 8,
            oneY: fromY + 13,
            manyX: targetX + 8,
            manyY: toY - 6
        };
    }
    function drawRelationships(relationships, visiblePositions) {
        edgesLayer.replaceChildren();
        relationships.forEach(function (relation) {
            if (!relation.one || !relation.many)
                return;
            const source = visiblePositions.get(relation.one);
            const target = visiblePositions.get(relation.many);
            if (!source || !target || relation.one === relation.many)
                return;
            const path = edgePath(source, target);
            const edge = element("g", {
                class: "relationship" + (relation.owned ? " owned" : ""),
                "data-one": relation.one,
                "data-many": relation.many
            });
            edge.appendChild(element("path", {
                class: "edge" + (relation.owned ? " owned" : ""),
                d: path.d
            }));
            edge.appendChild(element("text", { class: "edge-end", x: path.oneX, y: path.oneY }, "1"));
            edge.appendChild(element("text", { class: "edge-end", x: path.manyX, y: path.manyY }, "N"));
            edge.appendChild(element("text", { class: "edge-label", x: path.labelX, y: path.labelY }, relation.name || ""));
            if (selectedEntity && relation.one !== selectedEntity && relation.many !== selectedEntity) {
                edge.style.opacity = "0.12";
            }
            edgesLayer.appendChild(edge);
        });
    }
    function visiblePositions() {
        return new Map(filteredEntities().map(function (entity) {
            return [entity.name, required(positions.get(entity.name), "entity position")];
        }));
    }
    function render(fit = false) {
        if (!dictionary)
            return;
        nodesLayer.replaceChildren();
        const entities = filteredEntities();
        const relationships = dictionary.relationships || [];
        const entitiesByName = new Map(dictionary.entities.map(function (entity) { return [entity.name, entity]; }));
        const rowsByEntity = new Map(entities.map(function (entity) {
            return [entity.name, displayRows(entity, relationships, entitiesByName)];
        }));
        if (!positions.size) {
            const allEntities = dictionary.entities.slice().sort(compareEntities);
            const allRows = new Map(allEntities.map(function (entity) {
                return [entity.name, displayRows(entity, relationships, entitiesByName)];
            }));
            arrangeEntities(allEntities, relationships, allRows);
        }
        const visible = visiblePositions();
        if (selectedEntity && !visible.has(selectedEntity))
            selectedEntity = null;
        drawRelationships(relationships, visible);
        entities.forEach(function (entity) {
            addCard(entity, required(rowsByEntity.get(entity.name), "entity fields"), required(positions.get(entity.name), "entity position"));
        });
        if (selectedEntity) {
            nodesLayer.querySelectorAll(".entity-card").forEach(function (card) {
                const connected = relationships.some(function (relation) {
                    return (relation.one === selectedEntity && relation.many === card.dataset.entity) ||
                        (relation.many === selectedEntity && relation.one === card.dataset.entity);
                });
                if (card.dataset.entity !== selectedEntity && !connected)
                    card.style.opacity = "0.35";
            });
        }
        stats.textContent = entities.length + " / " + dictionary.entities.length + " Entitäten · " +
            relationships.filter(function (relation) {
                return relation.one && relation.many && visible.has(relation.one) && visible.has(relation.many);
            }).length + " Beziehungen";
        if (fit) {
            fitView(visible);
        }
        else {
            applyTransform();
        }
    }
    function setZoom(nextScale, point) {
        const next = Math.max(Math.min(scale, 0.18), Math.min(3, nextScale));
        const ratio = next / scale;
        panX = point.x - (point.x - panX) * ratio;
        panY = point.y - (point.y - panY) * ratio;
        scale = next;
        applyTransform();
    }
    search.addEventListener("input", function () {
        render();
    });
    packageFilter.addEventListener("change", function () {
        render();
    });
    requiredElement("fit").addEventListener("click", function () {
        render(true);
    });
    requiredElement("zoom-in").addEventListener("click", function () {
        const view = svg.viewBox.baseVal;
        setZoom(scale * 1.2, { x: view.width / 2, y: view.height / 2 });
    });
    requiredElement("zoom-out").addEventListener("click", function () {
        const view = svg.viewBox.baseVal;
        setZoom(scale / 1.2, { x: view.width / 2, y: view.height / 2 });
    });
    svg.addEventListener("wheel", function (event) {
        event.preventDefault();
        setZoom(scale * (event.deltaY < 0 ? 1.1 : 1 / 1.1), svgPoint(event));
    }, { passive: false });
    svg.addEventListener("pointerdown", function (event) {
        if (drag || (event.button !== undefined && event.button !== 0))
            return;
        const card = event.target instanceof Element ? event.target.closest(".entity-card") : null;
        const capture = card || svg;
        suppressClick = false;
        drag = {
            pointerId: event.pointerId,
            capture: capture,
            card: card,
            startX: event.clientX,
            startY: event.clientY,
            last: svgPoint(event),
            moved: false
        };
        capture.setPointerCapture(event.pointerId);
    });
    svg.addEventListener("pointermove", function (event) {
        if (!drag || drag.pointerId !== event.pointerId)
            return;
        if (!drag.moved && Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) < 3)
            return;
        drag.moved = true;
        const point = svgPoint(event);
        const dx = point.x - drag.last.x;
        const dy = point.y - drag.last.y;
        drag.last = point;
        if (drag.card) {
            const name = required(drag.card.dataset.entity, "dragged entity name");
            const position = required(positions.get(name), "dragged entity position");
            position.x += dx / scale;
            position.y += dy / scale;
            drag.card.setAttribute("transform", "translate(" + position.x + " " + position.y + ")");
            drawRelationships(required(dictionary, "DDIC").relationships || [], visiblePositions());
        }
        else {
            panX += dx;
            panY += dy;
            applyTransform();
        }
    });
    function endDrag(event) {
        if (!drag || drag.pointerId !== event.pointerId)
            return;
        const capture = drag.capture;
        suppressClick = drag.moved;
        drag = null;
        if (capture.hasPointerCapture(event.pointerId))
            capture.releasePointerCapture(event.pointerId);
    }
    svg.addEventListener("pointerup", endDrag);
    svg.addEventListener("pointercancel", endDrag);
    svg.addEventListener("lostpointercapture", endDrag);
    svg.addEventListener("click", function (event) {
        if (!suppressClick)
            return;
        suppressClick = false;
        event.preventDefault();
        event.stopPropagation();
    }, true);
    svg.addEventListener("click", function (event) {
        if (event.target === svg || event.target === scene) {
            selectedEntity = null;
            render();
        }
    });
    message.style.display = "block";
    fetch("/ddic/diagram.json", { headers: { Accept: "application/json" } })
        .then(function (response) {
        if (!response.ok)
            throw new Error("HTTP " + response.status);
        return response.json();
    })
        .then(function (data) {
        if (!Array.isArray(data.entities) || !Array.isArray(data.relationships)) {
            throw new Error("Ungültiges DDIC-Format");
        }
        dictionary = data;
        const packages = Array.from(new Set(data.entities.map(function (entity) {
            return entity.package || "";
        }).filter(Boolean))).sort();
        packages.forEach(function (name) {
            packageFilter.appendChild(new Option(name, name));
        });
        message.style.display = "none";
        render(true);
    })
        .catch(function (error) {
        message.textContent = "DDIC konnte nicht geladen werden: " + error.message;
        message.classList.add("error");
    });
})();
