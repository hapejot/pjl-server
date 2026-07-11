use axum::{
    Json, Router,
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};
use serde_json::{Value, json};
// use serde::{Deserialize, Serialize};

pub fn router() -> Router {
    Router::new()
        .route("/init.js", get(init))
        .route("/site.json", get(site))
}
async fn site() -> Json<Value> {
    Json(json!({
      "_version": "3.1.0",
      "applications": {
        "entityconfigs.app": {
          "sap.app": {
            "crossNavigation": {
              "inbounds": {
                "EntityConfigs-display": {
                  "action": "display",
                  "semanticObject": "EntityConfigs",
                  "signature": {
                    "additionalParameters": "allowed",
                    "parameters": {}
                  }
                }
              }
            },
            "id": "entityconfigs.app",
            "subTitle": "Generische Entitaeten verwalten",
            "title": "Entity-Konfigurationen"
          },
          "sap.flp": {
            "type": "application"
          },
          "sap.platform.runtime": {
            "componentProperties": {
              "url": "./apps/EntityConfigs/"
            }
          },
          "sap.ui": {
            "deviceTypes": {
              "desktop": true,
              "phone": true,
              "tablet": true
            },
            "technology": "UI5"
          },
          "sap.ui5": {
            "componentName": "entityconfigs.app"
          }
        },
        "entityrelationships.app": {
          "sap.app": {
            "crossNavigation": {
              "inbounds": {
                "EntityRelationships-display": {
                  "action": "display",
                  "semanticObject": "EntityRelationships",
                  "signature": {
                    "additionalParameters": "allowed",
                    "parameters": {}
                  }
                }
              }
            },
            "id": "entityrelationships.app",
            "subTitle": "Entity-Beziehungen",
            "title": "Relationships"
          },
          "sap.flp": {
            "type": "application"
          },
          "sap.platform.runtime": {
            "componentProperties": {
              "url": "./apps/EntityRelationships/"
            }
          },
          "sap.ui": {
            "deviceTypes": {
              "desktop": true,
              "phone": true,
              "tablet": true
            },
            "technology": "UI5"
          },
          "sap.ui5": {
            "componentName": "entityrelationships.app"
          }
        },
        "fieldvaluelists.app": {
          "sap.app": {
            "crossNavigation": {
              "inbounds": {
                "FieldValueLists-display": {
                  "action": "display",
                  "semanticObject": "FieldValueLists",
                  "signature": {
                    "additionalParameters": "allowed",
                    "parameters": {}
                  }
                }
              }
            },
            "id": "fieldvaluelists.app",
            "subTitle": "Feste Wertelisten verwalten",
            "title": "Wertelisten"
          },
          "sap.flp": {
            "type": "application"
          },
          "sap.platform.runtime": {
            "componentProperties": {
              "url": "./apps/FieldValueLists/"
            }
          },
          "sap.ui": {
            "deviceTypes": {
              "desktop": true,
              "phone": true,
              "tablet": true
            },
            "technology": "UI5"
          },
          "sap.ui5": {
            "componentName": "fieldvaluelists.app"
          }
        }
      },
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
                  "vizOrder": [
                    "EntityConfigs-display-viz",
                    "EntityRelationships-display-viz",
                    "FieldValueLists-display-viz"
                  ]
                },
                "locked": false,
                "preset": true,
                "title": "Applications",
                "visible": true,
                "viz": {
                  "EntityConfigs-display-viz": {
                    "id": "EntityConfigs-display-viz",
                    "vizId": "EntityConfigs-display-viz"
                  },
                  "EntityRelationships-display-viz": {
                    "id": "EntityRelationships-display-viz",
                    "vizId": "EntityRelationships-display-viz"
                  },
                  "FieldValueLists-display-viz": {
                    "id": "FieldValueLists-display-viz",
                    "vizId": "FieldValueLists-display-viz"
                  }
                }
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
      "visualizations": {
        "EntityConfigs-display-viz": {
          "businessApp": "entityconfigs.app",
          "target": {
            "action": "display",
            "semanticObject": "EntityConfigs"
          },
          "vizConfig": {
            "sap.app": {
              "icon": "sap-icon://settings",
              "info": "",
              "subTitle": "Generische Entitaeten verwalten",
              "title": "Entity-Konfigurationen"
            },
            "sap.flp": {
              "target": {
                "appId": "entityconfigs.app",
                "inboundId": "EntityConfigs-display"
              }
            }
          },
          "vizType": "sap.ushell.StaticAppLauncher"
        },
        "EntityRelationships-display-viz": {
          "businessApp": "entityrelationships.app",
          "target": {
            "action": "display",
            "semanticObject": "EntityRelationships"
          },
          "vizConfig": {
            "sap.app": {
              "icon": "sap-icon://connected",
              "info": "",
              "subTitle": "Entity-Beziehungen",
              "title": "Relationships"
            },
            "sap.flp": {
              "target": {
                "appId": "entityrelationships.app",
                "inboundId": "EntityRelationships-display"
              }
            }
          },
          "vizType": "sap.ushell.StaticAppLauncher"
        },
        "FieldValueLists-display-viz": {
          "businessApp": "fieldvaluelists.app",
          "target": {
            "action": "display",
            "semanticObject": "FieldValueLists"
          },
          "vizConfig": {
            "sap.app": {
              "icon": "sap-icon://value-help",
              "info": "",
              "subTitle": "Feste Wertelisten verwalten",
              "title": "Wertelisten"
            },
            "sap.flp": {
              "target": {
                "appId": "fieldvaluelists.app",
                "inboundId": "FieldValueLists-display"
              }
            }
          },
          "vizType": "sap.ushell.StaticAppLauncher"
        }
      },
      "vizTypes": {}
    }))
}
async fn init() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../static/flp/init.js"),
    )
}

pub async fn index() -> Html<String> {
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
