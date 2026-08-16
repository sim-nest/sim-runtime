use std::{env, fs, path::PathBuf};

use toml::Value;

fn main() {
    println!("cargo:rerun-if-changed=intrinsics.toml");
    let source = fs::read_to_string("intrinsics.toml").expect("read JVM intrinsic manifest");
    let manifest: Value = source.parse().expect("parse JVM intrinsic manifest");
    assert_eq!(
        manifest["schema"].as_str(),
        Some("sim.jvm-intrinsic-manifest/v1")
    );
    let members = manifest["members"]
        .as_array()
        .expect("intrinsic manifest members array");
    let mut generated = String::from("&[\n");
    for member in members {
        let text = |key: &str| {
            member[key]
                .as_str()
                .unwrap_or_else(|| panic!("{key} string"))
        };
        let support = match text("support") {
            "supported" => "IntrinsicSupport::Supported",
            "unsupported" => "IntrinsicSupport::Unsupported",
            other => panic!("unknown intrinsic support {other}"),
        };
        generated.push_str(&format!(
            "    IntrinsicMember {{ class: {:?}, name: {:?}, descriptor: {:?}, arguments_shape: {:?}, result_shape: {:?}, capability: {:?}, effect: {:?}, work: {}, support: {support} }},\n",
            text("class"),
            text("name"),
            text("descriptor"),
            text("arguments_shape"),
            text("result_shape"),
            text("capability"),
            text("effect"),
            member["work"].as_integer().expect("intrinsic work integer"),
        ));
    }
    generated.push_str("]\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(output.join("jvm_intrinsics.rs"), generated).expect("write generated intrinsics");
}
