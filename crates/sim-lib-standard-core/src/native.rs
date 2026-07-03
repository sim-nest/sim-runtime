//! Native ABI proof exports for the standard-core crate.

use std::ffi::{CStr, c_char, c_void};

use sim_kernel::{
    AbiVersion, Export, Expr, LibManifest, LibTarget, NativeAbiBorrowedBytes,
    NativeAbiCallResponse, NativeAbiError, NativeAbiOwnedBytes, NativeLibAbiV1, NumberLiteral,
    Result, Symbol, Version, native_abi_owned_bytes,
};

struct NativeStandardCore;

#[allow(unsafe_code)]
unsafe extern "C" fn instantiate() -> *mut c_void {
    Box::into_raw(Box::new(NativeStandardCore)).cast::<c_void>()
}

#[allow(unsafe_code)]
unsafe extern "C" fn destroy_instance(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(instance.cast::<NativeStandardCore>()));
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn manifest(instance: *mut c_void) -> NativeAbiCallResponse {
    if instance.is_null() {
        return failure("native standard-core manifest received a null instance");
    }
    success_expr(&manifest_to_expr(&native_manifest()))
}

#[allow(unsafe_code)]
unsafe extern "C" fn call(
    instance: *mut c_void,
    function: *const c_char,
    args: NativeAbiBorrowedBytes,
) -> NativeAbiCallResponse {
    if instance.is_null() {
        return failure("native standard-core call received a null instance");
    }
    if function.is_null() {
        return failure("native standard-core call received a null function symbol");
    }
    let function = unsafe { CStr::from_ptr(function) }
        .to_string_lossy()
        .into_owned();
    let arg_bytes = if args.ptr.is_null() && args.len == 0 {
        &[][..]
    } else if args.ptr.is_null() {
        return failure("native standard-core call received null argument bytes");
    } else {
        unsafe { std::slice::from_raw_parts(args.ptr, args.len) }
    };
    let expr = match sim_codec_binary::decode_frame(sim_kernel::CodecId(0), arg_bytes) {
        Ok((_, expr)) => expr,
        Err(err) => return failure(err.to_string()),
    };
    match call_expr(function.as_str(), expr) {
        Ok(expr) => success_expr(&expr),
        Err(err) => failure(err.to_string()),
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn destroy_bytes(bytes: NativeAbiOwnedBytes) {
    if !bytes.ptr.is_null() {
        unsafe {
            drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
        }
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn destroy_error(error: *mut NativeAbiError) {
    if error.is_null() {
        return;
    }
    let error = unsafe { Box::from_raw(error) };
    if !error.message.is_null() {
        unsafe {
            drop(std::ffi::CString::from_raw(error.message));
        }
    }
}

static ABI: NativeLibAbiV1 = NativeLibAbiV1::new(
    instantiate,
    destroy_instance,
    manifest,
    call,
    destroy_bytes,
    destroy_error,
);

/// Returns the standard-core native ABI vtable.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn sim_native_abi_v1() -> *const NativeLibAbiV1 {
    &ABI
}

fn native_manifest() -> LibManifest {
    LibManifest {
        id: Symbol::qualified("standard", "core-native-proof"),
        version: Version(env!("CARGO_PKG_VERSION").to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target: LibTarget::HostRegistered,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: vec![
            Export::Class {
                symbol: proof_box_symbol(),
                class_id: None,
            },
            Export::Function {
                symbol: Symbol::qualified("standard/proof-box", "value"),
                function_id: None,
            },
            Export::Macro {
                symbol: proof_quote_symbol(),
                macro_id: None,
            },
        ],
    }
}

fn call_expr(function: &str, expr: Expr) -> Result<Expr> {
    match function {
        "standard/proof-box/new" => proof_box_new(expr),
        "standard/proof-box/value" | "standard/proof-box.value" => proof_box_value(expr),
        "standard/proof-quote/expand" => proof_quote_expand(expr),
        _ => Err(sim_kernel::Error::UnknownFunction {
            function: Symbol::new(function),
        }),
    }
}

fn proof_box_new(expr: Expr) -> Result<Expr> {
    let Expr::List(args) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    let [value] = args.as_slice() else {
        return Err(sim_kernel::Error::Eval(format!(
            "{} expects 1 arg, got {}",
            proof_box_symbol(),
            args.len()
        )));
    };
    Ok(proof_box_expr(value.clone()))
}

fn proof_box_value(expr: Expr) -> Result<Expr> {
    let Expr::List(args) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    let [instance] = args.as_slice() else {
        return Err(sim_kernel::Error::Eval(format!(
            "{}/value expects 1 arg, got {}",
            proof_box_symbol(),
            args.len()
        )));
    };
    proof_box_field(instance, &Symbol::new("value"))
}

fn proof_quote_expand(expr: Expr) -> Result<Expr> {
    let Expr::List(items) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "list",
            found: "non-list",
        });
    };
    items.get(1).cloned().ok_or_else(|| {
        sim_kernel::Error::Eval(format!("{} expects one quoted form", proof_quote_symbol()))
    })
}

fn proof_box_expr(value: Expr) -> Expr {
    Expr::Extension {
        tag: Symbol::qualified("expr", "object"),
        payload: Box::new(Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("class")),
                Expr::Symbol(proof_box_symbol()),
            ),
            (
                Expr::Symbol(Symbol::new("fields")),
                Expr::Map(vec![(Expr::Symbol(Symbol::new("value")), value)]),
            ),
        ])),
    }
}

fn proof_box_field(expr: &Expr, field: &Symbol) -> Result<Expr> {
    let object = parse_object_expr(expr)?;
    if object.0 != proof_box_symbol() {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "standard/proof-box",
            found: "different class",
        });
    }
    object
        .1
        .into_iter()
        .find_map(|(key, value)| (key == *field).then_some(value))
        .ok_or_else(|| sim_kernel::Error::UnknownSymbol {
            symbol: field.clone(),
        })
}

fn parse_object_expr(expr: &Expr) -> Result<(Symbol, Vec<(Symbol, Expr)>)> {
    let Expr::Extension { tag, payload } = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "object",
            found: "non-object",
        });
    };
    if *tag != Symbol::qualified("expr", "object") {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "expr/object",
            found: "different extension",
        });
    }
    let Expr::Map(entries) = payload.as_ref() else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "object map",
            found: "non-map",
        });
    };
    let class = map_field(entries, &Symbol::new("class")).and_then(|expr| match expr {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected: "class symbol",
            found: "non-symbol",
        }),
    })?;
    let fields = map_field(entries, &Symbol::new("fields")).and_then(|expr| match expr {
        Expr::Map(entries) => entries
            .iter()
            .map(|(key, value)| match key {
                Expr::Symbol(symbol) => Ok((symbol.clone(), value.clone())),
                _ => Err(sim_kernel::Error::TypeMismatch {
                    expected: "field symbol",
                    found: "non-symbol",
                }),
            })
            .collect(),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected: "fields map",
            found: "non-map",
        }),
    })?;
    Ok((class, fields))
}

fn map_field<'a>(entries: &'a [(Expr, Expr)], field: &Symbol) -> Result<&'a Expr> {
    entries
        .iter()
        .find_map(|(key, value)| match key {
            Expr::Symbol(symbol) if symbol == field => Some(value),
            _ => None,
        })
        .ok_or_else(|| sim_kernel::Error::UnknownSymbol {
            symbol: field.clone(),
        })
}

fn manifest_to_expr(manifest: &LibManifest) -> Expr {
    Expr::Map(vec![
        symbol_entry("id", Expr::Symbol(manifest.id.clone())),
        symbol_entry("version", Expr::String(manifest.version.0.clone())),
        symbol_entry("abi-major", number_expr(manifest.abi.major)),
        symbol_entry("abi-minor", number_expr(manifest.abi.minor)),
        symbol_entry(
            "target",
            Expr::String(manifest.target.to_symbol().as_qualified_str()),
        ),
        symbol_entry("requires", Expr::List(Vec::new())),
        symbol_entry("capabilities", Expr::List(Vec::new())),
        symbol_entry(
            "exports",
            Expr::List(
                manifest
                    .exports
                    .iter()
                    .map(|export| {
                        let (kind, symbol) = match export {
                            Export::Class { symbol, .. } => ("class", symbol),
                            Export::Function { symbol, .. } => ("function", symbol),
                            Export::Macro { symbol, .. } => ("macro", symbol),
                            Export::Shape { symbol, .. } => ("shape", symbol),
                            Export::Codec { symbol, .. } => ("codec", symbol),
                            Export::NumberDomain { symbol, .. } => ("number-domain", symbol),
                            Export::Site { symbol, .. } => ("site", symbol),
                            Export::Value { symbol } => ("value", symbol),
                        };
                        Expr::Map(vec![
                            symbol_entry("kind", Expr::String(kind.to_owned())),
                            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn symbol_entry(key: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(key)), value)
}

fn number_expr(value: impl ToString) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "f64"),
        canonical: value.to_string(),
    })
}

fn proof_box_symbol() -> Symbol {
    Symbol::qualified("standard", "proof-box")
}

fn proof_quote_symbol() -> Symbol {
    Symbol::qualified("standard", "proof-quote")
}

fn success_expr(expr: &Expr) -> NativeAbiCallResponse {
    match sim_codec_binary::encode_frame(expr) {
        Ok(frame) => NativeAbiCallResponse::success(native_abi_owned_bytes(frame.0)),
        Err(err) => failure(err.to_string()),
    }
}

fn failure(message: impl Into<String>) -> NativeAbiCallResponse {
    NativeAbiCallResponse::failure(NativeAbiError::boxed(message))
}
