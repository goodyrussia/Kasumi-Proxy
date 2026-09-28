//! Regenerates the frontend's generated TypeScript from the Rust types —
//! `frontend/src/generated/{bindings,schemas,defaults}.ts`. Run it after any change
//! to a `specta::Type`/serde shape, a serde default, a `pub const` or an enum
//! variant list; CI's drift check (`--check`) fails when the committed files are
//! stale, which in turn fails the frontend's `tsc` until regenerated.
//!
//!   cargo run -p kasumi-codegen            # write the three files
//!   cargo run -p kasumi-codegen -- --check # verify they are up to date (CI)

mod bindings;
mod cast;
mod defaults;
mod schemas;

use std::path::PathBuf;

/// The generated artifacts: (relative display path, absolute path, fresh content).
fn artifacts() -> Vec<(&'static str, PathBuf, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![
        (
            "frontend/src/generated/bindings.ts",
            root.join("frontend/src/generated/bindings.ts"),
            bindings::render(),
        ),
        (
            "frontend/src/generated/schemas.ts",
            root.join("frontend/src/generated/schemas.ts"),
            schemas::render(),
        ),
        (
            "frontend/src/generated/defaults.ts",
            root.join("frontend/src/generated/defaults.ts"),
            defaults::render(),
        ),
    ]
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    if check {
        let mut stale = Vec::new();
        for (name, path, content) in artifacts() {
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            if current != content {
                stale.push(name);
            }
        }
        if stale.is_empty() {
            println!("generated files are up to date");
        } else {
            eprintln!(
                "generated files are stale: {}\nrun `cargo run -p kasumi-codegen` and commit the result",
                stale.join(", ")
            );
            std::process::exit(1);
        }
    } else {
        for (name, path, content) in artifacts() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create generated dir");
            }
            std::fs::write(&path, &content)
                .unwrap_or_else(|e| panic!("write {name} ({}): {e}", path.display()));
        }
        println!("regenerated frontend/src/generated/{{bindings,schemas,defaults}}.ts");
    }
}
