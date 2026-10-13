/*
 * Tag-Suche der Video-Listenseite (Tab, siehe fragments/TagSearch.fragment.xml).
 *
 * - Suchfeld: Tags werden nach Teilstring gefiltert (serverseitig, ohne Gross-/Kleinschreibung).
 * - Tag-Buttons: mehrere Tags waehlen (UND), die Auswahl verengt die angebotenen Tags.
 * - Videos sind klickbar und oeffnen die Object Page.
 *
 * Datenquellen (src/tags.rs):
 *   GET /ext/video/tags/facets?selected=<ids>&top=50&q=<text>
 *   GET /ext/video/tags/videos?selected=<ids>&skip&top=100
 *
 * Zustand im Modell "tagsearch", das die Controller-Erweiterung an die View haengt
 * (ui5/TagSearchController.js). Veraltete Antworten werden ueber eine Laufnummer verworfen.
 */
sap.ui.define(["sap/base/util/ObjectPath", "sap/ui/model/json/JSONModel", "sap/m/MessageToast", "sap/ui/core/UIComponent"], function (ObjectPath, JSONModel, MessageToast, UIComponent) {
    "use strict";

    var TOP_FACETS = 50;
    var PAGE_SIZE = 100;
    var FACETS_URL = "/ext/video/tags/facets";
    var VIDEOS_URL = "/ext/video/tags/videos";
    var OBJECT_ROUTE = "VideoObjectPage";
    var SEMANTIC_OBJECT = "Video";   // Inbound-Intent aus fiori-server/src/flp.rs
    var SEMANTIC_ACTION = "display";
    var DEBOUNCE_MS = 250;

    var model = new JSONModel({
        facets: [],
        matches: 0,
        results: [],
        hasMore: false,
        selectedCount: 0,
        error: ""
    });
    var selected = [];       // Gewaehlte Tag-IDs, in Auswahlreihenfolge
    var names = {};          // Tag-ID -> Name, auch fuer gerade nicht angebotene Tags
    var query = "";          // Aktueller Suchtext
    var sequence = 0;        // Laufnummer gegen veraltete Antworten
    var timer = null;        // Debounce des Suchfelds
    var view = null;         // View der Listenseite, gesetzt von der Controller-Erweiterung
    var playlist = [];       // IDs der Trefferreihenfolge beim letzten Klick aus der Tag-Suche
    var autoplayNext = false; // Naechstes geladenes Video direkt starten
    var listHandler = null;  // Uebergibt IDs an die Standardliste (Controller-Erweiterung)
    var LIST_LIMIT = 200;    // Obergrenze fuer den ID-Filter der Standardliste

    function getJson(url) {
        return fetch(url, { headers: { Accept: "application/json" } }).then(function (res) {
            if (!res.ok) {
                return res.json().catch(function () { return {}; }).then(function (body) {
                    throw new Error(body.error || ("HTTP " + res.status));
                });
            }
            return res.json();
        });
    }

    function ids(list) {
        return list.length ? "&selected=" + list.map(encodeURIComponent).join(",") : "";
    }

    function refresh() {
        var seq = ++sequence;
        loadFacets(selected.slice(), seq);
        loadResults(selected.slice(), seq, 0, false);
    }

    function loadFacets(list, seq) {
        var url = FACETS_URL + "?top=" + TOP_FACETS + ids(list);
        if (query) {
            url += "&q=" + encodeURIComponent(query);
        }
        getJson(url)
            .then(function (data) {
                if (seq !== sequence) { return; }
                var entries = [];
                // Gewaehlte Tags bleiben als Chips sichtbar, damit man sie abwaehlen kann.
                list.forEach(function (id) {
                    entries.push({ id: id, name: names[id] || id, count: "", selected: true });
                });
                data.tags.forEach(function (t) {
                    names[t.id] = t.name;
                    if (list.indexOf(t.id) < 0) {
                        entries.push({ id: t.id, name: t.name, count: t.count, selected: false });
                    }
                });
                model.setProperty("/facets", entries.map(function (e) {
                    e.label = e.count === "" ? e.name : e.name + " (" + e.count + ")";
                    return e;
                }));
                model.setProperty("/matches", data.matches);
                model.setProperty("/selectedCount", list.length);
                model.setProperty("/error", "");
            })
            .catch(function (err) {
                if (seq !== sequence) { return; }
                model.setProperty("/error", "Tags konnten nicht geladen werden: " + err.message);
            });
    }

    /* Trefferliste aus dem Index (nach Name sortiert, seitenweise). */
    function loadResults(list, seq, skip, append) {
        getJson(VIDEOS_URL + "?top=" + PAGE_SIZE + "&skip=" + skip + ids(list))
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
                model.setProperty("/hasMore", all.length < data.count);
            })
            .catch(function (err) {
                if (seq !== sequence) { return; }
                model.setProperty("/error", "Videos konnten nicht geladen werden: " + err.message);
            });
    }

    // Semantic Navigation (Intent Video-display) auf die Object Page eines Videos.
    // Ohne FLP faellt sie auf den Router der View zurueck.
    function navigateTo(id) {
        var key = "ID=" + id + ",IsActiveEntity=true";
        try {
            if (sap.ushell && sap.ushell.Container) {
                var nav = sap.ushell.Container.getService("CrossApplicationNavigation");
                nav.toExternal({
                    target: { semanticObject: SEMANTIC_OBJECT, action: SEMANTIC_ACTION },
                    appSpecificRoute: "&/VideoSet(" + key + ")"
                });
                return;
            }
            var router = view ? UIComponent.getRouterFor(view) : null;
            if (!router) {
                MessageToast.show("Navigation nicht verfuegbar");
                return;
            }
            router.navTo(OBJECT_ROUTE, { key: key });
        } catch (e) {
            console.error("[TagSearch] Navigation fehlgeschlagen", e);
            MessageToast.show("Navigation fehlgeschlagen: " + e.message);
        }
    }

    var TAG_SEARCH = {
        // Suchfeld: Eingabe verzoegert uebernehmen, dann Tags neu filtern.
        search: function (oEvent) {
            query = (oEvent.getParameter("newValue") || "").trim();
            clearTimeout(timer);
            timer = setTimeout(refresh, DEBOUNCE_MS);
        },
        // ToggleButton-Klick: Tag aufnehmen oder entfernen, dann neu laden.
        toggle: function (oEvent) {
            var src = oEvent.getSource();
            var id = src.data("id");
            var at = selected.indexOf(id);
            if (src.getPressed() && at < 0) {
                selected.push(id);
            } else if (!src.getPressed() && at >= 0) {
                selected.splice(at, 1);
            }
            refresh();
        },
        clear: function () {
            selected = [];
            refresh();
        },
        more: function () {
            loadResults(selected.slice(), sequence, model.getProperty("/results").length, true);
        },
        // Klick auf ein Video: Trefferreihenfolge als Wiedergabeliste merken, dann navigieren.
        open: function (oEvent) {
            var src = oEvent.getSource();
            var id = (src.getParent() && src.getParent().data("id")) || src.data("id");
            if (!id) {
                MessageToast.show("Video-ID fehlt");
                return;
            }
            playlist = model.getProperty("/results").map(function (r) { return r.ID; });
            navigateTo(id);
        },
        // Ende eines Videos in der Object Page: naechstes Video der Wiedergabeliste oeffnen
        // und dort automatisch starten.
        playNext: function (currentId) {
            var at = playlist.indexOf(currentId);
            if (at < 0 || at + 1 >= playlist.length) {
                MessageToast.show("Ende der Trefferliste");
                return;
            }
            autoplayNext = true;
            navigateTo(playlist[at + 1]);
        },
        // Liefert true, wenn das aktuell geladene Video automatisch starten soll (einmalig).
        consumeAutoplay: function () {
            var flag = autoplayNext;
            autoplayNext = false;
            return flag;
        },
        // Alle Treffer der Auswahl (bis LIST_LIMIT) in die Standardliste uebernehmen.
        showInList: function () {
            var url = VIDEOS_URL + "?top=" + LIST_LIMIT + "&skip=0" + ids(selected);
            getJson(url)
                .then(function (data) {
                    var list = (data.value || []).map(function (r) { return r.ID; });
                    if (data.count > list.length) {
                        MessageToast.show("Nur die ersten " + list.length + " von " + data.count + " Videos uebernommen");
                    }
                    if (listHandler) { listHandler(list); }
                })
                .catch(function (err) {
                    MessageToast.show("Videos konnten nicht geladen werden: " + err.message);
                });
        },
        // Filter der Standardliste entfernen.
        resetList: function () {
            if (listHandler) { listHandler([]); }
        },
        setListHandler: function (fn) { listHandler = fn; },
        getModel: function () { return model; },
        setView: function (v) { view = v; },
        init: refresh
    };

    // Globaler Name fuer die String-Handler im Fragment (video.ext.TagSearch.toggle).
    ObjectPath.set("video.ext.TagSearch", TAG_SEARCH);
    TAG_SEARCH.init();
    return TAG_SEARCH;
});
