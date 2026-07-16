#[derive(Clone, Debug)]
pub struct Package {
    name: String,
}

impl Package {
    pub fn new(name: &str) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}
