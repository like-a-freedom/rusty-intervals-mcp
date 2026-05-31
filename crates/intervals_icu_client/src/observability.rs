pub struct Health {
    pub ready: bool,
}

impl Health {
    #[must_use]
    pub fn readiness() -> Self {
        Self { ready: true }
    }
}
