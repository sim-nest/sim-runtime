fn main() {
    sim_cookbook::write_embed("recipes").expect("embed JavaScript recipes");
    println!("cargo:rerun-if-changed=intrinsics.tsv");
    let source = std::fs::read_to_string("intrinsics.tsv").expect("read JavaScript intrinsics");
    let mut names = std::collections::BTreeSet::new();
    let mut generated = String::from("&[\n");
    for (line_number, line) in source.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            3,
            "intrinsics.tsv:{} needs name, kind, backing",
            line_number + 1
        );
        assert!(names.insert(fields[0]), "duplicate intrinsic {}", fields[0]);
        generated.push_str(&format!(
            "JavascriptIntrinsic {{ name: {:?}, kind: {:?}, backing: {:?} }},\n",
            fields[0], fields[1], fields[2]
        ));
    }
    generated.push_str("]\n");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("javascript_intrinsics.rs"), generated)
        .expect("write JavaScript intrinsic manifest");
}
