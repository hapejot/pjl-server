sap.ui.define([], function () {
    "use strict";

    // Setzt die Quelle des Players auf die Video-ID des Object-Page-Kontexts.
    function attachSource(oView) {
        var oCtx = oView.getBindingContext();
        var sId = oCtx ? oCtx.getProperty("ID") : undefined;
        var oVideo = document.getElementById("videoPlayer");
        if (!oVideo || !sId) { return; }
        var sSrc = "/ext/video/stream/" + sId;
        if (oVideo.getAttribute("src") !== sSrc) {
            oVideo.setAttribute("src", sSrc);
            oVideo.load();
        }
    }

    // Controller-Erweiterung der Object Page (Manifest: sap.ui.controllerExtensions).
    return {
        onAfterRendering: function () {
            var oView = this.getView();
            attachSource(oView);
            // Wechselt die Auswahl in der Liste, aendert sich der Kontext der View: Quelle nachziehen.
            if (!oView.__videoPlayerBound) {
                oView.__videoPlayerBound = true;
                oView.attachModelContextChange(function () { attachSource(oView); });
            }
        }
    };
});
