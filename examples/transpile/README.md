# Phorge → PHP

Phorge can transpile to runnable **PHP 8.x**. This is the only Phorge↔PHP-ecosystem path: the
transpiler *produces* PHP source; Phorge does **not** consume Composer/PHP packages (FFI and live
transpile were rejected in the ecosystem roadmap).

```bash
phorge transpile demo.phg > demo.php   # regenerate the committed output
php demo.php                           # run it under any PHP 8.x
```

- `demo.phg` is a normal Phorge program — it also runs on both native backends
  (`phorge run demo.phg` / `phorge runvm demo.phg`) and is in the byte-identity sweep.
- `demo.php` is the committed output of `phorge transpile demo.phg`, kept in sync by a snapshot
  test (`tests/cli.rs::transpile_demo_matches_committed_php`) — regenerate it and re-commit if you
  change `demo.phg`.
- A separate round-trip test (`tests/cli.rs::transpiled_php_runs_and_matches_interpreter`) runs the
  emitted PHP under a real `php` when one is on `PATH`, asserting it prints exactly what the
  interpreter prints.

Note how `match` lowers to `instanceof` chains and enum variants become `final class … extends`
the enum's abstract base — idiomatic PHP 8.x.
