use sim_kernel::{Cx, Result, Symbol, Value};

/// Language-neutral slot lookup for values with metadata-driven behavior.
///
/// Implementations provide raw indexed access plus a named meta-slot lookup.
/// The slot names are supplied by language crates or other callers, so this
/// protocol can model metatables, prototype parents, method dictionaries, and
/// similar object layers without baking any one language's names into dispatch.
pub trait MetaObjectProtocol: Send + Sync {
    /// Returns a raw indexed value before consulting any metaobject fallback.
    fn raw_get(&self, cx: &mut Cx, value: &Value, key: &Value) -> Result<Option<Value>>;

    /// Returns the meta value installed under `slot` for `value`, if present.
    fn get_meta(&self, cx: &mut Cx, value: &Value, slot: &Symbol) -> Result<Option<Value>>;

    /// Applies a meta value to the original indexed read.
    ///
    /// The default treats the meta value as another indexable object and looks
    /// up `key` on it. Prototype languages can override this to recurse through
    /// parent objects, while function-backed systems can override it to invoke
    /// or otherwise interpret a callable meta value.
    fn apply_meta(
        &self,
        cx: &mut Cx,
        _receiver: &Value,
        key: &Value,
        _index_slot: &Symbol,
        meta_value: &Value,
    ) -> Result<Option<Value>> {
        self.raw_get(cx, meta_value, key)
    }
}

/// Performs an indexed read with a caller-selected metaobject fallback slot.
///
/// Raw values win. If raw access misses, the protocol looks up `index_slot` on
/// `receiver` and applies that meta value through
/// [`MetaObjectProtocol::apply_meta`].
pub fn meta_index(
    cx: &mut Cx,
    proto: &dyn MetaObjectProtocol,
    receiver: &Value,
    key: &Value,
    index_slot: &Symbol,
) -> Result<Option<Value>> {
    if let Some(value) = proto.raw_get(cx, receiver, key)? {
        return Ok(Some(value));
    }
    let Some(meta_value) = proto.get_meta(cx, receiver, index_slot)? else {
        return Ok(None);
    };
    proto.apply_meta(cx, receiver, key, index_slot, &meta_value)
}
