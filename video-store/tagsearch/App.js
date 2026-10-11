/*
 * Tag-Suche: mehrere Tags waehlen (UND), Auswahl verengt die angebotenen Tags.
 *
 * Datenquellen:
 *   GET /ext/video/tags/facets?selected=<ids>&top=20   -> waehlbare Tags mit Trefferzahl
 *   GET /ext/video/tags/videos?selected=<ids>&skip&top -> Trefferliste (sortiert nach Name)
 *
 * Jede Auswahl laedt Facetten und Trefferliste neu. Veraltete Antworten werden
 * ueber eine Laufnummer verworfen.
 */
sap.ui.define([
    "sap/m/App",
    "sap/m/Page",
    "sap/m/VBox",
    "sap/m/HBox",
    "sap/m/MultiComboBox",
    "sap/m/MessageStrip",
    "sap/m/Table",
    "sap/m/Button",
    "sap/m/Column",
    "sap/m/ColumnListItem",
    "sap/m/Image",
    "sap/m/Text",
    "sap/m/Title",
    "sap/ui/core/Item",
    "sap/ui/model/json/JSONModel"
], function (App, Page, VBox, HBox, MultiComboBox, MessageStrip, Table, Button, Column,
             ColumnListItem, Image, Text, Title, Item, JSONModel) {
    "use strict";

    var TOP_FACETS = 20;
    var PAGE_SIZE = 100;
    var FACETS_URL = "/ext/video/tags/facets";
    var VIDEOS_URL = "/ext/video/tags/videos";

    var model = new JSONModel({
        facets: [],          // Eintraege fuer die Auswahlbox: gewaehlte zuerst, dann Vorschlaege
        matches: 0,
        resultCount: 0,
        results: [],
        hasMore: false,
        error: ""
    });
    var names = {};          // Tag-ID -> Name, fuer gewaehlte Tags ueber Antworten hinweg
    var sequence = 0;        // Laufnummer gegen veraltete Antworten

    var combo = new MultiComboBox({
        width: "100%",
        placeholder: "Tags waehlen (alle gewaehlten muessen zutreffen)",
        selectionChange: onSelectionChange
    });
    combo.bindItems({
        path: "/facets",
        template: new Item({ key: "{id}", text: "{name} ({count})" })
    });

    var table = new Table({
        noDataText: "Keine Videos gefunden",
        columns: [
            new Column({ width: "120px", header: new Text({ text: "Vorschau" }) }),
            new Column({ header: new Text({ text: "Name" }) }),
            new Column({ width: "140px", hAlign: "End", header: new Text({ text: "Dauer" }) })
        ]
    });
    table.bindItems({
        path: "/results",
        template: new ColumnListItem({
            cells: [
                new Image({
                    src: "{= '/ext/video/thumbnail/' + ${ID} + '/0' }",
                    width: "96px",
                    densityAware: false
                }),
                new Text({ text: "{name}" }),
                new Text({ text: "{durationText}" })
            ]
        })
    });

    var moreButton = new Button({
        text: "Weitere Videos laden",
        icon: "sap-icon://navigation-down-arrow",
        visible: "{/hasMore}",
        press: function () { loadMore(); }
    });

    var summary = new Title({ text: "{/resultCount} Videos" });
    summary.setModel(model);
    var matchInfo = new Text({ text: "{/matches} Treffer fuer die Auswahl" });
    var errorStrip = new MessageStrip({
        type: "Error",
        showIcon: true,
        visible: "{= !!${/error} }"
    }).bindProperty("text", "/error");

    var page = new Page({
        title: "Tag-Suche",
        showNavButton: false,
        content: [
            new VBox({
                class: "sapUiSmallMargin",
                width: "100%",
                items: [
                    errorStrip,
                    combo,
                    new HBox({ class: "sapUiSmallMarginTop", items: [summary] }),
                    matchInfo
                ]
            }),
            table,
            new VBox({
                class: "sapUiSmallMargin",
                alignItems: "Center",
                items: [moreButton]
            })
        ]
    });

    page.setModel(model);
    combo.setModel(model);
    table.setModel(model);

    function selectedIds() {
        return combo.getSelectedKeys();
    }

    function onSelectionChange() {
        refresh(selectedIds());
    }

    function refresh(ids) {
        var seq = ++sequence;
        loadFacets(ids, seq);
        loadResults(ids, seq, 0, false);
    }

    function loadMore() {
        loadResults(selectedIds(), sequence, model.getProperty("/results").length, true);
    }

    function loadFacets(ids, seq) {
        var url = FACETS_URL + "?top=" + TOP_FACETS;
        if (ids.length) {
            url += "&selected=" + ids.map(encodeURIComponent).join(",");
        }
        fetch(url, { headers: { Accept: "application/json" } })
            .then(function (res) {
                if (!res.ok) {
                    return res.json().catch(function () { return {}; }).then(function (body) {
                        throw new Error(body.error || ("HTTP " + res.status));
                    });
                }
                return res.json();
            })
            .then(function (data) {
                if (seq !== sequence) { return; }
                // Gewaehlte Tags bleiben sichtbar, auch wenn sie nicht mehr angeboten werden.
                var entries = [];
                ids.forEach(function (id) {
                    entries.push({ id: id, name: names[id] || id, count: "" });
                });
                data.tags.forEach(function (t) {
                    names[t.id] = t.name;
                    if (ids.indexOf(t.id) < 0) {
                        entries.push({ id: t.id, name: t.name, count: t.count });
                    }
                });
                model.setProperty("/facets", entries.map(function (e) {
                    return { id: e.id, name: e.name, count: e.count === "" ? "" : e.count };
                }));
                combo.setSelectedKeys(ids);
                model.setProperty("/matches", data.matches);
                model.setProperty("/error", "");
            })
            .catch(function (err) {
                if (seq !== sequence) { return; }
                model.setProperty("/error", "Tags konnten nicht geladen werden: " + err.message);
            });
    }

    /*
     * Trefferliste aus der Erweiterung (Index, nach Name sortiert, seitenweise).
     * Der OData-Filter ueber _VideoTags/any ist im Store zu langsam fuer die Produktivdaten.
     */
    function loadResults(ids, seq, skip, append) {
        var url = VIDEOS_URL + "?top=" + PAGE_SIZE + "&skip=" + skip;
        if (ids.length) {
            url += "&selected=" + ids.map(encodeURIComponent).join(",");
        }
        fetch(url, { headers: { Accept: "application/json" } })
            .then(function (res) {
                if (!res.ok) {
                    return res.json().catch(function () { return {}; }).then(function (body) {
                        throw new Error(body.error || ("HTTP " + res.status));
                    });
                }
                return res.json();
            })
            .then(function (data) {
                if (seq !== sequence) { return; }
                var rows = (data.value || []).map(function (r) {
                    return {
                        ID: r.ID,
                        name: r.name,
                        durationText: r.duration_seconds ? Math.round(r.duration_seconds / 60) + " min" : ""
                    };
                });
                var all = append ? model.getProperty("/results").concat(rows) : rows;
                model.setProperty("/results", all);
                model.setProperty("/resultCount", data.count);
                model.setProperty("/hasMore", all.length < data.count);
            })
            .catch(function (err) {
                if (seq !== sequence) { return; }
                model.setProperty("/error", "Videos konnten nicht geladen werden: " + err.message);
            });
    }

    // Wird ueber data-sap-ui-onInit="module:tagsearch/App" geladen: Modul-Body startet die App.
    new App({ pages: [page] }).placeAt("content");
    refresh([]);
    return {};
});
