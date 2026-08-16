use std::{env, fs, path::PathBuf};

use toml::Value;

const GENERATED_RULES: &str = "src/verifier_rules_generated.rs";

fn main() {
    println!("cargo:rerun-if-changed=intrinsics.toml");
    println!("cargo:rerun-if-changed=supported-runtime.toml");
    println!("cargo:rerun-if-changed=lambda-bootstrap-protocols.toml");
    println!("cargo:rerun-if-changed={GENERATED_RULES}");
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
    fs::write(
        output.join("jvm_lambda_protocols.rs"),
        generate_lambda_protocols(),
    )
    .expect("write generated lambda bootstrap protocols");

    let rules = generate_verifier_rules();
    if env::var_os("SIM_REGENERATE_JVM_VERIFIER_RULES").is_some() {
        fs::write(GENERATED_RULES, &rules).expect("write generated verifier rules");
    } else {
        let checked = fs::read_to_string(GENERATED_RULES)
            .expect("read generated verifier rules; regenerate with SIM_REGENERATE_JVM_VERIFIER_RULES=1 cargo check -p sim-lib-lang-jvm");
        assert_eq!(
            checked, rules,
            "generated verifier rules are stale or hand-edited; regenerate with SIM_REGENERATE_JVM_VERIFIER_RULES=1 cargo check -p sim-lib-lang-jvm"
        );
    }
}

fn generate_lambda_protocols() -> String {
    let source = fs::read_to_string("lambda-bootstrap-protocols.toml")
        .expect("read lambda bootstrap protocol manifest");
    let manifest: Value = source
        .parse()
        .expect("parse lambda bootstrap protocol manifest");
    assert_eq!(
        manifest["schema"].as_str(),
        Some("sim.jvm-lambda-bootstrap-protocols/v1")
    );
    let owner = manifest["owner"].as_str().expect("lambda bootstrap owner");
    let protocols = manifest["protocol"]
        .as_array()
        .expect("lambda bootstrap protocol array");
    let admitted_flags = manifest["flags"]["admitted_mask"]
        .as_integer()
        .expect("lambda admitted flag mask");
    let mut reference_kinds = manifest["reference_kinds"]
        .as_table()
        .expect("lambda reference kinds")
        .values()
        .map(|value| value.as_integer().expect("lambda reference kind"))
        .collect::<Vec<_>>();
    reference_kinds.sort_unstable();
    reference_kinds.dedup();
    let mut generated = String::from("LambdaBootstrapRegistry { protocols: &[\n");
    for protocol in protocols {
        let name = protocol["name"].as_str().expect("lambda protocol name");
        let descriptor = protocol["descriptor"]
            .as_str()
            .expect("lambda protocol descriptor");
        let tail = match protocol["tail"].as_str().expect("lambda protocol tail") {
            "none" => "LambdaProtocolTail::None",
            "flag-governed" => "LambdaProtocolTail::FlagGoverned",
            other => panic!("unknown lambda protocol tail {other}"),
        };
        generated.push_str(&format!(
            "    LambdaBootstrapProtocol {{ owner: {owner:?}, name: {name:?}, descriptor: {descriptor:?}, tail: {tail} }},\n"
        ));
    }
    generated.push_str(&format!(
        "], admitted_flags_mask: {admitted_flags}, reference_kinds: &{reference_kinds:?} }}\n"
    ));
    generated
}

fn generate_verifier_rules() -> String {
    let source = fs::read_to_string("supported-runtime.toml").expect("read JVM runtime manifest");
    let manifest: Value = source.parse().expect("parse JVM runtime manifest");
    let declarations = manifest["verifier_rule"]
        .as_array()
        .expect("verifier_rule array");
    let mut owners: [Option<&str>; 256] = [None; 256];
    for declaration in declarations {
        let family = declaration["family"]
            .as_str()
            .expect("verifier rule family");
        let variant = match family {
            "constants-locals-stack" => "ConstantsLocalsStack",
            "numeric-conversion" => "NumericConversion",
            "control-return" => "ControlReturn",
            "object-array-field" => "ObjectArrayField",
            "explicit-refusal" => "ExplicitRefusal",
            other => panic!("unknown verifier rule family {other}"),
        };
        for range in declaration["ranges"]
            .as_array()
            .expect("verifier rule ranges")
        {
            let range = range.as_str().expect("verifier rule range string");
            let (start, end) = range.split_once('-').expect("verifier range START-END");
            let start = u8::from_str_radix(start, 16).expect("hex verifier range start");
            let end = u8::from_str_radix(end, 16).expect("hex verifier range end");
            assert!(start <= end, "reversed verifier rule range {range}");
            for byte in start..=end {
                assert!(
                    owners[usize::from(byte)].replace(variant).is_none(),
                    "opcode {byte:#04x} has duplicate verifier rule ownership"
                );
            }
        }
    }
    let mut generated = String::from(
        "// @generated by build.rs from supported-runtime.toml and sim-codec-classfile::OPCODES; DO NOT EDIT.\n\n",
    );
    generated.push_str("/// Dense execution family prepared from the shared opcode manifest.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum PreparedDispatchFamily {\n    /// Constants, locals, and operand-stack operations.\n    Storage,\n    /// Arithmetic, comparison, and conversion operations.\n    Numeric,\n    /// Branch, switch, and return operations.\n    Control,\n    /// Object, array, field, invocation, and allocation operations.\n    Object,\n}\n\n");
    generated.push_str("/// Byte-indexed dispatch identity generated from the one supported-runtime manifest.\npub const PREPARED_DISPATCH: [Option<PreparedDispatchFamily>; 256] = [\n");
    for owner in &owners {
        let family = match owner.expect("complete prepared dispatch ownership") {
            "ConstantsLocalsStack" => "Some(PreparedDispatchFamily::Storage)",
            "NumericConversion" => "Some(PreparedDispatchFamily::Numeric)",
            "ControlReturn" => "Some(PreparedDispatchFamily::Control)",
            "ObjectArrayField" => "Some(PreparedDispatchFamily::Object)",
            "ExplicitRefusal" => "None",
            other => panic!("unknown prepared dispatch owner {other}"),
        };
        generated.push_str(&format!("    {family},\n"));
    }
    generated.push_str("];\n\n");
    generated.push_str("/// Complete byte-indexed verifier rule ownership table.\n");
    generated.push_str("pub static VERIFIER_RULES: [VerifierRule; 256] = [\n");
    for (byte, metadata) in sim_codec_classfile::OPCODES.iter().enumerate() {
        assert_eq!(
            usize::from(metadata.opcode as u8),
            byte,
            "shared opcode table is not byte-indexed"
        );
        let owner = owners[byte].unwrap_or_else(|| {
            panic!(
                "opcode {} ({}) has no verifier rule owner",
                byte, metadata.mnemonic
            )
        });
        generated.push_str(&format!(
            "    VerifierRule {{ opcode: Opcode::{:?}, family: VerifierRuleFamily::{owner} }},\n",
            metadata.opcode
        ));
    }
    generated.push_str("];\n");
    let family_count = owners
        .iter()
        .flatten()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    generated.push_str(&format!(
        "\n/// Coverage derived with the rule table from the owning manifest.\npub const VERIFIER_COVERAGE: VerifierCoverage = VerifierCoverage {{ opcode_rows: 256, rule_families: {family_count}, source: \"supported-runtime.toml + sim-codec-classfile::OPCODES\" }};\n"
    ));
    let inventories = fs::read_dir("src")
        .expect("read JVM source directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("rs"))
        .filter(|entry| {
            fs::read_to_string(entry.path())
                .is_ok_and(|source| source.contains("static VERIFIER_RULES: [VerifierRule; 256]"))
        })
        .count();
    assert_eq!(
        inventories, 1,
        "verifier opcode ownership must have exactly one generated inventory"
    );
    generated
}
