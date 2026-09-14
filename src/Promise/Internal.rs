use std::rc::Rc;
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};

type PromiseValue = crate::UnknownType;
type Outcome = Result<PromiseValue, PromiseValue>;
type Reaction = Box<dyn FnOnce(Outcome) + 'static>;
struct PromiseState {
    outcome: Option<Outcome>,
    reactions: Vec<Reaction>,
    handled: bool,
}
pub struct Promise {
    queue: Rc<purust_core::microtasks::Queue>,
    state: Mutex<PromiseState>,
    resolved: AtomicBool,
}
fn promise_box(promise: Rc<Promise>) -> PromiseValue {
    purust_core::Value::Class(Rc::new(promise))
}
fn promise_cast(value: &PromiseValue) -> Option<Rc<Promise>> {
    match value.resolve() {
        purust_core::Value::Class(value) => value.downcast_ref::<Rc<Promise>>().cloned(),
        _ => None,
    }
}
fn promise_unbox(value: &PromiseValue) -> Rc<Promise> {
    promise_cast(value).expect("Expected a native Promise")
}
fn promise_try(action: impl FnOnce() -> PromiseValue) -> Outcome {
    Purs_Effect_Exception::purust_exception_try(action)
}
impl Promise {
    fn pending(queue: Rc<purust_core::microtasks::Queue>) -> Rc<Self> {
        Rc::new(Self { queue, resolved: AtomicBool::new(false),
            state: Mutex::new(PromiseState { outcome: None, reactions: Vec::new(), handled: false }) })
    }
    fn subscribe(self: &Rc<Self>, reaction: impl FnOnce(Outcome) + 'static) {
        let mut state = self.state.lock().unwrap();
        state.handled = true;
        match state.outcome.clone() {
            Some(outcome) => self.queue.enqueue(move || reaction(outcome)),
            None => state.reactions.push(Box::new(reaction)),
        }
    }
    fn settle(self: &Rc<Self>, outcome: Outcome) {
        let mut state = self.state.lock().unwrap();
        if state.outcome.is_some() { return; }
        state.outcome = Some(outcome.clone());
        // Queue while holding the state lock to preserve registration order
        // against a concurrent subscriber. No callback runs under this lock.
        for reaction in std::mem::take(&mut state.reactions) {
            let outcome = outcome.clone();
            self.queue.enqueue(move || reaction(outcome));
        }
        if let Err(error) = outcome {
            let promise = self.clone();
            self.queue.after_checkpoint(move || {
                if promise.state.lock().unwrap().handled { return; }
                // Catching the rejection still sees its original payload. Only
                // the uncaught process diagnostic needs an Error carrier.
                let error = match error.resolve() {
                    purust_core::Value::Class(value) if value.is::<std::sync::Arc<Purs_Effect_Exception::PurustExceptionError>>() => error,
                    purust_core::Value::String(text) => Purs_Effect_Exception::Effect_Exception_error(format!("Unhandled Promise rejection: {}", text)),
                    _ => Purs_Effect_Exception::Effect_Exception_error("Unhandled Promise rejection (non-Error value)".to_owned()),
                };
                Purs_Effect_Exception::purust_exception_raise(error);
            });
        }
    }
    fn resolve_once(self: &Rc<Self>, outcome: Outcome) {
        if self.resolved.swap(true, Ordering::AcqRel) { return; }
        match outcome { Ok(value) => self.adopt(value), Err(error) => self.settle(Err(error)) }
    }
    fn adopt(self: &Rc<Self>, value: PromiseValue) {
        match promise_cast(&value) {
            Some(source) if Rc::ptr_eq(self, &source) => self.settle(Err(
                Purs_Effect_Exception::Effect_Exception_errorWithName("A Promise cannot resolve to itself".to_owned(), "TypeError".to_owned()))),
            Some(source) => {
                let target = self.clone();
                // Adopting a Promise is itself a job, as in PromiseResolveThenableJob.
                self.queue.enqueue(move || source.subscribe(move |outcome| target.settle(outcome)));
            }
            None => self.settle(Ok(value)),
        }
    }
    fn chain(self: &Rc<Self>, success: Option<PromiseValue>, failure: Option<PromiseValue>) -> Rc<Self> {
        let result = Self::pending(self.queue.clone());
        let target = result.clone();
        self.subscribe(move |outcome| {
            let handler = if outcome.is_ok() { success } else { failure };
            match handler {
                Some(handler) => target.resolve_once(promise_try(|| handler.unwrap_func1()(match outcome { Ok(v) | Err(v) => v }))),
                None => target.resolve_once(outcome),
            }
        });
        result
    }
}

pub fn Promise_Internal_resolve(value: PromiseValue) -> Rc<Promise> {
    if let Some(promise) = promise_cast(&value) { return promise; }
    let promise = Promise::pending(purust_core::microtasks::current());
    promise.resolve_once(Ok(value));
    promise
}
pub fn Promise_Internal_reject(error: PromiseValue) -> Rc<Promise> {
    let promise = Promise::pending(purust_core::microtasks::current());
    promise.resolve_once(Err(error));
    promise
}
pub fn Promise_Internal_new() -> PromiseValue {
    purust_core::Value::Func1(purust_core::Func1::Static(|executor| {
        let promise = Promise::pending(purust_core::microtasks::current());
        let make_resolver = |reject| {
            let target = promise.clone();
            purust_core::Value::Func1(purust_core::Func1::Shared(Rc::new(move |value| {
                target.queue.turn(|| target.resolve_once(if reject { Err(value) } else { Ok(value) }));
                purust_core::Value::Unit
            })))
        };
        if let Err(error) = promise_try(|| executor.unwrap_func2()(make_resolver(false), make_resolver(true))) {
            promise.resolve_once(Err(error));
        }
        promise_box(promise)
    }))
}
pub fn Promise_Internal_then_() -> PromiseValue {
    purust_core::Value::Func2(purust_core::Func2::Static(|handler, promise| {
        promise_box(promise_unbox(&promise).chain(Some(handler), None))
    }))
}
pub fn Promise_Internal_thenOrCatch() -> PromiseValue {
    purust_core::Value::Func3(purust_core::Func3::Static(|success, failure, promise| {
        promise_box(promise_unbox(&promise).chain(Some(success), Some(failure)))
    }))
}
pub fn Promise_Internal_catch() -> PromiseValue {
    purust_core::Value::Func2(purust_core::Func2::Static(|handler, promise| {
        promise_box(promise_unbox(&promise).chain(None, Some(handler)))
    }))
}
pub fn Promise_Internal_finally() -> PromiseValue {
    purust_core::Value::Func2(purust_core::Func2::Static(|effect, promise| {
        let source = promise_unbox(&promise);
        let result = Promise::pending(source.queue.clone());
        let target = result.clone();
        source.subscribe(move |original| {
            match promise_try(|| effect.unwrap_func1()(purust_core::Value::Unit)) {
                Err(error) => target.resolve_once(Err(error)),
                Ok(value) => {
                    // finally returns the cleanup.then(valueThunk) Promise to
                    // the outer reaction; its adoption adds observable jobs.
                    let restored = Promise::pending(target.queue.clone());
                    let continuation = restored.clone();
                    Promise_Internal_resolve(value).subscribe(move |cleanup| {
                        continuation.resolve_once(match cleanup { Ok(_) => original, Err(error) => Err(error) });
                    });
                    target.resolve_once(Ok(promise_box(restored)));
                }
            }
        });
        promise_box(result)
    }))
}
pub fn Promise_Internal_all() -> PromiseValue {
    purust_core::Value::Func1(purust_core::Func1::Static(|array| {
        let values = array.unwrap_array();
        let result = Promise::pending(purust_core::microtasks::current());
        if values.is_empty() { result.resolve_once(Ok(purust_core::Value::Array(Rc::new(Vec::new())))); }
        let pending = Rc::new(Mutex::new((vec![None; values.len()], values.len())));
        for (index, value) in values.iter().enumerate() {
            let target = result.clone();
            let pending = pending.clone();
            Promise_Internal_resolve(value.clone()).subscribe(move |outcome| match outcome {
                Err(error) => target.resolve_once(Err(error)),
                Ok(value) => {
                    let mut pending = pending.lock().unwrap();
                    pending.0[index] = Some(value);
                    pending.1 -= 1;
                    if pending.1 == 0 {
                        target.resolve_once(Ok(purust_core::Value::Array(Rc::new(pending.0.iter().map(|v| v.clone().unwrap()).collect()))));
                    }
                }
            });
        }
        promise_box(result)
    }))
}
pub fn Promise_Internal_race() -> PromiseValue {
    purust_core::Value::Func1(purust_core::Func1::Static(|array| {
        let result = Promise::pending(purust_core::microtasks::current());
        for value in array.unwrap_array().iter() {
            let target = result.clone();
            Promise_Internal_resolve(value.clone()).subscribe(move |outcome| target.resolve_once(outcome));
        }
        promise_box(result)
    }))
}
