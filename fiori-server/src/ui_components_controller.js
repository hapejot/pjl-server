/*
 * Generische Controller-Erweiterung fuer Komponenten (fiori.ext.ComponentsController).
 *
 * Jede Verwendung (z. B. Zeitraum) steht als Formular im Abschnitt der Object Page. Das
 * Fragment traegt seine Konfiguration als customData "timeRange", Format:
 *   instance=termin;start=starts_at;end=ends_at;duration=duration_minutes;step=5
 *
 * - Beginn/Ende: nach einer Aenderung werden alle Felder der Verwendung neu geladen
 *   (die Regeln laufen serverseitig im Komponenten-Skript).
 * - Dauer: Eingabe als Text ("2:30", "2h", "2h 30", "30 min", "30"). Beim Verlassen des
 *   Feldes wird in Minuten umgerechnet und per Property-Bindung geschrieben.
 */
sap.ui.define([], function () {
    "use strict";

    function parseConfig(sValue) {
        var o = {};
        String(sValue || "").split(";").forEach(function (part) {
            var i = part.indexOf("=");
            if (i > 0) {
                o[part.slice(0, i)] = part.slice(i + 1);
            }
        });
        return o;
    }

    // Liefert Minuten oder null, wenn die Eingabe nicht verstanden wird.
    function parseDuration(sText) {
        var s = String(sText || "").trim().toLowerCase();
        if (s === "") {
            return null;
        }
        var m;
        m = /^(\d+):(\d{1,2})$/.exec(s);
        if (m) {
            return Number(m[2]) > 59 ? null : Number(m[1]) * 60 + Number(m[2]);
        }
        m = /^(\d+)\s*h(?:\s*(\d+)\s*(?:min|m)?)?$/.exec(s);
        if (m) {
            var mm = m[2] ? Number(m[2]) : 0;
            return mm > 59 ? null : Number(m[1]) * 60 + mm;
        }
        m = /^(\d+)\s*(?:min|m)$/.exec(s);
        if (m) {
            return Number(m[1]);
        }
        m = /^(\d+)$/.exec(s);
        if (m) {
            return Number(m[1]);
        }
        return null;
    }

    function fieldsOf(cfg) {
        return [cfg.start, cfg.end, cfg.duration];
    }

    function refresh(oView, cfg) {
        var oCtx = oView.getBindingContext();
        if (!oCtx) {
            return;
        }
        oCtx.requestSideEffects(fieldsOf(cfg)).catch(function (oErr) {
            console.error("ComponentsController: Nachladen fehlgeschlagen", oErr);
        });
    }

    function commitDuration(oView, cfg, oInput) {
        var oCtx = oView.getBindingContext();
        if (!oCtx) {
            return;
        }
        var nMinutes = parseDuration(oInput.getValue());
        if (nMinutes === null) {
            oInput.setValueState("Error");
            oInput.setValueStateText("Format: 2:30, 2h, 2h 30 min oder 30 min");
            return;
        }
        oInput.setValueState("None");
        // V4: Eine Property-Bindung darf erst geaendert werden, nachdem ihr Wert gelesen wurde.
        var oBinding = oCtx.getModel().bindProperty(cfg.duration, oCtx);
        oBinding.requestValue().then(function () {
            oBinding.setValue(nMinutes);
            setTimeout(function () { refresh(oView, cfg); }, 0);
        }, function (oErr) {
            console.error("ComponentsController: Dauer lesen fehlgeschlagen", oErr);
        });
    }

    function bindingPath(oControl) {
        var oBinding = oControl.getBinding("value");
        return oBinding ? oBinding.getPath() : null;
    }

    function attach(oView) {
        var aForms = oView.findAggregatedObjects(true, function (oControl) {
            return typeof oControl.getCustomData === "function" && oControl.getCustomData().some(function (cd) {
                return cd.getKey() === "timeRange";
            });
        });
        aForms.forEach(function (oForm) {
            var cfg = parseConfig(oForm.getCustomData().filter(function (cd) {
                return cd.getKey() === "timeRange";
            })[0].getValue());
            var aFields = fieldsOf(cfg);
            oForm.findAggregatedObjects(true, function (oControl) {
                if (typeof oControl.getBinding !== "function" || oControl.__componentAttached) {
                    return false;
                }
                var sPath = bindingPath(oControl);
                if (!sPath) {
                    return false;
                }
                if (sPath === cfg.duration && oControl.isA("sap.m.Input")) {
                    oControl.__componentAttached = true;
                    oControl.attachChange(function () { commitDuration(oView, cfg, oControl); });
                } else if (aFields.indexOf(sPath) >= 0) {
                    oControl.__componentAttached = true;
                    oControl.attachChange(function () {
                        setTimeout(function () { refresh(oView, cfg); }, 0);
                    });
                }
                return false;
            });
        });
    }

    return {
        onInit: function () {
            var oView = this.getView();
            oView.attachModelContextChange(function () { attach(oView); });
        },
        onAfterRendering: function () {
            attach(this.getView());
        }
    };
});
