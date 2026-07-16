use serde_json::json;
use spec::data_dictionary::DataDictonary;

fn main() {
    println!("boostrap");

    let ddic = DataDictonary::new();

    ddic.apps().for_each(|app| {
        let app_json = json!({
            "sap.app":{
                "crossNavigation":{
                    "inbounds":{
                        format!("{}-{}", app.main_entity(), app.action()):{
                            "action":app.action(),
                            "semanticObject":app.main_entity(),
                            "signature":{
                                "additionalParameters":"allowed",
                                "parameters":{}}}}},
                                "id":app.name(),
                                "subTitle":"...",
                                "title":app.title()},
                                "sap.flp":{
                                    "type":"application"},
                                    "sap.platform.runtime":{
                                        "componentProperties":{
                                            "url":app.url()}},
                                            "sap.ui":{
                                                "deviceTypes":{
                                                    "desktop":true,
                                                    "phone":true,
                                                    "tablet":true},
                                                    "technology":"UI5"},
                                                    "sap.ui5":{
                                                        "componentName":app.name()}});
        println!("{}", app_json.to_string());
    });
}
