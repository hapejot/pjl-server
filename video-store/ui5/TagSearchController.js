sap.ui.define(["video/ext/TagSearch", "sap/m/MessageToast"], function (TagSearch, MessageToast) {
    "use strict";

    // Controller-Erweiterung der ListReport-Seite (Manifest: sap.ui5/extends).
    // - haengt das Modell "tagsearch" an die View
    // - uebernimmt Video-IDs als ID-Bedingung in die Filterleiste der Standardliste

    function findTabBar(view) {
        var found = view.findAggregatedObjects(true, function (c) {
            return c.isA && c.isA("sap.m.IconTabBar");
        });
        return found[0];
    }

    function applyIds(view, ids) {
        var filterBar = view.getController()._getFilterBarControl
            ? view.getController()._getFilterBarControl()
            : null;
        if (!filterBar) {
            MessageToast.show("Filterleiste nicht gefunden");
            return;
        }
        var conditions = ids.map(function (id) {
            return { operator: "EQ", values: [id], validated: "Validated" };
        });
        var cond = {};
        cond.ID = conditions;
        filterBar.setFilterConditions(cond);

        var applied = (filterBar.getFilterConditions() || {}).ID || [];
        if (ids.length && applied.length !== ids.length) {
            MessageToast.show("ID-Filter wird von der Filterleiste nicht unterstuetzt");
            return;
        }
        var api = filterBar.getParent();
        if (api && api.triggerSearch) { api.triggerSearch(); }

        var bar = findTabBar(view);
        if (bar && bar.getItems().length) {
            bar.setSelectedKey(bar.getItems()[0].getKey());
        }
        if (ids.length) {
            MessageToast.show(ids.length + " Videos in der Liste");
        }
    }

    return {
        onInit: function () {
            var view = this.getView();
            view.setModel(TagSearch.getModel(), "tagsearch");
            TagSearch.setView(view);
            TagSearch.setListHandler(function (ids) { applyIds(view, ids); });
        }
    };
});
