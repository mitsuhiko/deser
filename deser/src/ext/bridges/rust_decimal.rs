use crate::error::Error;
use crate::ext::Decimal;
use crate::ext::known::{Bridge, impl_bridge, invalid};

impl Bridge for ::rust_decimal::Decimal {
    type Known = Decimal;

    fn to_known(&self) -> Result<Decimal, Error> {
        Decimal::new(self.to_string())
    }

    /// Converts a decimal, fails if it cannot be represented exactly.
    fn from_known(value: Decimal) -> Result<::rust_decimal::Decimal, Error> {
        let s = value.as_str();
        let rv = if s.contains(['e', 'E']) {
            ::rust_decimal::Decimal::from_scientific(s)
        } else {
            ::rust_decimal::Decimal::from_str_exact(s)
        };
        rv.map_err(|err| invalid(format!("cannot represent decimal: {}", err)))
    }
}

impl_bridge!(::rust_decimal::Decimal);
