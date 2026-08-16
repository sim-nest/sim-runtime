use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{BoxValue, IntrinsicError, PrimitiveBoxes, admit_intrinsic};

#[test]
fn cached_boxes_preserve_identity_at_each_boundary() {
    let mut boxes = PrimitiveBoxes::new(
        32,
        CollectionLimits {
            objects: 32,
            edges: 32,
            stack: 32,
            work: 256,
            clears: 32,
            finalizers: 0,
        },
    )
    .unwrap();
    for value in [
        BoxValue::Integer(-128),
        BoxValue::Integer(127),
        BoxValue::Character(127),
        BoxValue::Boolean(false),
    ] {
        let left = boxes.box_value(value).unwrap();
        let right = boxes.box_value(value).unwrap();
        assert_eq!(left.handle(), right.handle());
        assert_eq!(left.identity_hash(), right.identity_hash());
        assert_ne!(left.identity_hash(), 0);
    }
    let outside = boxes.box_value(BoxValue::Integer(128)).unwrap();
    let again = boxes.box_value(BoxValue::Integer(128)).unwrap();
    assert_ne!(outside.handle(), again.handle());
}

#[test]
fn unsupported_member_names_itself_before_effect_dispatch() {
    let error = admit_intrinsic("java/lang/Integer", "<init>", "(I)V").unwrap_err();
    assert_eq!(
        error,
        IntrinsicError::Unsupported {
            class: "java/lang/Integer",
            name: "<init>",
            descriptor: "(I)V"
        }
    );
    assert_eq!(
        error.to_string(),
        "unsupported intrinsic java/lang/Integer.<init>(I)V"
    );
}

#[test]
fn crate_has_no_ad_hoc_member_name_switch() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains("match name"),
            "ad-hoc member-name match in {}",
            path.display()
        );
        assert!(
            !source.contains("match member.name"),
            "ad-hoc member-name match in {}",
            path.display()
        );
    }
}
