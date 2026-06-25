//! Checker test suite, split by language feature (M-Decomp W2b). These are integration
//! tests through the public `check()`; shared helpers live in `support`.

mod basics;
mod calls;
mod casing;
mod collections;
mod constants;
mod destructuring;
mod field_init;
mod generics;
mod inheritance;
mod interfaces;
mod intersections;
mod loops;
mod matching;
mod mutation;
mod optionals;
mod overloading;
mod reflect;
mod support;
mod throws;
mod totality;
mod traits;
mod types;
mod unions;
