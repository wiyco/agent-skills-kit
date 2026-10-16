//! Versioned observational receipts. Parsing never grants filesystem authority.

mod path;
mod schema;
mod validation;

pub use path::*;
pub use schema::*;
pub use validation::*;
