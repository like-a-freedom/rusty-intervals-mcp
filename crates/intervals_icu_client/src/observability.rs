pub struct Health {
    pub ready: bool,
}

impl Health {
    pub fn readiness() -> Self {
        Self { ready: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_ok() {
        let h = Health::readiness();
        assert!(h.ready);
    }
}
