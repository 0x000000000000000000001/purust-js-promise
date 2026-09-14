# purescript-js-promise

[![Latest release](http://img.shields.io/github/release/purescript-contrib/purescript-js-promise.svg)](https://github.com/purescript-web/purescript-js-promise/releases)
[![Build status](https://github.com/purescript-contrib/purescript-js-promise/workflows/CI/badge.svg?branch=master)](https://github.com/purescript-web/purescript-js-promise/actions?query=workflow%3ACI+branch%3Amaster)
[![Pursuit](https://pursuit.purescript.org/packages/purescript-js-promise/badge)](https://pursuit.purescript.org/packages/purescript-js-promise)

Types and low-level implementations for JavaScript Promises.

## Installation

```
spago install js-promise
```

## Documentation

Module documentation is [published on Pursuit](http://pursuit.purescript.org/packages/purescript-js-promise).

## Native Rust FFI

`Promise/Internal.rs` implements the native Promise carrier in Rc and threaded
Arc mode. `Promise/Rejection.rs` preserves the original rejected value; only
native `Effect.Exception.Error` values are recognized by `Rejection.toError`.
String and custom-record rejections remain readable through `Foreign`.

The purust entry point installs a microtask queue. Executors run immediately;
reactions run FIFO at a checkpoint after synchronous turns finish. A resolver
may be called from a native worker in threaded mode: it schedules reactions,
without running PureScript callbacks on that worker's resolving stack. Aff
integrates the same queue and waits for queued work before exiting.

Settlement is single-shot, including while adopting another native Promise.
The implementation supports chaining, rejection recovery, `finally`, `all`,
`race`, self-resolution rejection and uncaught-rejection propagation. `finally`
waits for cleanup and preserves its observable JavaScript job ordering. This
is native Promise adoption, not an interpreter for arbitrary JavaScript
thenable objects. A pending Promise alone does not keep a process alive;
native asynchronous producers must register their lifetime with the runtime
(for example through Aff).

After rebuilding the sibling `purust` compiler, run:

```sh
node test/native-contract.mjs
node ../purust-js-promise-aff/test/native.mjs
```

The first suite compiles the actual Rust FFI in Rc/Arc and compares a combined
reaction trace with the original JavaScript FFI. The second uses fresh TAST,
the unchanged Promise.Aff assertions, and delayed/cancellation bridge checks.
