const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { test } = require("node:test");
const vm = require("node:vm");

async function loadDiagram(dictionary = { entities: [], relationships: [] }) {
  const nodes = new Map();
  function makeNode() {
      const listeners = new Map();
      const attributes = new Map();
      const captures = new Set();
      return {
        value: "",
        children: [],
        dataset: {},
        style: {},
        classList: { add() {} },
        viewBox: { baseVal: { width: 1400, height: 900 } },
        clientWidth: 1400,
        clientHeight: 900,
        addEventListener(type, handler, capture = false) {
          if (!listeners.has(type)) listeners.set(type, []);
          listeners.get(type).push({ handler, capture: capture === true });
        },
        dispatch(type, data = {}) {
          let stopped = false;
          const event = {
            target: this,
            preventDefault() {},
            stopPropagation() { stopped = true; },
            ...data
          };
          const path = [];
          for (let current = event.target; current; current = current.parent) path.push(current);
          for (const current of path.slice().reverse()) {
            current.invoke(type, event, true);
            if (stopped) return;
          }
          for (const current of path) {
            current.invoke(type, event, false);
            if (stopped) return;
          }
        },
        invoke(type, event, capture) {
          for (const listener of listeners.get(type) || []) {
            if (listener.capture === capture) listener.handler(event);
          }
        },
        setAttribute(name, value) {
          attributes.set(name, value);
          if (name.startsWith("data-")) this.dataset[name.slice(5)] = value;
        },
        getAttribute(name) { return attributes.get(name); },
        appendChild(child) { child.parent = this; this.children.push(child); },
        replaceChildren() { this.children = []; },
        querySelectorAll(selector) {
          return this.children.flatMap(function (child) {
            return (child.matches(selector) ? [child] : []).concat(child.querySelectorAll(selector));
          });
        },
        matches(selector) {
          return (this.getAttribute("class") || "").split(" ").includes(selector.slice(1));
        },
        closest(selector) {
          if (this.matches(selector)) return this;
          return this.parent ? this.parent.closest(selector) : null;
        },
        setPointerCapture(id) { captures.add(id); },
        hasPointerCapture(id) { return captures.has(id); },
        releasePointerCapture(id) { captures.delete(id); },
        createSVGPoint() {
          return { matrixTransform() { return { x: this.x, y: this.y }; } };
        },
        getScreenCTM() { return { inverse() { return {}; } }; }
      };
  }
  function node(id) {
    if (!nodes.has(id)) nodes.set(id, makeNode());
    return nodes.get(id);
  }
  node("diagram").appendChild(node("scene"));
  node("scene").appendChild(node("edges"));
  node("scene").appendChild(node("nodes"));
  vm.runInNewContext(
    readFileSync(new URL("../static/ddic-diagram.js", `file://${__filename}`), "utf8"),
    {
      document: {
        querySelector: (selector) => node(selector.slice(1)),
        createElementNS: makeNode
      },
      Element: Object,
      Option: function () { return makeNode(); },
      fetch: async () => ({
        ok: true,
        json: async () => dictionary
      })
    }
  );
  await new Promise(setImmediate);
  return node;
}

test("panning and the subsequent background click preserve zoom and position", async () => {
  const node = await loadDiagram();
  const svg = node("diagram");
  const scene = node("scene");
  const initial = scene.getAttribute("transform");
  node("zoom-in").dispatch("click");
  const zoomed = scene.getAttribute("transform");
  assert.notEqual(zoomed, initial);

  svg.dispatch("pointerdown", { pointerId: 1, clientX: 100, clientY: 100 });
  svg.dispatch("pointermove", { pointerId: 1, clientX: 160, clientY: 140 });
  const panned = scene.getAttribute("transform");
  assert.notEqual(panned, zoomed);
  assert.equal(panned.split("scale")[1], zoomed.split("scale")[1]);
  svg.dispatch("pointerup", { pointerId: 1 });
  svg.dispatch("click");
  assert.equal(scene.getAttribute("transform"), panned);

  svg.dispatch("wheel", { deltaY: -100, clientX: 300, clientY: 200, preventDefault() {} });
  const scrolled = scene.getAttribute("transform");
  assert.notEqual(scrolled, panned);
  node("search").dispatch("input");
  assert.equal(scene.getAttribute("transform"), scrolled);
  node("package").dispatch("change");
  assert.equal(scene.getAttribute("transform"), scrolled);

  node("fit").dispatch("click");
  assert.equal(scene.getAttribute("transform"), initial);
});

function entity(name) {
  return { name, package: "test", fields: [] };
}

function relation(one, many) {
  return { name: `${one}_${many}`, one, many, many_property: one.toLowerCase() };
}

function cards(node) {
  return new Map(node("nodes").querySelectorAll(".entity-card").map(function (card) {
    return [card.dataset.entity, card];
  }));
}

function coordinates(card) {
  return card.getAttribute("transform").match(/-?[\d.]+/g).map(Number);
}

test("1:N layout places all parents above their children in an acyclic graph", async () => {
  const relationships = [
    relation("ZParent", "BChild"),
    relation("AParent", "BChild"),
    relation("BChild", "AGrandchild")
  ];
  const node = await loadDiagram({
    entities: ["AGrandchild", "BChild", "ZParent", "AParent", "Independent"].map(entity),
    relationships
  });
  const positions = cards(node);
  for (const edge of relationships) {
    const parent = positions.get(edge.one);
    const child = positions.get(edge.many);
    const height = Number(parent.children[0].getAttribute("height"));
    assert.ok(coordinates(parent)[1] + height < coordinates(child)[1]);
  }
  const bounds = Array.from(positions.values()).map(function (card) {
    const [x, y] = coordinates(card);
    return { x, y, width: 300, height: Number(card.children[0].getAttribute("height")) };
  });
  for (let i = 0; i < bounds.length; i++) {
    for (let j = i + 1; j < bounds.length; j++) {
      const a = bounds[i], b = bounds[j];
      assert.ok(a.x + a.width <= b.x || b.x + b.width <= a.x ||
        a.y + a.height <= b.y || b.y + b.height <= a.y);
    }
  }
});

test("cycles, self-relations and unknown endpoints produce stable, finite positions", async () => {
  const dictionary = {
    entities: ["A", "B", "C", "D"].map(entity),
    relationships: [
      relation("A", "B"), relation("B", "C"), relation("C", "A"),
      relation("A", "A"), relation("C", "D"), relation("Missing", "A")
    ]
  };
  const first = cards(await loadDiagram(dictionary));
  const second = cards(await loadDiagram(dictionary));
  assert.equal(first.size, 4);
  let downward = 0;
  for (const [name, card] of first) {
    assert.deepEqual(coordinates(card), coordinates(second.get(name)));
    assert.ok(coordinates(card).every(Number.isFinite));
  }
  for (const edge of dictionary.relationships.slice(0, 4)) {
    if (coordinates(first.get(edge.one))[1] < coordinates(first.get(edge.many))[1]) downward++;
  }
  assert.ok(downward >= 2);
});

test("dragging a table updates its links at the current zoom without moving the viewport", async () => {
  const node = await loadDiagram({
    entities: ["Parent", "Child"].map(entity),
    relationships: [relation("Parent", "Child")]
  });
  const svg = node("diagram");
  const scene = node("scene");
  node("zoom-in").dispatch("click");
  const view = scene.getAttribute("transform");
  const scale = Number(view.match(/scale\(([^)]+)\)/)[1]);
  const initialCards = cards(node);
  const parent = initialCards.get("Parent");
  const [x, y] = coordinates(parent);
  const childPosition = coordinates(initialCards.get("Child"));
  const path = node("edges").children[0].children[0].getAttribute("d");
  const moveX = -1200, moveY = -300;
  svg.dispatch("pointerdown", { target: parent, pointerId: 1, clientX: 100, clientY: 100 });
  svg.dispatch("pointermove", {
    target: parent, pointerId: 1, clientX: 100 + moveX, clientY: 100 + moveY
  });
  svg.dispatch("pointerup", { target: parent, pointerId: 1 });
  parent.dispatch("click");
  assert.equal(scene.getAttribute("transform"), view);
  assert.deepEqual(coordinates(parent), [x + moveX / scale, y + moveY / scale]);
  assert.deepEqual(coordinates(initialCards.get("Child")), childPosition);
  assert.notEqual(node("edges").children[0].children[0].getAttribute("d"), path);
  assert.equal(parent.getAttribute("class"), "entity-card");

  node("search").value = "Child";
  node("search").dispatch("input");
  node("search").value = "";
  node("search").dispatch("input");
  assert.deepEqual(coordinates(cards(node).get("Parent")), coordinates(parent));
  node("fit").dispatch("click");
  assert.deepEqual(coordinates(cards(node).get("Parent")), coordinates(parent));
  assert.notEqual(scene.getAttribute("transform"), view);

  // Ein normaler Klick nach dem Ziehen muss weiterhin eine Tabelle auswaehlen.
  cards(node).get("Parent").dispatch("click");
  assert.equal(cards(node).get("Parent").getAttribute("class"), "entity-card selected");
});
