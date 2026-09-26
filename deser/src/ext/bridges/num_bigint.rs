use ::num_bigint::BigUint;

use crate::error::Error;
use crate::event::Atom;
use crate::ext::BigInt;
use crate::ext::known::{Bridge, impl_bridge, out_of_range};

super::num_bigint_conversions!(::num_bigint);

impl Bridge for ::num_bigint::BigInt {
    type Known = BigInt;

    const EXPECTING: &'static str = "integer";

    fn to_known(&self) -> Result<BigInt, Error> {
        Ok(from_num(self))
    }

    fn from_known(value: BigInt) -> Result<::num_bigint::BigInt, Error> {
        Ok(to_num(&value))
    }

    /// Integers that fit into 64 or 128 bits are serialized as such.
    fn serialize_atom(&self) -> Result<Atom<'static>, Error> {
        Ok(from_num(self).into_atom())
    }
}

impl Bridge for BigUint {
    type Known = BigInt;

    const EXPECTING: &'static str = "unsigned integer";

    fn to_known(&self) -> Result<BigInt, Error> {
        Ok(BigInt {
            negative: false,
            magnitude: self.to_bytes_be(),
        })
    }

    fn from_known(value: BigInt) -> Result<BigUint, Error> {
        if value.is_negative() {
            return Err(out_of_range("integer cannot be negative"));
        }
        Ok(BigUint::from_bytes_be(&value.magnitude))
    }

    fn serialize_atom(&self) -> Result<Atom<'static>, Error> {
        Ok(self.to_known()?.into_atom())
    }
}

impl_bridge!(::num_bigint::BigInt, BigUint);
