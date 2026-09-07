use std::sync::atomic::{AtomicUsize, Ordering};

use sim_kernel::{CodecId, Datum, NumberLiteral, Origin, SourceId, Span, Symbol};
use sim_lib_machine::{
    AdmissionLimits, AdmissionPolicy, EffectPolicy, InstructionPolicy, LocatedCode,
    LocatedInstruction, MachineDescription, MachinePermit, SourceLocation,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Instruction(u8);

struct Instructions;

impl InstructionPolicy for Instructions {
    type Instruction = Instruction;
    type InstructionId = u8;

    fn instruction_id(instruction: &Instruction) -> u8 {
        instruction.0
    }
}

static EFFECT_CALLBACKS: AtomicUsize = AtomicUsize::new(0);

struct Effects;

impl EffectPolicy<Instruction> for Effects {
    type Effect = ();

    fn classify(_instruction: &Instruction) {
        EFFECT_CALLBACKS.fetch_add(1, Ordering::SeqCst);
    }
}

struct Policy;

impl AdmissionPolicy<Instructions, u8> for Policy {
    type Refusal = &'static str;

    fn validate_description(
        description: &MachineDescription<'_, Instructions, u8>,
    ) -> Result<(), Self::Refusal> {
        (description
            .code()
            .instruction(description.code().entry())
            .instruction()
            .0
            == *description.metadata())
        .then_some(())
        .ok_or("entry shape")
    }

    fn validate_instruction(
        instruction: &Instruction,
        _metadata: &u8,
    ) -> Result<(), Self::Refusal> {
        (instruction.0 != 0)
            .then_some(())
            .ok_or("unknown instruction")
    }

    fn policy_identity() -> Symbol {
        Symbol::qualified("machine-test", "admission")
    }
    fn policy_version() -> u32 {
        1
    }
    fn metadata_datum(metadata: &u8) -> Datum {
        number(*metadata)
    }
    fn instruction_datum(instruction: &Instruction) -> Datum {
        number(instruction.0)
    }
}

struct PolicyV2;

impl AdmissionPolicy<Instructions, u8> for PolicyV2 {
    type Refusal = &'static str;
    fn validate_description(
        _: &MachineDescription<'_, Instructions, u8>,
    ) -> Result<(), Self::Refusal> {
        Ok(())
    }
    fn validate_instruction(_: &Instruction, _: &u8) -> Result<(), Self::Refusal> {
        Ok(())
    }
    fn policy_identity() -> Symbol {
        <Policy as AdmissionPolicy<Instructions, u8>>::policy_identity()
    }
    fn policy_version() -> u32 {
        2
    }
    fn metadata_datum(metadata: &u8) -> Datum {
        number(*metadata)
    }
    fn instruction_datum(instruction: &Instruction) -> Datum {
        number(instruction.0)
    }
}

fn number(value: u8) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u8"),
        canonical: value.to_string(),
    })
}

#[test]
fn admission_validates_every_instruction_without_classifying_an_effect() {
    EFFECT_CALLBACKS.store(0, Ordering::SeqCst);
    let _effects = Effects;
    let code = code(&[1, 2, 3]);
    let description = MachineDescription::new(&code, limits(), &1);

    let permit = MachinePermit::admit::<_, _, Policy>(&description).unwrap();

    assert!(permit.accepts::<_, _, Policy>(&description));
    assert_eq!(EFFECT_CALLBACKS.load(Ordering::SeqCst), 0);
}

#[test]
fn editing_an_instruction_changes_identity_and_refuses_the_old_permit() {
    let original = code(&[1, 2, 3]);
    let original_description = MachineDescription::new(&original, limits(), &1);
    let permit = MachinePermit::admit::<_, _, Policy>(&original_description).unwrap();

    let edited = code(&[1, 2, 4]);
    let edited_description = MachineDescription::new(&edited, limits(), &1);
    let edited_permit = MachinePermit::admit::<_, _, Policy>(&edited_description).unwrap();

    assert_ne!(permit.content_id(), edited_permit.content_id());
    assert!(!permit.accepts::<_, _, Policy>(&edited_description));
}

#[test]
fn policy_location_and_limits_are_all_bound_by_the_permit() {
    let original = code_in_source(&[1, 2], "source-a", CodecId(1));
    let description = MachineDescription::new(&original, limits(), &1);
    let permit = MachinePermit::admit::<_, _, Policy>(&description).unwrap();
    assert_eq!(
        permit.content_id().algorithm,
        sim_kernel::datum_content_algorithm()
    );
    assert!(!permit.accepts::<_, _, PolicyV2>(&description));

    let relocated = code_in_source(&[1, 2], "source-b", CodecId(1));
    assert!(!permit.accepts::<_, _, Policy>(&MachineDescription::new(&relocated, limits(), &1,)));
    let recoded = code_in_source(&[1, 2], "source-a", CodecId(2));
    assert!(!permit.accepts::<_, _, Policy>(&MachineDescription::new(&recoded, limits(), &1,)));
    let mut tighter = limits();
    tighter.work -= 1;
    assert!(!permit.accepts::<_, _, Policy>(&MachineDescription::new(&original, tighter, &1,)));
}

fn code(values: &[u8]) -> LocatedCode<Instructions> {
    code_in_source(values, "admission-test", CodecId(1))
}

fn code_in_source(values: &[u8], source: &str, codec: CodecId) -> LocatedCode<Instructions> {
    LocatedCode::freeze(
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                LocatedInstruction::new(
                    Instruction(*value),
                    *value,
                    SourceLocation::Bytes(Origin {
                        codec,
                        source: SourceId(source.into()),
                        span: Span {
                            start: index,
                            end: index + 1,
                        },
                        trivia: vec![],
                    }),
                    false,
                    None,
                )
            })
            .collect(),
        vec![],
        vec![],
    )
    .unwrap()
}

fn limits() -> AdmissionLimits {
    AdmissionLimits {
        instructions: 8,
        operand_units: 16,
        slots: 4,
        frames: 2,
        work: 32,
    }
}
