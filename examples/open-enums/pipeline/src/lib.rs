//! A text processing pipeline whose steps are an open enum: the steps
//! of this crate are variants, other crates can add more (see
//! `pipeline-extras`).  The program registers the steps it accepts.
use deser::{Deserialize, Error, OpenEnums, Serialize};

/// Registers the steps of this crate.
pub fn register(variants: &mut OpenEnums) -> Result<(), Error> {
    variants
        .register::<dyn Step, Replace>()?
        .register::<dyn Step, Trim>()?;
    Ok(())
}

/// A step of the pipeline.
///
/// The steps are internally tagged (`{"type": "replace", ...}`) and named
/// in snake case.  Steps that are unit structs are the tag alone.
#[deser::open_enum(tag = "type", rename_all = "snake_case")]
pub trait Step: std::fmt::Debug + Send + Sync {
    /// Processes the text.
    fn run(&self, input: String) -> String;
}

/// A pipeline of steps.
#[derive(Debug, Serialize, Deserialize)]
pub struct Pipeline {
    pub name: String,
    pub steps: Vec<Box<dyn Step>>,
}

impl Pipeline {
    /// Runs all steps.
    pub fn run(&self, input: &str) -> String {
        let mut text = input.to_string();
        for step in &self.steps {
            text = step.run(text);
        }
        text
    }
}

/// Replaces all occurrences of a string.
#[derive(Debug, Serialize, Deserialize)]
pub struct Replace {
    pub from: String,
    pub to: String,
}

#[deser::variant]
impl Step for Replace {
    fn run(&self, input: String) -> String {
        input.replace(&self.from, &self.to)
    }
}

/// Removes whitespace at the start and the end.
#[derive(Debug, Serialize, Deserialize)]
pub struct Trim;

#[deser::variant]
impl Step for Trim {
    fn run(&self, input: String) -> String {
        input.trim().to_string()
    }
}
