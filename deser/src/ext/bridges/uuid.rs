use crate::error::Error;
use crate::ext::known::{impl_bridge, Bridge};
use crate::ext::Uuid;

impl Bridge for ::uuid::Uuid {
    type Known = Uuid;

    fn to_known(&self) -> Result<Uuid, Error> {
        Ok(Uuid(*self.as_bytes()))
    }

    fn from_known(value: Uuid) -> Result<::uuid::Uuid, Error> {
        Ok(::uuid::Uuid::from_bytes(value.0))
    }

    fn parse_fallback(value: &str) -> Option<::uuid::Uuid> {
        // braced and URN representations
        value.parse().ok()
    }
}

impl_bridge!(::uuid::Uuid);
