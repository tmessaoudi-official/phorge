<?php
abstract class Shape {}
final class Circle extends Shape {
    public function __construct(public float $r) {}
}
final class Square extends Shape {
    public function __construct(public float $side) {}
}
class Named {
    function __construct(private string $label) {}
    function label_of(): string {
        return $this->label;
    }
}
function area(Shape $s): float {
    if ($s instanceof Circle) { $r = $s->r; return (3.14159 * $r) * $r; }
    if ($s instanceof Square) { $side = $s->side; return $side * $side; }
    throw new \UnhandledMatchError();
}
function main(): void {
    $n = new Named("demo");
    echo __phorge_str($n->label_of()) . ": circle area = " . __phorge_str(area(new Circle(2.0))) . "\n";
}
main();
function __phorge_str($v) {
    if (is_bool($v)) { return $v ? "true" : "false"; }
    return (string)$v;
}
