use sim_kernel::Symbol;

/// Shared organ a CL-lite surface form delegates to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClLiteFormRole {
    /// Form backed by the binding organ (e.g. `defun`, `let`).
    Binding,
    /// Form backed by the control organ (e.g. `handler-case`).
    Control,
    /// Form backed by the dispatch organ (e.g. `defgeneric`).
    Dispatch,
    /// Form backed by the namespace organ (e.g. `defpackage`).
    Namespace,
    /// Form backed by the mutation organ (e.g. `setq`, `setf`).
    Mutation,
}

/// Specification of one CL-lite surface form and the organ it routes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClLiteFormSpec {
    /// `cl`-qualified surface symbol of the form.
    pub symbol: Symbol,
    /// Organ role classifying the form.
    pub role: ClLiteFormRole,
    /// Symbol of the organ that provides the form's behavior.
    pub organ: Symbol,
    /// One-line description of the form.
    pub doc: &'static str,
}

/// Returns the CL-lite surface forms and the organs they delegate to.
pub fn cl_lite_form_specs() -> Vec<ClLiteFormSpec> {
    vec![
        binding_form(
            "defun",
            "Define a function through the shared binding organ.",
        ),
        binding_form(
            "defmacro",
            "Define a macro function through the binding organ.",
        ),
        binding_form(
            "let",
            "Enter a lexical binding frame through the binding organ.",
        ),
        mutation_form("setq", "Update a CL-lite variable through a mutation cell."),
        control_form(
            "handler-case",
            "Signal to the nearest condition handler through the control organ.",
        ),
        control_form(
            "restart-case",
            "Expose and invoke restarts through the control organ.",
        ),
        dispatch_form(
            "defgeneric",
            "Declare a generic function backed by the dispatch organ.",
        ),
        dispatch_form(
            "defmethod",
            "Attach a primary method through the dispatch organ.",
        ),
        namespace_form(
            "defpackage",
            "Create a package through the namespace organ.",
        ),
        namespace_form(
            "in-package",
            "Resolve the current package through the namespace organ.",
        ),
        mutation_form(
            "setf",
            "Update generalized places through the mutation organ.",
        ),
    ]
}

fn binding_form(name: &'static str, doc: &'static str) -> ClLiteFormSpec {
    ClLiteFormSpec {
        symbol: Symbol::qualified("cl", name),
        role: ClLiteFormRole::Binding,
        organ: sim_lib_binding::binding_organ_symbol(),
        doc,
    }
}

fn control_form(name: &'static str, doc: &'static str) -> ClLiteFormSpec {
    ClLiteFormSpec {
        symbol: Symbol::qualified("cl", name),
        role: ClLiteFormRole::Control,
        organ: sim_lib_control::control_organ_symbol(),
        doc,
    }
}

fn dispatch_form(name: &'static str, doc: &'static str) -> ClLiteFormSpec {
    ClLiteFormSpec {
        symbol: Symbol::qualified("cl", name),
        role: ClLiteFormRole::Dispatch,
        organ: sim_lib_dispatch::dispatch_organ_symbol(),
        doc,
    }
}

fn namespace_form(name: &'static str, doc: &'static str) -> ClLiteFormSpec {
    ClLiteFormSpec {
        symbol: Symbol::qualified("cl", name),
        role: ClLiteFormRole::Namespace,
        organ: sim_lib_namespace::namespace_organ_symbol(),
        doc,
    }
}

fn mutation_form(name: &'static str, doc: &'static str) -> ClLiteFormSpec {
    ClLiteFormSpec {
        symbol: Symbol::qualified("cl", name),
        role: ClLiteFormRole::Mutation,
        organ: sim_lib_mutation::mutation_organ_symbol(),
        doc,
    }
}
