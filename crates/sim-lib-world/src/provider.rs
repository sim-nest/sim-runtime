use sim_incremental_core::projection::{ConfigShapeVerifier, ProjectionError};
use sim_kernel::{ContentId, Datum, Symbol};

pub(crate) const PROVIDER_SOURCE: &str = include_str!("provider.rs");

pub(crate) struct WorldConfigShape {
    pub(crate) id: ContentId,
}

impl ConfigShapeVerifier for WorldConfigShape {
    fn verify(&self, shape: &ContentId, config: &Datum) -> Result<(), String> {
        if shape != &self.id {
            return Err("unknown world projection config Shape".to_owned());
        }
        let Datum::Node { tag, fields } = config else {
            return Err("world projection config must be a world/config-v1 node".to_owned());
        };
        if tag != &Symbol::qualified("world", "config-v1") {
            return Err("world projection config has the wrong Shape tag".to_owned());
        }
        if !fields.is_empty() {
            return Err("world/config-v1 has no open fields".to_owned());
        }
        Ok(())
    }
}

pub(crate) fn config() -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "config-v1"),
        fields: Vec::new(),
    }
}

pub(crate) fn content_id(value: Datum) -> Result<ContentId, ProjectionError> {
    value
        .content_id()
        .map_err(|error| ProjectionError::Canonical(error.to_string()))
}
