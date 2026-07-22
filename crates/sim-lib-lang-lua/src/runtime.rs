use sim_kernel::Ref;
use sim_lib_control::Coroutine;

/// Builds a Lua coroutine that alternates between two ref lanes.
///
/// Lowers Lua coroutines onto the control organ's [`Coroutine`] rather than
/// defining bespoke coroutine semantics.
pub fn lua_coroutine(first: Vec<Ref>, second: Vec<Ref>) -> Coroutine {
    Coroutine::alternating(first, second)
}
