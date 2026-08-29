use crate::{EntitySpec, data_dictionary::DataDictonary, package::Package};

#[derive(Debug)]
pub struct App {
    name: String,
    title: String,
    main_entity: String,
    subtitle: Option<String>,
    icon: Option<String>,
    package: Package,
}

impl App {
    pub fn new(
        name: String,
        title: String,
        main_entity: String,
        subtitle: Option<String>,
        icon: Option<String>,
        package: Package,
    ) -> Self {
        Self {
            name,
            title,
            main_entity,
            subtitle,
            icon,
            package,
        }
    }
    // getters
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn url(&self) -> String {
        format!("./apps/{}/", self.main_entity)
    }

    pub fn action(&self) -> &str {
        "display"
    }

    // resolvers
    pub fn main_entity<'a>(&self, _ddic: &'a DataDictonary) -> EntitySpec {
        if let Some(entity) = _ddic.active_entity(&self.main_entity) {
            return entity.clone();
        }

        _ddic
            .entities()
            .iter()
            .find(|e| e.name() == self.main_entity)
            .cloned()
            .expect(&format!("Entity {} not found", self.main_entity))
    }

    pub fn subtitle(&self) -> Option<&String> {
        self.subtitle.as_ref()
    }

    pub fn icon(&self) -> Option<&String> {
        self.icon.as_ref()
    }

    pub fn package(&self) -> &Package {
        &self.package
    }
}
