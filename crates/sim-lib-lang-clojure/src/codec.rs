use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, Decoder, Input, LocatedDecoder, ReadCx, TreeDecoder,
    codec_value,
};
use sim_kernel::{
    AbiVersion, CodecId, DefaultFactory, Dependency, Export, Lib, LibManifest, LibTarget, Linker,
    LocatedExpr, LocatedExprTree, Result, Symbol, Version,
};

use crate::{clojure_edn_reader_symbol, decode_clojure_edn_tree};

/// Decoder that reads Clojure/EDN surface syntax into the shared [`Expr`](sim_kernel::Expr) graph.
///
/// Implements the kernel [`Decoder`], [`LocatedDecoder`], and [`TreeDecoder`]
/// contracts; this profile only decodes (surface -> `Expr`) and registers no
/// encoder. See the [crate] README for the language-profile role.
pub struct ClojureEdnCodec;

impl Decoder for ClojureEdnCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        decode_clojure_edn_tree(cx, "clojure-edn", input).map(|tree| tree.expr)
    }
}

impl LocatedDecoder for ClojureEdnCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        decode_clojure_edn_tree(cx, source_id, input).map(|tree| tree.located())
    }
}

impl TreeDecoder for ClojureEdnCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_clojure_edn_tree(cx, source_id, input)
    }
}

/// Loadable [`Lib`] that registers the [`ClojureEdnCodec`] as a runtime codec object.
///
/// Exports a single [`Export::Codec`] under [`clojure_edn_reader_symbol`] and
/// installs it via the [`Linker`] when loaded.
///
/// # Examples
///
/// Load the codec and decode a small EDN form into the shared `Expr` graph:
///
/// ```
/// use std::sync::Arc;
/// use sim_codec::{Input, decode_tree_with_codec};
/// use sim_kernel::{
///     CapabilitySet, Cx, DefaultFactory, Expr, NoopEvalPolicy, ReadPolicy, TrustLevel,
/// };
/// use sim_lib_lang_clojure::{ClojureEdnCodecLib, clojure_edn_reader_symbol};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let codec_id = cx.registry_mut().fresh_codec_id();
/// cx.load_lib(&ClojureEdnCodecLib::new(codec_id))?;
///
/// let policy = ReadPolicy {
///     trust: TrustLevel::TrustedSource,
///     capabilities: CapabilitySet::new(),
/// };
/// let tree = decode_tree_with_codec(
///     &mut cx,
///     &clojure_edn_reader_symbol(),
///     Input::Text("[1 2 3]".to_owned()),
///     policy,
///     "doc.edn",
/// )?;
/// assert!(matches!(tree.expr, Expr::Vector(_)));
/// # Ok::<(), sim_kernel::Error>(())
/// ```
pub struct ClojureEdnCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl ClojureEdnCodecLib {
    /// Builds the codec lib bound to the given runtime [`CodecId`].
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: clojure_edn_reader_symbol(),
            codec_id: id,
        }
    }
}

impl Lib for ClojureEdnCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: self.symbol.clone(),
                codec_id: Some(self.codec_id),
            }],
        }
    }

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let _factory = DefaultFactory;
        let expr_shape = sim_codec::resolve_expr_shape(linker, &Symbol::qualified("core", "Expr"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(ClojureEdnCodec)),
                located_decoder: Some(Arc::new(ClojureEdnCodec)),
                tree_decoder: Some(Arc::new(ClojureEdnCodec)),
                encoder: None,
                located_encoder: None,
                tree_encoder: None,
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::Datum,
            }),
        )?;
        Ok(())
    }
}
