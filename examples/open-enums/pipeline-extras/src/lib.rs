//! Steps for the pipeline in another crate.  The steps are variants of
//! `Step` like the ones of `pipeline`.
use deser::{Deserialize, Error, OpenEnums, Serialize};
use pipeline::Step;

/// Registers the steps of this crate.
pub fn register(variants: &mut OpenEnums) -> Result<(), Error> {
    variants
        .register::<dyn Step, Uppercase>()?
        .register::<dyn Step, Repeat>()?;
    Ok(())
}

/// Converts the text to uppercase.
#[derive(Debug, Serialize, Deserialize)]
pub struct Uppercase;

// a shorter name, the old one is still accepted
#[deser::variant(rename = "upper", alias = "uppercase")]
impl Step for Uppercase {
    fn run(&self, input: String) -> String {
        input.to_uppercase()
    }
}

/// Repeats the text.
#[derive(Debug, Serialize, Deserialize)]
pub struct Repeat {
    pub times: usize,
    #[deser(default)]
    pub separator: String,
}

#[deser::variant]
impl Step for Repeat {
    fn run(&self, input: String) -> String {
        vec![input; self.times].join(&self.separator)
    }
}
