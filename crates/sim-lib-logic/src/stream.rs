use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use sim_kernel::{
    CORE_LIST_CLASS_ID, ClassRef, Cx, Error, Expr, Object, Result, ShapeMatch, Stream, Symbol,
    Value,
};

use crate::query::SequenceEngine;

#[derive(Clone, Debug)]
pub struct LogicAnswer {
    pub matched: ShapeMatch,
}

#[sim_citizen_derive::non_citizen(
    reason = "live logic answer stream; reconstruct from query and logic/Db descriptor data",
    kind = "handle",
    descriptor = "logic/Db"
)]
#[derive(Clone, Debug)]
pub struct LogicStream {
    state: Arc<Mutex<LogicStreamState>>,
}

#[derive(Debug)]
struct LogicStreamState {
    buffered: VecDeque<LogicAnswer>,
    remaining: VecDeque<LogicAnswer>,
    engine: Option<SequenceEngine>,
    buffer_limit: Option<usize>,
    closed: bool,
}

impl LogicStream {
    pub fn new(answers: Vec<ShapeMatch>, stream_buffer: usize) -> Self {
        let buffer_limit = (stream_buffer > 0).then_some(stream_buffer);
        let mut remaining = answers
            .into_iter()
            .map(|matched| LogicAnswer { matched })
            .collect::<VecDeque<_>>();
        let mut buffered = VecDeque::new();
        refill(&mut buffered, &mut remaining, buffer_limit);
        Self {
            state: Arc::new(Mutex::new(LogicStreamState {
                buffered,
                remaining,
                engine: None,
                buffer_limit,
                closed: false,
            })),
        }
    }

    pub(crate) fn from_engine(engine: SequenceEngine, stream_buffer: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(LogicStreamState {
                buffered: VecDeque::new(),
                remaining: VecDeque::new(),
                engine: Some(engine),
                buffer_limit: (stream_buffer > 0).then_some(stream_buffer),
                closed: false,
            })),
        }
    }

    pub fn collect(&self, cx: &mut Cx, limit: Option<usize>) -> Result<Vec<Value>> {
        let mut values = Vec::new();
        while limit.is_none_or(|bound| values.len() < bound) {
            let Some(value) = Stream::next(self, cx)? else {
                break;
            };
            values.push(value);
        }
        Ok(values)
    }
}

impl Object for LogicStream {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<logic-stream>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for LogicStream {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(class) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "List"))
        {
            return Ok(class.clone());
        }
        cx.factory().class_stub(
            CORE_LIST_CLASS_ID,
            Symbol::qualified("logic", "AnswerStream"),
        )
    }
    fn as_stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        self.as_table(cx)?.object().as_expr(cx)
    }
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("logic stream"))?;
        cx.factory().table(vec![
            (
                Symbol::new("kind"),
                cx.factory().symbol(Symbol::new("logic-stream"))?,
            ),
            (
                Symbol::new("buffered"),
                cx.factory().string(state.buffered.len().to_string())?,
            ),
            (
                Symbol::new("remaining"),
                cx.factory().string(stream_remaining(&state))?,
            ),
            (Symbol::new("closed"), cx.factory().bool(state.closed)?),
        ])
    }
}

impl Stream for LogicStream {
    fn next(&self, cx: &mut Cx) -> Result<Option<Value>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("logic stream"))?;
        if state.closed {
            return Ok(None);
        }
        if state.buffered.is_empty() {
            refill_state(&mut state, cx)?;
        }
        if let Some(answer) = state.buffered.pop_front() {
            return sim_kernel::shape_match_value(cx, answer.matched).map(Some);
        }
        if let Some(engine) = &state.engine
            && let Some(value) = engine.next_value(cx)?
        {
            return Ok(Some(value));
        }
        if let Some(answer) = state.remaining.pop_front() {
            return sim_kernel::shape_match_value(cx, answer.matched).map(Some);
        }
        state.closed = true;
        Ok(None)
    }

    fn close(&self, cx: &mut Cx) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("logic stream"))?;
        state.closed = true;
        state.buffered.clear();
        state.remaining.clear();
        if let Some(engine) = &state.engine {
            engine.close(cx)?;
        }
        Ok(())
    }
}

fn refill(
    buffered: &mut VecDeque<LogicAnswer>,
    remaining: &mut VecDeque<LogicAnswer>,
    limit: Option<usize>,
) {
    match limit {
        Some(limit) => {
            while buffered.len() < limit {
                let Some(answer) = remaining.pop_front() else {
                    break;
                };
                buffered.push_back(answer);
            }
        }
        None => buffered.extend(remaining.drain(..)),
    }
}

fn refill_state(state: &mut LogicStreamState, cx: &mut Cx) -> Result<()> {
    match state.buffer_limit {
        Some(limit) => {
            while state.buffered.len() < limit {
                let Some(answer) = next_answer(state, cx)? else {
                    break;
                };
                state.buffered.push_back(answer);
            }
        }
        None if state.engine.is_some() => {
            if let Some(answer) = next_answer(state, cx)? {
                state.buffered.push_back(answer);
            }
        }
        None => {
            let drained = state.remaining.drain(..).collect::<Vec<_>>();
            state.buffered.extend(drained);
        }
    }
    Ok(())
}

fn next_answer(state: &mut LogicStreamState, cx: &mut Cx) -> Result<Option<LogicAnswer>> {
    if let Some(answer) = state.remaining.pop_front() {
        return Ok(Some(answer));
    }
    let Some(engine) = &state.engine else {
        return Ok(None);
    };
    Ok(engine
        .next_match(cx)?
        .map(|matched| LogicAnswer { matched }))
}

fn stream_remaining(state: &LogicStreamState) -> String {
    if state.engine.is_some() {
        "lazy".to_owned()
    } else {
        state.remaining.len().to_string()
    }
}
