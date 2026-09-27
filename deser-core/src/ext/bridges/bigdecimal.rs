use ::bigdecimal::BigDecimal;

use crate::error::Error;
use crate::ext::Decimal;
use crate::ext::known::{Bridge, impl_bridge, out_of_range};

super::num_bigint_conversions!(::bigdecimal::num_bigint);

impl Bridge for BigDecimal {
    type Known = Decimal;

    fn to_known(&self) -> Result<Decimal, Error> {
        let (mantissa, scale) = self.as_bigint_and_exponent();
        let exponent = scale
            .checked_neg()
            .ok_or_else(|| out_of_range("decimal out of range"))?;
        Ok(Decimal::from_parts(&from_num(&mantissa), exponent))
    }

    fn from_known(value: Decimal) -> Result<BigDecimal, Error> {
        let (mantissa, exponent) = value.to_parts();
        let scale = exponent
            .checked_neg()
            .ok_or_else(|| out_of_range("decimal out of range"))?;
        Ok(BigDecimal::new(to_num(&mantissa), scale))
    }
}

impl_bridge!(BigDecimal);
