use std::sync::Arc;

use sim_codec::{
    CodecDefaultDecode, CodecRuntime, Decoder, Input, LocatedDecoder, ReadCx, TreeDecoder,
    codec_value,
};
use sim_kernel::{
    AbiVersion, CodecId, DefaultFactory, Dependency, Export, Lib, LibManifest, LibTarget, Linker,
    LocatedExpr, LocatedExprTree, Result, Symbol, Version,
};

use crate::{decode_scheme_tree, scheme_reader_symbol};

/// Decoder turning Scheme surface text into the shared `Expr` graph.
///
/// Implements the kernel [`Decoder`], [`LocatedDecoder`], and [`TreeDecoder`]
/// contracts; this profile supplies no encoder.
pub struct SchemeCodec;

impl Decoder for SchemeCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<sim_kernel::Expr> {
        decode_scheme_tree(cx, "scheme", input).map(|tree| tree.expr)
    }
}

impl LocatedDecoder for SchemeCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExpr> {
        decode_scheme_tree(cx, source_id, input).map(|tree| tree.located())
    }
}

impl TreeDecoder for SchemeCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        decode_scheme_tree(cx, source_id, input)
    }
}

/// Loadable [`Lib`] that registers [`SchemeCodec`] as a runtime codec.
pub struct SchemeCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}

impl SchemeCodecLib {
    /// Builds the codec lib bound to the given runtime [`CodecId`].
    pub fn new(id: CodecId) -> Self {
        Self {
            symbol: scheme_reader_symbol(),
            codec_id: id,
        }
    }
}

impl Lib for SchemeCodecLib {
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
                decoder: Some(Arc::new(SchemeCodec)),
                located_decoder: Some(Arc::new(SchemeCodec)),
                tree_decoder: Some(Arc::new(SchemeCodec)),
                encoder: None,
                located_encoder: None,
                tree_encoder: None,
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::TermInEvalDatumOtherwise,
            }),
        )?;
        Ok(())
    }
}
