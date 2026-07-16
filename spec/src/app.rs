use crate::package::Package;

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

    pub fn main_entity(&self) -> &str {
        &self.main_entity
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
