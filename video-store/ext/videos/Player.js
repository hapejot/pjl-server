sap.ui.define(["sap/base/util/ObjectPath"], function (ObjectPath) {
    "use strict";

    var PLAYER = {
        // Klick auf ein Thumbnail: springt zur Zeit des Thumbnails und startet die Wiedergabe.
        seek: function (oEvent) {
            var iSeconds = Number(oEvent.getSource().data("seconds"));
            var oVideo = document.getElementById("videoPlayer");
            if (!oVideo || isNaN(iSeconds)) { return; }
            oVideo.currentTime = iSeconds;
            oVideo.play();
        }
    };
    // Globaler Name fuer den String-Handler im Thumbnail-Fragment (videos.ext.Player.seek).
    ObjectPath.set("videos.ext.Player", PLAYER);
    return PLAYER;
});
