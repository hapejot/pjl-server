/*
 * Controller-Erweiterung der Object Page (Manifest: controller_extensions).
 *
 * Abschnitt "Zeitraum" (fragment/TimeSection.fragment.xml):
 * - Beginn/Ende: change-Event -> Nachladen von Beginn, Ende und Dauer.
 * - Dauer: Eingabefeld, direkt an duration_minutes gebunden (Anzeige als h:mm, Formatter
 *   fiori.ext.Format.hm). Beim change wird der Text geparst ("2:30", "2h", "2h 30",
 *   "30 min", "30") und in Minuten als Property-Bindung geschrieben (PATCH).
 */
sap.ui.define([], function () {
    "use strict";

    var TIME_PROPERTIES = ["starts_at", "ends_at", "duration_minutes"];

    // Liefert Minuten oder null, wenn die Eingabe nicht verstanden wird.
    function parseDuration(sText) {
        var s = String(sText || "").trim().toLowerCase();
        if (s === "") {
            return null;
        }
        var m;
        // 2:30 / 0:45
        m = /^(\d+):(\d{1,2})$/.exec(s);
        if (m) {
            return Number(m[2]) > 59 ? null : Number(m[1]) * 60 + Number(m[2]);
        }
        // 2h / 2h 30 / 2h30 / 2 h 30 min
        m = /^(\d+)\s*h(?:\s*(\d+)\s*(?:min|m)?)?$/.exec(s);
        if (m) {
            var mm = m[2] ? Number(m[2]) : 0;
            return mm > 59 ? null : Number(m[1]) * 60 + mm;
        }
        // 30 min / 30m
        m = /^(\d+)\s*(?:min|m)$/.exec(s);
        if (m) {
            return Number(m[1]);
        }
        // 30 (Minuten)
        m = /^(\d+)$/.exec(s);
        if (m) {
            return Number(m[1]);
        }
        return null;
    }

    function bindingPath(oControl) {
        var oBinding = oControl.getBinding("value");
        return oBinding ? oBinding.getPath() : null;
    }

    function refresh(oView) {
        var oCtx = oView.getBindingContext();
        if (!oCtx) {
            return;
        }
        // Die gebundenen Felder aktualisieren sich danach selbst.
        oCtx.requestSideEffects(TIME_PROPERTIES).catch(function (oErr) {
            console.error("TimeSectionController: Nachladen fehlgeschlagen", oErr);
        });
    }

    function commitDuration(oView, oInput) {
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
        var oBinding = oCtx.getModel().bindProperty("duration_minutes", oCtx);
        oBinding.requestValue().then(function () {
            oBinding.setValue(nMinutes);
            setTimeout(function () { refresh(oView); }, 0);
        }, function (oErr) {
            console.error("TimeSectionController: Dauer lesen fehlgeschlagen", oErr);
        });
    }

    function attachControls(oView) {
        var aControls = oView.findAggregatedObjects(true, function (oControl) {
            return typeof oControl.getBinding === "function" && !!bindingPath(oControl);
        });
        aControls.forEach(function (oControl) {
            if (oControl.__timeSectionAttached) {
                return;
            }
            var sPath = bindingPath(oControl);
            if (sPath === "duration_minutes" && oControl.isA("sap.m.Input")) {
                oControl.__timeSectionAttached = true;
                oControl.attachChange(function () { commitDuration(oView, oControl); });
            } else if (TIME_PROPERTIES.indexOf(sPath) >= 0) {
                oControl.__timeSectionAttached = true;
                oControl.attachChange(function () {
                    setTimeout(function () { refresh(oView); }, 0);
                });
            }
        });
    }

    return {
        onInit: function () {
            var oView = this.getView();
            oView.attachModelContextChange(function () { attachControls(oView); });
        },
        onAfterRendering: function () {
            attachControls(this.getView());
        }
    };
});
