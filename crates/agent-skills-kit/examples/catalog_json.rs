//! Export the validated bundled catalog without inspecting a host or environment.
use agent_skills_kit::catalog::Catalog;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::bundled()?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &catalog.document())?;
    Ok(())
}
