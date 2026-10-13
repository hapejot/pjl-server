use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::header,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use spec::app::App;
use spec::EntitySpec;
use tracing::{debug, info};

use crate::AppState;
use axum::{http::StatusCode, response::Response};

pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("Interner Fehler: {:?}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Etwas ist schiefgelaufen: {}", self.0),
        )
            .into_response()
    }
}

// Ermöglicht ?-Operator für JEDEN Fehlertyp, der Into<anyhow::Error> ist
// (also praktisch jeden std::error::Error)
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        AppError(err.into())
    }
}
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/init.js", get(init))
        .route("/site.json", get(site))
}

const DEFAULT_APP_ICON: &str = "sap-icon://settings";

fn subtitle_of(app: &App) -> &str {
    app.subtitle().map(String::as_str).unwrap_or_default()
}

fn icon_of(app: &App) -> &str {
    app.icon().map(String::as_str).unwrap_or(DEFAULT_APP_ICON)
}

async fn site(State(_state): State<AppState>) -> Json<Value> {
    info!("site");
    let ddic = _state.ddic();
    let mut apps: HashMap<String, Value> = HashMap::new();
    for app in ddic.apps() {
        let app_json = json!({
            "sap.app":{
                "crossNavigation":{
                    "inbounds":{
                        format!("{}-{}", app.main_entity(&ddic).name(), app.action()):{
                            "action":app.action(),
                            "semanticObject":app.main_entity(&ddic).name(),
                            "signature":{
                                "additionalParameters":"allowed",
                                "parameters":{}}}}},
                "id":app.name(),
                "subTitle":subtitle_of(app),
                "title":app.title()},
            "sap.flp":{ "type":"application"},
            "sap.platform.runtime":{ "componentProperties": {"url": format!("./apps/{}/", app.main_entity(&ddic).name())}},                                            
            "sap.ui":{"deviceTypes":{"desktop":true,"phone":true,"tablet":true}, "technology":"UI5"},
            "sap.ui5":{"componentName":app.name()}});
        apps.insert(app.name().to_string(), app_json);
    }

    let mut viz: HashMap<String, Value> = HashMap::new();
    let mut viz_refs: HashMap<String, Value> = HashMap::new();
    let mut viz_order: Vec<String> = Vec::new();
    for app in ddic.apps() {
        let viz_json = json!({
            "businessApp": app.name(),
            "target": {
                "action": app.action(),
                "semanticObject": app.main_entity(&ddic).name()
            },
            "vizConfig": {
                "sap.app": {
                    "icon": icon_of(app),
                    "info": "",
                    "subTitle": subtitle_of(app),
                    "title": app.title()
                },
                "sap.flp": {
                    "target": {
                        "appId": app.name(),
                        "inboundId": format!("{}-{}", app.main_entity(&ddic).name(), app.action())
                    }
                }
            },
            "vizType": "sap.ushell.StaticAppLauncher"
        });
        let viz_name = format!("{}-{}-viz", app.main_entity(&ddic).name(), app.action());
        viz.insert(viz_name.clone(), viz_json);
        viz_refs.insert(
            viz_name.clone(),
            json!({
                "id": viz_name,
                "vizId": viz_name
            }),
        );
        viz_order.push(viz_name.clone());
    }

    Json(json!({
      "_version": "3.1.0",
      "applications": apps,
      "menus": {
        "main": {
          "payload": {
            "menuEntries": [
              {
                "id": "home-space-entry",
                "target": {
                  "action": "openFLPPage",
                  "parameters": [
                    {
                      "name": "spaceId",
                      "value": "home-space"
                    },
                    {
                      "name": "pageId",
                      "value": "home-page"
                    }
                  ],
                  "semanticObject": "Launchpad"
                },
                "title": "Home",
                "type": "IBN"
              }
            ]
          }
        }
      },
      "pages": {
        "home-page": {
          "identification": {
            "id": "home-page",
            "title": "Home"
          },
          "payload": {
            "layout": {
              "sectionOrder": [
                "apps-section"
              ]
            },
            "sections": {
              "apps-section": {
                "default": true,
                "id": "apps-section",
                "layout": {
                  "vizOrder": viz_order
                },
                "locked": false,
                "preset": true,
                "title": "Applications",
                "visible": true,
                "viz": viz_refs
              }
            }
          }
        }
      },
      "site": {
        "identification": {
          "id": "local-flp-site",
          "title": "Local Fiori Launchpad"
        },
        "payload": {}
      },
      "systemAliases": {},
      "visualizations": viz,
      "vizTypes": {}
    }))
}
async fn init() -> impl IntoResponse {
    info!("static init.js");
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../static/flp/init.js"),
    )
}

/// `sap.ui.loader.config`-Aufrufe fuer die UI5-Pfade der Erweiterungen.
/// Zusaetzliche Abhaengigkeiten fuer `sap.ui.define`, z. B. `, "video/ext/Format"`.
fn ui5_module_list(state: &AppState) -> String {
    crate::extension::ui5_modules(&state.extensions)
        .iter()
        .map(|m| format!(", {}", serde_json::to_string(m).expect("Modulname ist ein String")))
        .collect()
}

fn ui5_path_config(state: &AppState) -> String {
    state
        .extensions
        .iter()
        .flat_map(|ext| ext.ui5_paths())
        .map(|(ns, url)| {
            format!(
                "sap.ui.loader.config({{ paths: {{ \"{}\": \"{}\" }} }});\n",
                ns.replace('.', "/"),
                url
            )
        })
        .collect()
}

pub async fn app_component_handler(
    Path(app): Path<String>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    debug!("Component.js for {}", app);

    let ddic = _state.ddic();
    let a = ddic
        .apps()
        .find(|x| x.main_entity(&ddic).name() == app)
        .unwrap();

    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        format!(
            r##"
// Namespace der App auf ihr Verzeichnis abbilden, damit UI5 Modul- und Preload-Pfade findet.
sap.ui.loader.config({{ paths: {{ "{cmp}": "/apps/{main}" }} }});
{ui5_paths}
sap.ui.define(["sap/fe/core/AppComponent"{modules}], function (AppComponent) {{
    "use strict";
	return AppComponent.extend("{cmp}.Component", {{ metadata: {{ manifest: "json" }} }});
}}); "##,
            cmp = a.name(),
            main = a.main_entity(&ddic).name(),
            ui5_paths = ui5_path_config(&_state),
            modules = ui5_module_list(&_state)
        ),
    )
}

/// LRep-Stub: UI5 fragt beim Start Aenderungs-Daten und Feature-Settings ab.
/// Ohne Antwort (404) oder mit falschem Content-Type meldet UI5 Fehler.
pub async fn lrep_settings() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        r#"{"isKeyUser":false,"isVariantSharingEnabled":false,"isAtoAvailable":false,"isAtoEnabled":false,"isProductiveSystem":true}"#,
    )
}

/// LRep-Stub fuer Flex-Aenderungen: keine Aenderungen vorhanden.
pub async fn lrep_flex_data() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        r#"{"changes":[],"contents":[]}"#,
    )
}

/// Kein Preload-Bundle vorhanden: leere Antwort statt 404, damit UI5 nicht
/// fehlschlaegt. Die Komponente laedt ihre Module einzeln.
pub async fn component_preload_handler() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        "// no preload bundle\n",
    )
}

pub async fn app_manifest_handler(
    Path(app): Path<String>,
    State(_state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    debug!("manifest.json for {}", app);

    let ddic = _state.ddic();
    let a = ddic
        .apps()
        .find(|x| x.main_entity(&ddic).name() == app)
        .unwrap();
    let main_entity = a.main_entity(&ddic);
    let main_name = main_entity.name().to_string();
    let main_set = main_entity.setname();
    let list_target = format!("{}List", main_name);
    let object_target = format!("{}ObjectPage", main_name);

    let mut routes: Vec<Value> = Vec::new();
    routes.push(json!(
    {
      "name": list_target.clone(),
      "pattern": ":?query:",
      "target": list_target.clone()
    }));
    routes.push(json!(
      {
        "name": object_target.clone(),
        "pattern": format!("{}({{key}}):?query:", main_set.clone()),
        "target": [
          list_target.clone(),
          object_target.clone()
        ]
      }
    ));

    let mut targets: serde_json::Map<String, Value> = serde_json::Map::new();
    targets.insert(
        list_target.clone(),
        json!({
          "type": "Component",
          "id": list_target.clone(),
          // Without an explicit contextPattern the target inherits the deepest
          // context of the route (i.e. the sub item), see ObjectPage below.
          "contextPattern": "",
          "controlAggregation": "beginColumnPages",
          "name": "sap.fe.templates.ListReport",
          "options": {
            "settings": {
              "entitySet": main_set.clone(),
              "contextPath": format!("/{}", &main_set),
              "initialLoad": "Enabled",
              "controlConfiguration": list_control_configuration_with_extensions(&_state, a.name(), &main_name),
              "navigation": {
                main_set.clone(): {
                  "detail": {
                    "route": object_target.clone()
                  }
                }
              },
              "variantManagement": "Page"
            }
          },
        }),
    );
    // Listenansichten (View-Leiste) in die ListReport-Settings einhaengen.
    if let Some(settings) = targets
        .get_mut(&list_target)
        .and_then(|t| t.get_mut("options"))
        .and_then(|o| o.get_mut("settings"))
        .and_then(|s| s.as_object_mut())
    {
        settings.extend(list_views_settings(
            &main_entity,
            &crate::extension::list_views(&_state.extensions, &main_name),
        ));
    }

    let mut object_navigation: serde_json::Map<String, Value> = serde_json::Map::new();

    // Every to-many navigation of the main entity becomes a section (table) on the
    // mid column ObjectPage. Clicking a row must open the sub ObjectPage in the
    // third (end) column of the FlexibleColumnLayout.
    for rel in ddic
        .relationships()
        .filter(|r| r.one.is_some() && r.many.is_some() && r.one() == main_name)
    {
        // Navigation property as it exists in the EDMX (e.g. `_Fields`), NOT the
        // technical relationship name.
        let nav = rel.one_property().to_string();
        let child_entity = rel.many().to_string();
        // Unique per navigation: two relationships may point to the same child
        // entity (e.g. Entity/_One and Entity/_Many -> Relation).
        let child_object_target = format!("{}{}ObjectPage", main_name, nav.trim_start_matches('_'));

        routes.push(json!({
          "name": child_object_target,
          "pattern": format!("{}({{key}})/{}({{key2}}):?query:", main_set, nav),
          "target": [
            list_target,
            object_target,
            child_object_target
          ]
        }));

        targets.insert(
            child_object_target.clone(),
            json!({
              "contextPattern": format!("/{}({{key}})/{}({{key2}})", main_set, nav),
              "controlAggregation": "endColumnPages",
              "id": child_object_target,
              "name": "sap.fe.templates.ObjectPage",
              "options": {
                "settings": {
                  "contextPath": format!("/{}/{}", main_set, nav)
                }
              },
              "type": "Component"
            }),
        );
        debug!(
            "{}: sub object page {} -> {} ({})",
            main_name, nav, child_object_target, child_entity
        );

        object_navigation.insert(
            nav,
            json!({
              "detail": {
                "route": child_object_target
              }
            }),
        );
    }

    targets.insert(
        object_target.clone(),
        json!({
            "type": "Component",
            "controlAggregation": "midColumnPages",
            // Pin this page to the main entity context. Otherwise the three
            // target route (list, object, sub object) binds the mid column to
            // the sub item context as well and the page requests main entity
            // properties (e.g. `plural`) on the child entity.
            "contextPattern": format!("/{}({{key}})", main_set),
            "id": object_target.clone(),
            "name": "sap.fe.templates.ObjectPage",
            "options": {
              "settings": {
                "entitySet": main_set.clone(),
                "contextPath": format!("/{}", &main_set),
                "navigation": Value::Object(object_navigation),
              }
            },
        }),
    );

    // Erweiterungen (Custom Sections, ausgeblendete Facets) in die Object Page.
    if let Some(content) = crate::extension::object_page_content(&_state.extensions, &main_name) {
        if let Some(settings) = targets
            .get_mut(&object_target)
            .and_then(|t| t.get_mut("options"))
            .and_then(|o| o.get_mut("settings"))
            .and_then(|s| s.as_object_mut())
        {
            settings.insert("content".into(), content);
        }
    }


    let sem = format!("{}-{}", a.main_entity(&ddic).name(), a.action());
    let sap_app = json!({
      "applicationVersion": {
        "version": "1.0.0"
      },
      "crossNavigation": {
        "inbounds": {
          sem: {
            "action": a.action(),
            "semanticObject": a.main_entity(&ddic).name(),
            "signature": {
              "additionalParameters": "allowed",
              "parameters": {}
            }
          }
        }
      },
      "dataSources": {
        "mainService": {
          "settings": {
            "odataVersion": "4.0"
          },
          "type": "OData",
          "uri": "/Service/"
        }
      },
      "description": "Fiori Elements List Report + Object Page",
      "i18n": "i18n/i18n.properties",
      "id": a.name(),
      "subTitle": subtitle_of(a),
      "title": a.title(),
      "type": "application"
    });
    let r = json!(
    {
      "_version": "1.65.0",
      "sap.app": sap_app,
      "sap.ui5": {
        "dependencies": {
          "libs": {
            "sap.f": {},
            "sap.fe.templates": {},
            "sap.m": {},
            "sap.ui.core": {}
          },
          "minUI5Version": "1.139.0",
          "modules": crate::extension::ui5_modules(&_state.extensions)
        },
        "extends": crate::extension::controller_extensions_manifest(&_state.extensions),
        "flexEnabled": false,
        "commands": {
          "DeleteEntry::ContextMenu": {}
        },
        "models": {
          "": {
            "dataSource": "mainService",
            "settings": {
              "autoExpandSelect": true,
              "earlyRequests": true,
              "operationMode": "Server"
            }
          },
          "@i18n": {
            "type": "sap.ui.model.resource.ResourceModel",
            "uri": "i18n/i18n.properties"
          }
        },
        "rootView": {
          "id": "appRootView",
          "type": "XML",
          "viewName": "sap.fe.core.rootView.Fcl"
        },
        "routing": {
          "config": {
            "controlAggregation": "beginColumnPages",
            "controlId": "appContent",
            "flexibleColumnLayout": {
              "defaultThreeColumnLayoutType": "ThreeColumnsMidExpanded",
              "defaultTwoColumnLayoutType": "TwoColumnsMidExpanded"
            },
            "routerClass": "sap.f.routing.Router"
          },
          "routes": routes,
          "targets": targets
        }
      }
    });

    Ok(Json(r))
}

/// Ansichten der ListReport-Seite aus den Listenansichten der Hauptentitaet.
///
/// Nur bei mehr als einer Ansicht: dann erscheint die Auswahl (View-Leiste) und
/// jede Ansicht zeigt ihr `UI.PresentationVariant#<name>` (bzw. unqualifiziert
/// fuer `default`). Mit einer oder keiner Ansicht bleibt die Seite unveraendert.
pub(crate) fn list_views_settings(
    entity: &EntitySpec,
    extra: &[Value],
) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let views = entity.list_views();
    let mut ordered: Vec<&spec::ListView> = views.iter().filter(|v| v.is_default()).collect();
    ordered.extend(views.iter().filter(|v| !v.is_default()));
    let mut paths: Vec<Value> = ordered
        .iter()
        .map(|v| {
            let annotation_path = if v.is_default() {
                "com.sap.vocabularies.UI.v1.PresentationVariant".to_string()
            } else {
                format!("com.sap.vocabularies.UI.v1.PresentationVariant#{}", v.name)
            };
            json!({ "key": v.name, "annotationPath": annotation_path })
        })
        .collect();
    paths.extend(extra.iter().cloned());
    if paths.len() <= 1 {
        return out;
    }
    out.insert("views".into(), json!({ "paths": paths }));
    out
}

/// Designer-Konfiguration plus Spalten aus den Erweiterungen (`Extension::list_columns`).
fn list_control_configuration_with_extensions(
    state: &AppState,
    app_id: &str,
    entity: &str,
) -> Value {
    let mut config = list_control_configuration(state, app_id);
    if let Some(columns) = crate::extension::list_columns(&state.extensions, entity) {
        let line_item = config
            .as_object_mut()
            .expect("controlConfiguration ist ein Objekt")
            .entry("@com.sap.vocabularies.UI.v1.LineItem")
            .or_insert_with(|| json!({}));
        if let Some(obj) = line_item.as_object_mut() {
            // Zusammenfuehren, falls Format-Overrides schon gesetzt sind.
            match (obj.get_mut("columns"), columns) {
                (Some(Value::Object(existing)), Value::Object(extra)) => existing.extend(extra),
                (_, columns) => {
                    obj.insert("columns".into(), columns);
                }
            }
        }
    }
    config
}

/// Zusatzkonfiguration der ListReport-Tabelle.
///
/// Im Designer bekommt jede Liste einen Toolbar-Button "Publish Dictionary".
/// Der Button ist eine Fiori-Elements-*custom action* - das betrifft aber nur
/// die Darstellung. Ausgeloest wird die ungebundene OData-Aktion `export`
/// (siehe `srv::export_action`), die regulaer im `$metadata` deklariert ist.
///
/// Fiori Elements kennt fuer Toolbar-Buttons nur `DataFieldForAction` auf
/// *gebundene* Aktionen; ein ungebundener Action Import laesst sich nicht
/// annotieren. Deshalb der Umweg ueber einen eigenen Handler, der die Aktion
/// selbst am Modell aufruft.
///
/// Der Laufzeitserver bekommt den Button nicht - dort ist die Aktion gar nicht
/// erst deklariert.
fn list_control_configuration(state: &AppState, app_id: &str) -> Value {
    if !state.mode().is_designer() {
        return json!({});
    }
    json!({
      "@com.sap.vocabularies.UI.v1.LineItem": {
        "actions": {
          "publishDictionary": {
            "enabled": true,
            // Modulpfad relativ zum Komponenten-Namensraum: UI5 laedt
            // `./apps/<Entity>/ext/Publish.js`, siehe `publish_ext_handler`.
            "press": format!("{app_id}.ext.Publish.publish"),
            "text": "Publish Dictionary",
            "visible": true
          }
        }
      }
    })
}

/// Handler-Modul hinter dem "Publish Dictionary"-Button.
///
/// Ruft die ungebundene Aktion ueber das OData-V4-Modell auf. Ein
/// Operation-Binding auf den Action Import (`/export(...)`) statt eines
/// direkten `fetch`: so laeuft der Aufruf durch dieselbe Modellschicht wie
/// jede andere Aktion des Service - inklusive der Fehlerbehandlung des
/// Modells - und der Pfad stammt aus dem `$metadata` statt aus einer hier
/// fest verdrahteten URL.
pub async fn publish_ext_handler(
    Path(app): Path<String>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    debug!("ext/Publish.js for {}", app);
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        format!(
            r##"sap.ui.define([
    "sap/m/MessageBox",
    "sap/m/MessageToast"
], function (MessageBox, MessageToast) {{
    "use strict";
    return {{
        publish: function () {{
            // `this` ist die Fiori-Elements ExtensionAPI der ListReport-Seite.
            var oModel = this.getModel();
            // Ungebundene Aktion: das Binding haengt an der Wurzel des
            // Service, nicht an einem Kontext. Die drei Punkte sind die
            // V4-Schreibweise fuer "noch nicht ausgefuehrt".
            var oOperation = oModel.bindContext("/{action}(...)");
            MessageToast.show("Publishing data dictionary...");
            oOperation.execute().then(function () {{
                var r = oOperation.getBoundContext().getObject();
                MessageBox.success(
                    "Data dictionary published.\n\n" +
                    r.entities + " entities, " + r.relationships + " relationships, " +
                    r.apps + " apps, " + r.entity_sets + " entity sets.\n\n" +
                    r.ddic_path + "\n" + r.schema_path + "\n\n" +
                    "Restart the runtime server to pick up the changes."
                );
            }}).catch(function (oError) {{
                MessageBox.error("Export failed: " + (oError.message || oError));
            }});
        }}
    }};
}});
"##,
            action = crate::srv::EXPORT_ACTION
        ),
    )
}


pub async fn app_handler(
    Path((app, path)): Path<(String, String)>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    debug!("{} {}", app, path);

    let ddic = _state.ddic();
    let a = ddic.apps().find(|x| x.main_entity(&ddic).name() == app);
    debug!("{:?}", a);

    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        "",
    )
}

pub async fn index() -> impl IntoResponse {
    info!("fetch static index");
    let out_format = r##"
        <!doctype html>
<html>
<head>
    <meta http-equiv="X-UA-Compatible" content="IE=edge" />
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Local Fiori Launchpad</title>
    <link rel="icon" type="image/svg+xml" href="/favicon.svg"/>

    <script type="text/javascript">
        window["sap-ushell-config"] = {
            defaultRenderer: "fiori2",
            ushell: {
                spaces: {
                    enabled: true,
                    myHome: {
                        enabled: false
                    }
                },
                shell: {
                    enablePersonalization: false
                }
            },
            renderers: {
                fiori2: {
                    componentData: {
                        config: {
                            enableSearch: false,
                            rootIntent: "Shell-home"
                        }
                    }
                }
            },
            services: {
                CommonDataModel: {
                    adapter: {
                        config: {
                            siteDataUrl: "/flp/site.json"
                        }
                    }
                },
                // CDM platform has no UserInfoAdapter — use the local one
                UserInfo: {
                    adapter: {
                        module: "sap.ushell.adapters.local.UserInfoAdapter",
                        config: {
                            id: "DEMO_USER",
                            firstName: "John",
                            lastName: "Doe",
                            fullName: "John Doe",
                            email: "john.doe@example.com"
                        }
                    }
                },
                Container: {
                    adapter: {
                        config: {
                            id: "DEMO_USER",
                            firstName: "John",
                            lastName: "Doe",
                            fullName: "John Doe",
                            email: "john.doe@example.com",
                            storageResourceRoot: "/"
                        }
                    }
                },
                // Use local Personalization adapter (localStorage-based)
                Personalization: {
                    adapter: {
                        module: "sap.ushell.adapters.local.PersonalizationAdapter",
                        config: {
                            storageResourceRoot: "/"
                        }
                    }
                },
                PersonalizationV2: {
                    adapter: {
                        module: "sap.ushell.adapters.local.PersonalizationAdapter",
                        config: {
                            storageResourceRoot: "/"
                        }
                    }
                }
            },
            ushellProperties: {
                "/core/companyLogo/url": "/logo.svg"
            }
        };
    </script>

    <script src="flp/init.js"></script>

    <script
        id="sap-ui-bootstrap"
        src="https://ui5.sap.com/1.139.0/resources/sap-ui-core.js"
        data-sap-ui-libs="sap.m, sap.ushell, sap.fe.templates, sap.f"
        data-sap-ui-async="true"
        data-sap-ui-preload="async"
        data-sap-ui-theme="sap_horizon"
        data-sap-ui-compatVersion="edge"
        data-sap-ui-language="de"
        data-sap-ui-bindingSyntax="complex"
        data-sap-ui-resourceroots='{
            "products.demo": "./"
        }'
        data-sap-ui-frameOptions="allow"
    ></script>

    <script>
        sap.ui.getCore().attachInit(function () {
            sap.ushell.Container.createRenderer("fiori2", true).then(function (oRenderer) {
                oRenderer.placeAt("content");
            });
        });
    </script>
</head>
<body class="sapUiBody" id="content"></body>
</html>
    "##;
    (
        // UI5 (ResizeHandler) registriert einen unload-Listener; ohne diese
        // Freigabe meldet der Browser eine Permissions-Policy-Verletzung.
        [(
            header::HeaderName::from_static("permissions-policy"),
            "unload=(self)",
        )],
        Html(out_format.to_string()),
    )
}
