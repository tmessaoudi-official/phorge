//! Checker test suite, split by language feature (M-Decomp W2b). These are integration
//! tests through the public `check()`; shared helpers live in `support`.

mod basics;
mod calls;
mod casing;
mod casting;
mod collections;
mod constants;
mod ctor_visibility;
mod decimal;
mod default_params;
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
mod static_methods;
mod support;
mod throws;
mod totality;
mod traits;
mod types;
mod unions;
mod uniqueness;
mod visibility;
