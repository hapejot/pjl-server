sap.ui.define(["sap/base/util/ObjectPath", "sap/ui/core/format/DateFormat"], function (ObjectPath, DateFormat) {
    "use strict";
    // Eingebaute Anzeigeformate (`ui.format` in ddic.json). Datum und Uhrzeit folgen
    // dem Locale des Benutzers ueber DateFormat; Zahlen bleiben bei FE-Standard.
    var UNITS = ["B", "KB", "MB", "GB", "TB"];

    function num(v) {
        return (v === null || v === undefined || v === "") ? null : Number(v);
    }
    function one(n) {
        return n < 10 ? n.toFixed(1) : Math.round(n).toString();
    }
    function pad(x) {
        return (x < 10 ? "0" : "") + x;
    }

    var FORMATTER = {
        bytes: function (v) {
            var n = num(v);
            if (n === null || isNaN(n)) { return ""; }
            var i = 0;
            while (n >= 1024 && i < UNITS.length - 1) { n /= 1024; i++; }
            return (i === 0 ? String(Math.round(n)) : one(n)) + " " + UNITS[i];
        },
        bitrate: function (v) {
            var n = num(v);
            if (n === null || isNaN(n)) { return ""; }
            return n >= 1000000 ? one(n / 1000000) + " Mbit/s" : Math.round(n / 1000) + " kbit/s";
        },
        duration: function (v) {
            var n = num(v);
            if (n === null || isNaN(n)) { return ""; }
            var s = Math.round(n);
            var h = Math.floor(s / 3600);
            var m = Math.floor((s % 3600) / 60);
            var sec = s % 60;
            return h > 0 ? h + ":" + pad(m) + ":" + pad(sec) : m + ":" + pad(sec);
        },
        date: function (v) {
            if (!v) { return ""; }
            var d = new Date(v);
            if (isNaN(d.getTime())) { return ""; }
            return DateFormat.getDateInstance().format(d);
        },
        datetime: function (v) {
            if (!v) { return ""; }
            var d = new Date(v);
            if (isNaN(d.getTime())) { return ""; }
            return DateFormat.getDateTimeInstance().format(d);
        }
    };
    // Globaler Name fuer die Formatter-Referenz in den Fragmenten (XML-Bindung).
    ObjectPath.set("fiori.ext.Format", FORMATTER);
    return FORMATTER;
});
