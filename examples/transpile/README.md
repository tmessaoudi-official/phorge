# Phorj → PHP

Phorj can transpile to runnable **PHP 8.x**. This is the only Phorj↔PHP-ecosystem path: the
transpiler *produces* PHP source; Phorj does **not** consume Composer/PHP packages (FFI and live
transpile were rejected in the ecosystem roadmap).

```bash
phg transpile demo.phg > demo.php   # regenerate the committed output
php demo.php                           # run it under any PHP 8.x
```

- `demo.phg` is a normal Phorj program — it also runs on both native backends
  (`phg run demo.phg` / `phg runvm demo.phg`) and is in the byte-identity sweep.
- `demo.php` is the committed output of `phg transpile demo.phg`, kept in sync by a snapshot
  test (`tests/cli.rs::transpile_demo_matches_committed_php`) — regenerate it and re-commit if you
  change `demo.phg`.
- A separate round-trip test (`tests/cli.rs::transpiled_php_runs_and_matches_interpreter`) runs the
  emitted PHP under a real `php` when one is on `PATH`, asserting it prints exactly what the
  interpreter prints.

Note how `match` lowers to `instanceof` chains and enum variants become `final class … extends`
the enum's abstract base — idiomatic PHP 8.x.
