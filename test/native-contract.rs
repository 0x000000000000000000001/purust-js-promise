use super::*;
use purust_core::Value;

fn value(n: i64) -> Value { Value::Int(n) }
fn boxed(n: i64) -> Value { promise_box(Promise_Internal_resolve(value(n))) }
fn function(f: impl Fn(Value) -> Value + 'static) -> Value {
    Value::Func1(purust_core::Func1::Shared(Rc::new(f)))
}
fn success(p: &Rc<Promise>) -> Value {
    match p.state.lock().unwrap().outcome.clone() { Some(Ok(v)) => v, _ => panic!("not fulfilled") }
}
fn failure(p: &Rc<Promise>) -> Value {
    let mut state = p.state.lock().unwrap();
    state.handled = true;
    match state.outcome.clone() { Some(Err(v)) => v, _ => panic!("not rejected") }
}
fn in_loop(test: impl FnOnce(Rc<purust_core::microtasks::Queue>)) {
    let queue = purust_core::microtasks::Queue::new(|| {});
    let _scope = purust_core::microtasks::Scope::enter(queue.clone());
    test(queue.clone());
    queue.drain();
    assert!(!queue.has_jobs());
}
#[test]
fn reaction_order_matches_the_original_javascript_ffi() {
    in_loop(|queue| {
        let trace = Rc::new(Mutex::new(Vec::new()));
        let record = |label| {
            let trace = trace.clone();
            function(move |_| { trace.lock().unwrap().push(label); promise_box(Promise_Internal_resolve(Value::Unit)) })
        };
        let p = Promise_Internal_resolve(value(1));
        p.chain(Some(record("then")), None).chain(Some(record("chain")), None);
        let finally = Promise_Internal_finally().unwrap_func2()(record("finally"), promise_box(p.clone()));
        promise_unbox(&finally).chain(Some(record("after-finally")), None);
        Promise_Internal_resolve(value(9)).chain(Some(record("marker")), None);
        let array = Value::Array(Rc::new(vec![promise_box(p.clone()), boxed(2)]));
        promise_unbox(&Promise_Internal_all().unwrap_func1()(array.clone())).chain(Some(record("all")), None);
        promise_unbox(&Promise_Internal_race().unwrap_func1()(array)).chain(Some(record("race")), None);
        let adopted = Promise::pending(queue.clone());
        adopted.resolve_once(Ok(promise_box(p)));
        adopted.chain(Some(record("adopted")), None);
        trace.lock().unwrap().push("sync");
        queue.drain();
        assert_eq!(*trace.lock().unwrap(), EXPECTED_TRACE);
    });
}
#[test]
fn executor_is_immediate_reactions_are_fifo_after_the_turn() {
    in_loop(|queue| {
        let log = Rc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        let p = queue.turn(|| {
            let p = promise_unbox(&Promise_Internal_new().unwrap_func1()(Value::Func2(purust_core::Func2::Shared(Rc::new(move |resolve, reject| {
                seen.lock().unwrap().push(1);
                resolve.unwrap_func1()(value(42));
                reject.unwrap_func1()(value(99));
                resolve.unwrap_func1()(value(100));
                seen.lock().unwrap().push(2);
                Value::Unit
            })))));
            for index in [3, 4] {
                let log = log.clone();
                p.chain(Some(function(move |v| { assert_eq!(v.unwrap_int(), 42); log.lock().unwrap().push(index); boxed(index) })), None);
            }
            queue.drain(); // A checkpoint inside a turn must not run reactions.
            assert_eq!(*log.lock().unwrap(), vec![1, 2]);
            p
        });
        queue.drain();
        assert_eq!(*log.lock().unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(success(&p).unwrap_int(), 42);
    });
}
#[test]
fn nested_jobs_follow_existing_jobs_and_do_not_reenter() {
    in_loop(|queue| {
        let log = Rc::new(Mutex::new(Vec::new()));
        let p = Promise_Internal_resolve(value(1));
        let seen = log.clone();
        let source = p.clone();
        let reenter = queue.clone();
        p.subscribe(move |_| {
            seen.lock().unwrap().push(1);
            let seen2 = seen.clone();
            source.subscribe(move |_| seen2.lock().unwrap().push(3));
            reenter.drain();
            assert_eq!(*seen.lock().unwrap(), vec![1]);
        });
        let seen = log.clone();
        p.subscribe(move |_| seen.lock().unwrap().push(2));
        queue.drain();
        assert_eq!(*log.lock().unwrap(), vec![1, 2, 3]);
    });
}
#[test]
fn adopting_a_pending_promise_locks_out_later_settlement() {
    in_loop(|queue| {
        let source = Promise::pending(queue.clone());
        let p = Promise::pending(queue.clone());
        p.resolve_once(Ok(promise_box(source.clone())));
        p.resolve_once(Err(value(99)));
        assert!(p.state.lock().unwrap().outcome.is_none());
        queue.drain();
        source.resolve_once(Ok(value(42)));
        queue.drain();
        assert_eq!(success(&p).unwrap_int(), 42);
        assert!(Rc::ptr_eq(&p, &Promise_Internal_resolve(promise_box(p.clone()))));
    });
}
#[test]
fn direct_and_handler_self_resolution_reject_as_type_error() {
    in_loop(|queue| {
        let p = Promise::pending(queue.clone());
        p.resolve_once(Ok(promise_box(p.clone())));
        assert_eq!(Purs_Effect_Exception::Effect_Exception_name(failure(&p)), "TypeError");
        let slot = Rc::new(Mutex::new(None::<Rc<Promise>>));
        let seen = slot.clone();
        let p = Promise_Internal_resolve(value(1)).chain(Some(function(move |_| promise_box(seen.lock().unwrap().clone().unwrap()))), None);
        *slot.lock().unwrap() = Some(p.clone());
        p.subscribe(|outcome| assert_eq!(Purs_Effect_Exception::Effect_Exception_name(outcome.err().unwrap()), "TypeError"));
        queue.drain();
        slot.lock().unwrap().take();
    });
}
#[test]
fn executor_and_handler_exceptions_preserve_native_error_identity() {
    in_loop(|queue| {
        let error = Purs_Effect_Exception::Effect_Exception_error("boom".to_owned());
        let raised = error.clone();
        let p = promise_unbox(&Promise_Internal_new().unwrap_func1()(Value::Func2(purust_core::Func2::Shared(Rc::new(move |_, _| {
            Purs_Effect_Exception::purust_exception_raise(raised.clone())
        })))));
        let original = Purs_Effect_Exception::purust_exception_unbox(&error);
        assert!(std::sync::Arc::ptr_eq(&original, &Purs_Effect_Exception::purust_exception_unbox(&failure(&p))));
        let raised = error.clone();
        let p = Promise_Internal_resolve(value(1)).chain(Some(function(move |_| Purs_Effect_Exception::purust_exception_raise(raised.clone()))), None);
        let seen = original.clone();
        p.subscribe(move |outcome| assert!(std::sync::Arc::ptr_eq(&seen, &Purs_Effect_Exception::purust_exception_unbox(&outcome.err().unwrap()))));
        queue.drain();
    });
}
#[test]
fn rejection_propagation_recovery_and_two_branch_abi() {
    in_loop(|queue| {
        let text = Value::String("original".to_owned());
        let p = promise_box(Promise_Internal_reject(text));
        let child = Promise_Internal_then_().unwrap_func2()(function(|_| panic!("success")), p);
        let recovered = promise_unbox(&Promise_Internal_catch().unwrap_func2()(function(|error| {
            assert_eq!(error.unwrap_string(), "original"); boxed(42)
        }), child));
        let sibling = promise_unbox(&Promise_Internal_thenOrCatch().unwrap_func3()(function(|v| boxed(v.unwrap_int() + 1)), function(|_| panic!("failure")), boxed(10)));
        queue.drain();
        assert_eq!(success(&recovered).unwrap_int(), 42);
        assert_eq!(success(&sibling).unwrap_int(), 11);
    });
}
#[test]
fn finally_waits_preserves_outcome_and_can_override_it() {
    in_loop(|queue| {
        for reject_original in [false, true] {
            for reject_cleanup in [false, true] {
                let cleanup = Promise::pending(queue.clone());
                let retained = cleanup.clone();
                let original = if reject_original { Promise_Internal_reject(value(1)) } else { Promise_Internal_resolve(value(1)) };
                let p = promise_unbox(&Promise_Internal_finally().unwrap_func2()(function(move |_| promise_box(retained.clone())), promise_box(original)));
                p.subscribe(|_| {});
                queue.drain();
                assert!(p.state.lock().unwrap().outcome.is_none());
                cleanup.resolve_once(if reject_cleanup { Err(value(2)) } else { Ok(value(99)) });
                queue.drain();
                if reject_cleanup { assert_eq!(failure(&p).unwrap_int(), 2); }
                else if reject_original { assert_eq!(failure(&p).unwrap_int(), 1); }
                else { assert_eq!(success(&p).unwrap_int(), 1); }
            }
        }
    });
}
#[test]
fn all_keeps_input_order_and_race_keeps_settlement_order() {
    in_loop(|queue| {
        let first = Promise::pending(queue.clone());
        let second = Promise::pending(queue.clone());
        let array = Value::Array(Rc::new(vec![promise_box(first.clone()), promise_box(second.clone())]));
        let all = promise_unbox(&Promise_Internal_all().unwrap_func1()(array.clone()));
        let race = promise_unbox(&Promise_Internal_race().unwrap_func1()(array));
        second.resolve_once(Ok(value(2)));
        queue.drain();
        assert_eq!(success(&race).unwrap_int(), 2);
        assert!(all.state.lock().unwrap().outcome.is_none());
        first.resolve_once(Ok(value(1)));
        queue.drain();
        assert_eq!(success(&all).unwrap_array().iter().map(|v| v.unwrap_int()).collect::<Vec<_>>(), vec![1, 2]);
    });
}
#[test]
fn aggregates_empty_input_rejection_and_losing_rejections() {
    in_loop(|queue| {
        let empty = Value::Array(Rc::new(vec![]));
        let all = promise_unbox(&Promise_Internal_all().unwrap_func1()(empty.clone()));
        assert!(success(&all).unwrap_array().is_empty());
        let race = promise_unbox(&Promise_Internal_race().unwrap_func1()(empty));
        assert!(race.state.lock().unwrap().outcome.is_none());
        for aggregate in [Promise_Internal_all(), Promise_Internal_race()] {
            let array = Value::Array(Rc::new(vec![promise_box(Promise_Internal_reject(value(1))), promise_box(Promise_Internal_reject(value(2)))]));
            let p = promise_unbox(&aggregate.unwrap_func1()(array));
            p.subscribe(|outcome| assert_eq!(outcome.err().unwrap().unwrap_int(), 1));
        }
        queue.drain();
    });
}
#[test]
fn handled_before_checkpoint_is_quiet_but_unhandled_is_not_swallowed() {
    in_loop(|queue| {
        let p = Promise_Internal_reject(value(1));
        queue.enqueue(move || p.subscribe(|_| {}));
        queue.drain();
        let original = Purs_Effect_Exception::Effect_Exception_error("uncaught".to_owned());
        Promise_Internal_reject(original.clone());
        let caught = promise_try(|| { queue.drain(); Value::Unit }).err().unwrap();
        assert!(std::sync::Arc::ptr_eq(&Purs_Effect_Exception::purust_exception_unbox(&original), &Purs_Effect_Exception::purust_exception_unbox(&caught)));
        // A panic must restore the queue's checkpoint guard.
        let p = Promise_Internal_resolve(value(1)).chain(Some(function(|_| boxed(2))), None);
        queue.drain();
        assert_eq!(success(&p).unwrap_int(), 2);
    });
}
#[test]
fn deep_chains_are_stack_safe() {
    in_loop(|queue| {
        let mut p = Promise_Internal_resolve(value(0));
        for _ in 0..10_000 { p = p.chain(Some(function(|v| boxed(v.unwrap_int() + 1))), None); }
        queue.drain();
        assert_eq!(success(&p).unwrap_int(), 10_000);
    });
}
#[cfg(feature = "threaded")]
#[test]
fn worker_resolution_waits_for_the_synchronous_turn() {
    in_loop(|queue| {
        let source = Promise::pending(queue.clone());
        let p = source.chain(Some(function(|_| boxed(42))), None);
        let (entered, ready) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let worker = queue.clone();
        let thread = std::thread::spawn(move || worker.turn(|| {
            source.resolve_once(Ok(value(1)));
            entered.send(()).unwrap();
            released.recv().unwrap();
        }));
        ready.recv().unwrap();
        queue.drain();
        assert!(p.state.lock().unwrap().outcome.is_none());
        release.send(()).unwrap();
        thread.join().unwrap();
        queue.drain();
        assert_eq!(success(&p).unwrap_int(), 42);
    });
}
