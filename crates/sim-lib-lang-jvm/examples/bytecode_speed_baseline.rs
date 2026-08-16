use sim_codec_classfile::{ByteReader, ConstantPool, Opcode, decode_instructions};
use sim_kernel::SourceId;
use sim_lib_lang_jvm::{
    JvmBenchmarkCounters, JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, prepare_code,
};

const NONE: &[JvmSlotKind] = &[];
const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];

struct CorpusPolicy;
impl JvmInstructionPolicy for CorpusPolicy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        match opcode {
            Opcode::Iconst0 => Some(JvmInstructionSemantics {
                pops: NONE,
                pushes: INT,
                safepoint: false,
            }),
            Opcode::Ireturn => Some(JvmInstructionSemantics {
                pops: INT,
                pushes: NONE,
                safepoint: true,
            }),
            _ => None,
        }
    }
}

fn main() {
    let phase = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "warm-execution".into());
    let iterations: u64 = std::env::var("SIM_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let pool = ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap();
    let bytes = [Opcode::Iconst0 as u8, Opcode::Ireturn as u8];
    let decoded = decode_instructions(&bytes, 61, &pool).unwrap();
    let mut counters = JvmBenchmarkCounters::default();

    for _ in 0..iterations {
        let prepared = prepare_code::<CorpusPolicy>(
            &decoded,
            bytes.len(),
            &[],
            SourceId("bench:Example.zero()I".into()),
        )
        .unwrap();
        counters.prepared(decoded.instructions.len() as u64);
        if phase == "cold-preparation" {
            continue;
        }

        counters.resolved();
        counters.allocated();
        let mut cursor = prepared.entry();
        loop {
            let instruction = prepared.instruction(cursor);
            counters.dispatched(1);
            if instruction.is_safepoint() {
                counters.polled_safepoint();
                counters.scanned_roots();
            }
            let Some(next) = prepared.next(cursor) else {
                break;
            };
            cursor = next;
        }
    }
    let fields = counters
        .as_map()
        .into_iter()
        .map(|(name, value)| format!("\"{name}\":{value}"))
        .collect::<Vec<_>>()
        .join(",");
    println!("{{{fields}}}");
}
