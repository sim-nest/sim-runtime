//! Epoch admission, subject selection, and report rendering.

use super::*;

/// epoch, and its required floor is sealed by the decision specification.
pub fn negative_projection_allowed(spec: &DecisionSpec, epochs: &BTreeSet<ContentId>) -> bool {
    epochs.len() >= usize::max(2, spec.negative_epoch_floor as usize)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StalenessInputs {
    pub subject_snapshot: ContentId,
    pub evidence_root: ContentId,
    pub policy: ContentId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub id: ContentId,
    pub subjects: Vec<ContentId>,
    pub decisive_evidence: BTreeSet<ContentId>,
    pub report_root: ContentId,
    pub expiry: ContentId,
    pub staleness: StalenessInputs,
}

pub fn select(
    graph: &ReportGraph,
    evidence: &[SubjectEvidence],
    staleness: StalenessInputs,
) -> Result<Selection, DecisionError> {
    let by_subject = evidence
        .iter()
        .map(|row| (row.subject.clone(), row))
        .collect::<BTreeMap<_, _>>();
    let subjects = graph
        .subjects
        .iter()
        .filter(|row| matches!(row.verdict, Verdict::Preferred | Verdict::Incomparable))
        .map(|row| row.subject.clone())
        .collect::<Vec<_>>();
    if subjects.is_empty() {
        return Err(DecisionError::UnresolvedSelection);
    }
    let mut decisive_evidence = BTreeSet::new();
    for subject in &subjects {
        let row = by_subject
            .get(subject)
            .ok_or(DecisionError::UnresolvedSelection)?;
        if row.bootstrap
            || row.private_unapproved
            || row.report_only
            || row.unresolved
            || row.quarantined
        {
            return Err(DecisionError::InadmissibleSelection);
        }
        decisive_evidence.extend(row.evidence.iter().cloned());
    }
    let mut selection = Selection {
        id: graph.id.clone(),
        subjects,
        decisive_evidence,
        report_root: graph.id.clone(),
        expiry: graph.spec.expiry.clone(),
        staleness,
    };
    selection.id = selection
        .to_datum()
        .content_id()
        .map_err(|_| DecisionError::Noncanonical)?;
    Ok(selection)
}

impl ReportGraph {
    pub fn to_datum(&self) -> Datum {
        Datum::Node {
            tag: sym("decision-report"),
            fields: vec![
                (sym("spec"), cid(&self.spec.id)),
                (sym("evidence-root"), cid(&self.evidence_root)),
                (
                    sym("subjects"),
                    Datum::Vector(
                        self.subjects
                            .iter()
                            .map(|row| Datum::Node {
                                tag: sym("subject"),
                                fields: vec![
                                    (sym("id"), cid(&row.subject)),
                                    (
                                        sym("verdict"),
                                        Datum::Symbol(sym(verdict_name(row.verdict))),
                                    ),
                                    (
                                        sym("evidence"),
                                        Datum::Vector(
                                            row.decisive_evidence.iter().map(cid).collect(),
                                        ),
                                    ),
                                    (
                                        sym("inferences"),
                                        Datum::Vector(
                                            row.decisive_inferences.iter().map(cid).collect(),
                                        ),
                                    ),
                                    (
                                        sym("attributions"),
                                        Datum::Vector(
                                            row.decisive_attributions.iter().map(cid).collect(),
                                        ),
                                    ),
                                    (
                                        sym("reasons"),
                                        Datum::Vector(
                                            row.reasons
                                                .iter()
                                                .cloned()
                                                .map(Datum::Symbol)
                                                .collect(),
                                        ),
                                    ),
                                ],
                            })
                            .collect(),
                    ),
                ),
                (
                    sym("edges"),
                    Datum::Vector(
                        self.edges
                            .iter()
                            .map(|edge| Datum::Node {
                                tag: sym("edge"),
                                fields: vec![
                                    (sym("left"), cid(&edge.left)),
                                    (sym("right"), cid(&edge.right)),
                                    (
                                        sym("relation"),
                                        Datum::Symbol(sym(relation_name(&edge.relation))),
                                    ),
                                    (
                                        sym("dimensions"),
                                        Datum::Vector(
                                            edge.dimensions
                                                .iter()
                                                .cloned()
                                                .map(Datum::Symbol)
                                                .collect(),
                                        ),
                                    ),
                                ],
                            })
                            .collect(),
                    ),
                ),
                (
                    sym("equivalence-classes"),
                    Datum::Vector(
                        self.equivalence_classes
                            .iter()
                            .map(|class| Datum::Vector(class.iter().map(cid).collect()))
                            .collect(),
                    ),
                ),
                (
                    sym("tiers"),
                    Datum::Vector(
                        self.tiers
                            .iter()
                            .map(|tier| Datum::Vector(tier.iter().map(cid).collect()))
                            .collect(),
                    ),
                ),
            ],
        }
    }

    pub fn render_sim(&self) -> String {
        render_datum(&self.to_datum())
    }
    pub fn render_markdown(&self) -> String {
        let mut out = String::from(
            "| subject | verdict | evidence | inference | attribution |\n|---|---|---|---|---|\n",
        );
        for row in &self.subjects {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                short(&row.subject),
                verdict_name(row.verdict),
                row.decisive_evidence
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_inferences
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_attributions
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";")
            ));
        }
        out
    }
    pub fn render_csv(&self) -> String {
        let mut out = String::from("subject,verdict,evidence,inference,attribution\n");
        for row in &self.subjects {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                short(&row.subject),
                verdict_name(row.verdict),
                row.decisive_evidence
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_inferences
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";"),
                row.decisive_attributions
                    .iter()
                    .map(short)
                    .collect::<Vec<_>>()
                    .join(";")
            ));
        }
        out
    }
    pub fn render_table(&self) -> Vec<Vec<String>> {
        self.subjects
            .iter()
            .map(|row| {
                vec![
                    short(&row.subject),
                    verdict_name(row.verdict).into(),
                    row.decisive_evidence
                        .iter()
                        .map(short)
                        .collect::<Vec<_>>()
                        .join(";"),
                    row.decisive_inferences
                        .iter()
                        .map(short)
                        .collect::<Vec<_>>()
                        .join(";"),
                    row.decisive_attributions
                        .iter()
                        .map(short)
                        .collect::<Vec<_>>()
                        .join(";"),
                ]
            })
            .collect()
    }
}

impl Selection {
    fn to_datum(&self) -> Datum {
        Datum::Node {
            tag: sym("selection"),
            fields: vec![
                (
                    sym("subjects"),
                    Datum::Vector(self.subjects.iter().map(cid).collect()),
                ),
                (
                    sym("evidence"),
                    Datum::Vector(self.decisive_evidence.iter().map(cid).collect()),
                ),
                (sym("report-root"), cid(&self.report_root)),
                (sym("expiry"), cid(&self.expiry)),
                (
                    sym("subject-snapshot"),
                    cid(&self.staleness.subject_snapshot),
                ),
                (sym("evidence-root"), cid(&self.staleness.evidence_root)),
                (sym("policy"), cid(&self.staleness.policy)),
            ],
        }
    }
}

fn verdict_name(value: Verdict) -> &'static str {
    match value {
        Verdict::InsufficientEvidence => "insufficient-evidence",
        Verdict::RejectedGate => "rejected-gate",
        Verdict::OverBudget => "over-budget",
        Verdict::Dominated => "dominated",
        Verdict::Eligible => "eligible",
        Verdict::Preferred => "preferred",
        Verdict::Incomparable => "incomparable",
    }
}
fn relation_name(value: &Relation) -> &'static str {
    match value {
        Relation::Dominates => "dominates",
        Relation::Equivalent => "equivalent",
        Relation::Incomparable => "incomparable",
    }
}
pub(super) fn sym(name: &str) -> Symbol {
    Symbol::qualified("study", name)
}
pub(super) fn cid(id: &ContentId) -> Datum {
    Datum::Node {
        tag: sym("content-id"),
        fields: vec![
            (sym("algorithm"), Datum::Symbol(id.algorithm.clone())),
            (sym("digest"), Datum::Bytes(id.bytes.to_vec())),
        ],
    }
}
fn short(id: &ContentId) -> String {
    id.bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn render_datum(datum: &Datum) -> String {
    format!("{datum:?}")
}
