//! Rejects guest-local copies of the shared managed graph substrate.

use std::{fs, path::Path};

pub fn run(args: Vec<String>) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.get(1).map(String::as_str) != Some("check-managed-substrate") || args.len() != 2 {
        return Err(format!("usage: {program} check-managed-substrate"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "xtask manifest should have a workspace root parent".to_owned())?;
    let mut errors = Vec::new();
    inspect_tree(&root.join("crates"), &mut errors)?;
    if errors.is_empty() {
        println!("check-managed-substrate: OK");
        Ok(())
    } else {
        Err(format!(
            "guest crates must layer payload policy over ManagedNode and ManagedHeap:\n{}",
            errors.join("\n")
        ))
    }
}

fn inspect_tree(dir: &Path, errors: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let path = entry
            .map_err(|e| format!("read {} entry: {e}", dir.display()))?
            .path();
        if path.is_dir() {
            inspect_tree(&path, errors)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs")
            && path.components().any(|part| {
                part.as_os_str()
                    .to_string_lossy()
                    .starts_with("sim-lib-lang-")
            })
        {
            let source =
                fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            inspect_source(&path, &source, errors);
        }
    }
    Ok(())
}

fn inspect_source(path: &Path, source: &str, errors: &mut Vec<String>) {
    let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("ManagedArena<") && compact.contains("struct") {
        errors.push(format!(
            "- {} contains a guest-owned ManagedArena wrapper",
            path.display()
        ));
    }
    let has_node_storage = compact.contains("BTreeMap<EdgeId,ManagedId>")
        || (compact.contains("strong:")
            && compact.contains("weak:")
            && compact.contains("ephemeron"));
    if has_node_storage && compact.contains("implManagedObjectfor") {
        errors.push(format!(
            "- {} duplicates the generic managed node",
            path.display()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_both_frozen_fork_shapes() {
        let mut errors = Vec::new();
        inspect_source(
            Path::new("arena.rs"),
            "struct Heap { arena: ManagedArena<Node> }",
            &mut errors,
        );
        inspect_source(
            Path::new("node.rs"),
            "struct Node { strong: BTreeMap<EdgeId, ManagedId>, weak: BTreeMap<EdgeId, ManagedId>, ephemerons: BTreeMap<EdgeId, (ManagedId, ManagedId)> } impl ManagedObject for Node {}",
            &mut errors,
        );
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn permits_language_policy_over_shared_node() {
        let mut errors = Vec::new();
        inspect_source(
            Path::new("managed.rs"),
            "type GuestObject = ManagedNode<GuestRole>; trait GuestHeapExt { fn connect(&mut self); } impl GuestHeapExt for ManagedHeap<GuestObject> {}",
            &mut errors,
        );
        assert!(errors.is_empty());
    }
}
