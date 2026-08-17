use std::sync::atomic::{AtomicUsize, Ordering};

use sim_kernel::{CodecId, Origin, SourceId, Span};
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

    fn encode_metadata(metadata: &u8, output: &mut Vec<u8>) {
        output.push(*metadata);
    }

    fn encode_instruction(instruction: &Instruction, output: &mut Vec<u8>) {
        output.push(instruction.0);
    }
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

fn code(values: &[u8]) -> LocatedCode<Instructions> {
    LocatedCode::freeze(
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                LocatedInstruction::new(
                    Instruction(*value),
                    *value,
                    SourceLocation::Bytes(Origin {
                        codec: CodecId(1),
                        source: SourceId("admission-test".into()),
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
