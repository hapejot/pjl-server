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

    // ── CDM bootstrap ───────────────────────────────────────────────

    /**
     * Load the current user from the server (/flp/me, resolved from the
     * auth header) and write it into the Container adapter config BEFORE
     * Container.init. The CDM ContainerAdapter builds the shell user from
     * services.Container.adapter.config.userProfile.defaults
     * (sapui5 1.153, adapters/cdm/ContainerAdapter). Failures are logged;
     * the boot continues without server user data.
     */
    function loadServerUser(oUshellConfig) {
        return fetch("/flp/me", { headers: { "Accept": "application/json" } })
            .then(function (res) {
                if (!res.ok) { throw new Error("HTTP " + res.status); }
                return res.json();
            })
            .then(function (oMe) {
                var oServices = oUshellConfig.services = oUshellConfig.services || {};
                var oContainer = oServices.Container = oServices.Container || {};
                var oAdapter = oContainer.adapter = oContainer.adapter || {};
                var oConfig = oAdapter.config = oAdapter.config || {};
                var oProfile = oConfig.userProfile = oConfig.userProfile || {};
                var oDefaults = Object.assign({}, oProfile.defaults, {
                    id: oMe.name,
                    fullName: oMe.fullname || oMe.name
                });
                // Settings from SystemUser. Only keys supported by sap.ushell.User
                // are kept (ContainerAdapter removes the rest). Empty values are skipped.
                var aSettingKeys = ["email", "language", "timeZone", "calendarWeekNumbering",
                    "contentDensity", "accessibility", "detectDarkMode", "showMyHome",
                    "importBookmarks", "imageConsent", "trackUsageAnalytics"];
                aSettingKeys.forEach(function (sKey) {
                    if (oMe[sKey] !== null && oMe[sKey] !== undefined && oMe[sKey] !== "") {
                        oDefaults[sKey] = oMe[sKey];
                    }
                });
                if (oMe.theme) {
                    oDefaults.bootTheme = { theme: oMe.theme, root: "" };
                }
                oProfile.defaults = oDefaults;
                console.log("[flp-init] Server user loaded:", oMe.name);
            })
            .catch(function (e) {
                console.warn("[flp-init] Could not load /flp/me:", e);
            });
    }

    // ── CDM bootstrap ───────────────────────────────────────────────

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

                        loadServerUser(oUshellConfig)
                            .then(function () {
                                return ushellUtils.requireAsync(["sap/ushell/bootstrap/Config"]);
                            })
                            .then(function (aConfig) {
                                // Must run before sap/ushell/Container is loaded,
                                // otherwise ShellModel reads the config too early.
                                aConfig[0].initialize(oUshellConfig);

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
                                return ushellUtils.requireAsync(["sap/ushell/Container"]);
                            })
                            .then(function (aModules) {
                                aModules[0].init("cdm").then(function () {
                                    applyUshellProperties(savedUshellProps);
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
