/**
 * @fileOverview FLP bootstrap for CDM 3.1 platform mode.
 *
 * The server provides a CDM site document at /cdm/site.json. The UShell
 * CommonDataModelAdapter fetches it automatically via the siteDataUrl
 * configured in window["sap-ushell-config"]. This script only needs to
 * boot the Container in CDM mode and apply post-init customizations
 * (company logo, user profile).
 *
 * Uses the official xx-bootTask entry point so that UI5 core settings
 * (theme, language, etc.) injected by the server into the HTML are respected.
 */
(function () {
    "use strict";

    // ── helpers ──────────────────────────────────────────────────────

    function getNestedValue(obj, path) {
        return path.split(".").reduce(function (cur, key) {
            return cur && cur[key];
        }, obj);
    }

    // ── apply ushellProperties after Container.init ───────────────

    /**
     * Apply the company logo on the ShellHeader control once the renderer
     * has loaded.  CDM mode may not pick up ushellProperties automatically.
     */
    function applyUshellProperties(oProps) {
        if (!oProps) { return; }
        var sLogo = oProps["/core/companyLogo/url"];
        if (!sLogo) { return; }

        function setLogo() {
            var oHeader = sap.ui.getCore().byId("shell-header");
            if (oHeader && typeof oHeader.setLogo === "function") {
                oHeader.setLogo(sLogo);
                console.log("[flp-init] Company logo set:", sLogo);
            }
        }

        var oBus = sap.ui.getCore().getEventBus();
        oBus.subscribeOnce("sap.ushell", "rendererLoaded", setLogo);
    }

    /**
     * Apply user profile data to the UShell Container.getUser() object.
     * In CDM mode some setter methods may not exist — guard each call.
     */
    function applyUserProfile(oUshellConfig) {
        var oCfg = getNestedValue(oUshellConfig, "services.Container.adapter.config");
        if (!oCfg) { return; }

        try {
            var oUser = sap.ushell.Container.getUser();
            if (!oUser) { return; }

            if (oCfg.id        && typeof oUser.setId === "function")        { oUser.setId(oCfg.id); }
            if (oCfg.firstName && typeof oUser.setFirstName === "function") { oUser.setFirstName(oCfg.firstName); }
            if (oCfg.lastName  && typeof oUser.setLastName === "function")  { oUser.setLastName(oCfg.lastName); }
            if (oCfg.fullName  && typeof oUser.setFullName === "function")  { oUser.setFullName(oCfg.fullName); }
            if (oCfg.email     && typeof oUser.setEmail === "function")     { oUser.setEmail(oCfg.email); }

            console.log("[flp-init] User profile applied:", oCfg.fullName || oCfg.id);
        } catch (e) {
            console.warn("[flp-init] Could not set user profile:", e);
        }
    }

    // ── CDM bootstrap ───────────────────────────────────────────────

    function init() {
        return new Promise(function (resolve) {
            sap.ui.require(
                ["sap/ushell/utils"],
                function (ushellUtils) {
                    function bootstrap(fnCallback) {
                        var oUshellConfig = window["sap-ushell-config"] || {};
                        window["sap-ushell-config"] = oUshellConfig;

                        // Clear stale UShell personalization from localStorage.
                        // Switching from local to CDM platform mode leaves
                        // incompatible personalization data that causes
                        // "Cannot mixin the personalization" errors.
                        try {
                            for (var i = localStorage.length - 1; i >= 0; i--) {
                                var key = localStorage.key(i);
                                if (key && (key.indexOf("sap.ushell") !== -1 ||
                                            key.indexOf("sap-ushell") !== -1)) {
                                    localStorage.removeItem(key);
                                }
                            }
                        } catch (e) { /* localStorage may be unavailable */ }

                        // Save and remove ushellProperties before Container.init —
                        // CDM mode may not process them from config.
                        var savedUshellProps = oUshellConfig.ushellProperties
                            ? JSON.parse(JSON.stringify(oUshellConfig.ushellProperties))
                            : null;
                        delete oUshellConfig.ushellProperties;

                        // Boot UShell in CDM platform mode.
                        // The CommonDataModelAdapter will fetch /cdm/site.json
                        // from the siteDataUrl configured in sap-ushell-config.

                        // Pre-register missing CDM adapters.
                        // NavTargetResolutionInternalAdapter and AppStateAdapter
                        // do not exist in the CDM platform but are required at
                        // runtime.  These shims delegate to the real services.
                        sap.ui.define("sap/ushell/adapters/cdm/NavTargetResolutionInternalAdapter", [
                                      "sap/ui/thirdparty/jquery",
                                      "sap/ushell/Container"
                        ], function (jQ, Ctnr) {
                            function A() {
                                function c() { return Ctnr.getServiceAsync("ClientSideTargetResolution"); }
                                this.resolveHashFragment = function (h) {
                                    var d = new jQ.Deferred();
                                    c().then(function (o) { o.resolveHashFragment(h).then(d.resolve, d.reject); })["catch"](d.reject);
                                    return d.promise();
                                };
                                this.isIntentSupported = function (a) {
                                    var d = new jQ.Deferred();
                                    c().then(function (o) { o.isIntentSupported(a).then(d.resolve, d.reject); })["catch"](d.reject);
                                    return d.promise();
                                };
                                this.getSemanticObjectLinks = function (s, m) {
                                    var d = new jQ.Deferred();
                                    c().then(function (o) { o.getLinks({semanticObject: s, params: m}).then(d.resolve, d.reject); })["catch"](d.reject);
                                    return d.promise();
                                };
                                this.getDistinctSemanticObjects = function () {
                                    var d = new jQ.Deferred();
                                    c().then(function (o) {
                                        if (typeof o.getDistinctSemanticObjects === "function") {
                                            o.getDistinctSemanticObjects().then(d.resolve, d.reject);
                                        } else {
                                            d.resolve([]);
                                        }
                                    })["catch"](d.reject);
                                    return d.promise();
                                };
                            }
                            return A;
                        });

                        sap.ui.define("sap/ushell/adapters/cdm/AppStateAdapter", [
                                      "sap/ushell/adapters/local/AppStateAdapter"
                        ], function (LocalAdapter) {
                            return LocalAdapter;
                        });

                        ushellUtils
                            .requireAsync(["sap/ushell/Container"])
                            .then(function (aModules) {
                                aModules[0].init("cdm").then(function () {
                                    applyUshellProperties(savedUshellProps);
                                    applyUserProfile(oUshellConfig);
                                    console.log("[flp-init] CDM platform initialized.");
                                    fnCallback();
                                });
                            });
                    }

                    resolve(bootstrap);
                }
            );
        });
    }

    // ── entry point: xx-bootTask ────────────────────────────────────

    window["sap-ui-config"] = {
        "xx-bootTask": function (fnCallback) {
            init().then(function (bootstrap) {
                bootstrap(fnCallback);
            });
        }
    };
})();
