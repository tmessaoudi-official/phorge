<?php
// A small, typed PHP program — the kind `phg lift` handles 1:1 (Tier-1), now including
// double-quoted string interpolation: simple `"$name"`, simple `"$o->prop"`, and complex
// `"{$o->method()}"` all lift to Phorj `"{…}"` holes (the faithful access-chain subset).
// Run `phg lift sample.php` to see the Phorj draft (committed alongside as sample.phg).

function greet(string $name): string {
    return "Hello, $name!";
}

class Counter {
    public function __construct(public int $start) {}

    public function next(): int {
        return $this->start + 1;
    }
}

$c = new Counter(41);
echo greet("Phorj");
echo " Counter starts at $c->start, next is {$c->next()}.";
