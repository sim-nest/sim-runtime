use sim_kernel::{Cx, Ref, Result, Symbol};
use sim_lib_control::{ControlPrompt, ControlTag};

/// The control prompt emitted when the logic resolver encounters `!`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CutPrompt {
    /// Choice-frame boundary retained after the cut commits.
    pub(crate) cut_parent: u64,
}

impl CutPrompt {
    pub(crate) fn tag_symbol() -> Symbol {
        Symbol::qualified("logic", "cut")
    }
}

impl ControlPrompt for CutPrompt {
    fn tag(&self) -> ControlTag {
        ControlTag::new(Self::tag_symbol())
    }

    fn input(&self) -> Ref {
        Ref::Symbol(Symbol::qualified(
            "logic-cut",
            format!("parent-{}", self.cut_parent),
        ))
    }
}

pub(crate) fn raise_cut_prompt(cx: &mut Cx, prompt_record: CutPrompt) -> Result<()> {
    sim_lib_control::raise_prompt(cx, &prompt_record).map(|_| ())
}
