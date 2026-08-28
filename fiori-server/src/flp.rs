use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};
use serde_json::{Value, json};
use tracing::{debug, info};

use crate::AppState;
// use serde::{Deserialize, Serialize};
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

async fn site(State(_state): State<AppState>) -> Json<Value> {
    info!("site");
    let mut apps: HashMap<String, Value> = HashMap::new();
    for app in _state.ddic.apps() {
        let app_json = json!({
            "sap.app":{
                "crossNavigation":{
                    "inbounds":{
                        format!("{}-{}", app.main_entity(&_state.ddic).name(), app.action()):{
                            "action":app.action(),
                            "semanticObject":app.main_entity(&_state.ddic).name(),
                            "signature":{
                                "additionalParameters":"allowed",
                                "parameters":{}}}}},
                "id":app.name(),
                "subTitle":"...",
                "title":app.title()},
            "sap.flp":{ "type":"application"},
            "sap.platform.runtime":{ "componentProperties": {"url": format!("./apps/{}/", app.main_entity(&_state.ddic).name())}},                                            
            "sap.ui":{"deviceTypes":{"desktop":true,"phone":true,"tablet":true}, "technology":"UI5"},
            "sap.ui5":{"componentName":app.name()}});
        apps.insert(app.name().to_string(), app_json);
    }

    let mut viz: HashMap<String, Value> = HashMap::new();
    let mut viz_refs: HashMap<String, Value> = HashMap::new();
    let mut viz_order: Vec<String> = Vec::new();
    for app in _state.ddic.apps() {
        let viz_json = json!({
            "businessApp": app.name(),
            "target": {
                "action": app.action(),
                "semanticObject": app.main_entity(&_state.ddic).name()
            },
            "vizConfig": {
                "sap.app": {
                    "icon": "sap-icon://settings",
                    "info": "",
                    "subTitle": "...",
                    "title": app.title()
                },
                "sap.flp": {
                    "target": {
                        "appId": app.name(),
                        "inboundId": format!("{}-{}", app.main_entity(&_state.ddic).name(), app.action())
                    }
                }
            },
            "vizType": "sap.ushell.StaticAppLauncher"
        });
        let viz_name = format!(
            "{}-{}-viz",
            app.main_entity(&_state.ddic).name(),
            app.action()
        );
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

pub async fn app_component_handler(
    Path(app): Path<String>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    debug!("Component.js for {}", app);

    let a = _state
        .ddic
        .apps()
        .find(|x| x.main_entity(&_state.ddic).name() == app)
        .unwrap();

    dbg!(a);

    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        format!(
            r##"
sap.ui.define(["sap/fe/core/AppComponent"], function (AppComponent) {{
    "use strict";
	return AppComponent.extend("{cmp}.Component", {{ metadata: {{ manifest: "json" }} }});
}}); "##,
            cmp = a.name()
        ),
    )
}

pub async fn app_manifest_handler(
    Path(app): Path<String>,
    State(_state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    debug!("manifest.json for {}", app);

    let a = _state
        .ddic
        .apps()
        .find(|x| x.main_entity(&_state.ddic).name() == app)
        .unwrap();
    dbg!(a);
    let main_entity = a.main_entity(&_state.ddic);
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
          "controlAggregation": "beginColumnPages",
          "name": "sap.fe.templates.ListReport",
          "options": {
            "settings": {
              "entitySet": main_set.clone(),
              "contextPath": format!("/{}", &main_set),
              "initialLoad": "Enabled",
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
    targets.insert(
        object_target.clone(),
        json!({
            "type": "Component",
            "controlAggregation": "midColumnPages",
            "id": object_target.clone(),
            "name": "sap.fe.templates.ObjectPage",
            "options": {
              "settings": {
                "entitySet": main_set.clone(),
                "contextPath": format!("/{}", &main_set),
              }
            },
        }),
    );

    let mut object_navigation: serde_json::Map<String, Value> = serde_json::Map::new();

    for rel in _state.ddic.relationships().filter(|r| r.one() == main_name) {
        let nav = rel.name().to_string();
        let child_entity = rel.many().to_string();
        let child_object_target = format!("{}ObjectPage", child_entity);

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

        object_navigation.insert(
            nav,
            json!({
              "detail": {
                "route": child_entity.to_string() + "ObjectPage"
              }
            }),
        );
    }

    let sem = format!("{}-{}", a.main_entity(&_state.ddic).name(), a.action());
    let sap_app = json!({
      "applicationVersion": {
        "version": "1.0.0"
      },
      "crossNavigation": {
        "inbounds": {
          sem: {
            "action": a.action(),
            "semanticObject": a.main_entity(&_state.ddic).name(),
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
          "minUI5Version": "1.139.0"
        },
        "flexEnabled": true,
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

pub async fn app_handler(
    Path((app, path)): Path<(String, String)>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    debug!("{} {}", app, path);

    let a = _state
        .ddic
        .apps()
        .find(|x| x.main_entity(&_state.ddic).name() == app);
    debug!("{:?}", a);

    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        "",
    )
}

pub async fn index() -> Html<String> {
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
    Html(out_format.into())
}
