<?php
abstract class Shape {}
final class Circle extends Shape {
    public function __construct(public float $r) {}
}
final class Square extends Shape {
    public function __construct(public float $side) {}
}
final class Named {
    function __construct(private string $label) {}
    function labelOf(): string {
        return $this->label;
    }
}
function area(Shape $s): float {
    if ($s instanceof Circle) { $r = $s->r; return (3.14159 * $r) * $r; }
    elseif ($s instanceof Square) { $side = $s->side; return $side * $side; }
    else { throw new \UnhandledMatchError(); }
}
function main(): void {
    $n = new Named("demo");
    echo __phorge_str($n->labelOf()) . ": circle area = " . __phorge_str(area(new Circle(2.0))) . "\n";
}
main();
function __phorge_str($v) {
    if (is_bool($v)) { return $v ? "true" : "false"; }
    if (is_float($v)) { return __phorge_float($v); }
    return (string)$v;
}
function __phorge_float($v) {
    if (is_nan($v)) { return "NaN"; }
    if (is_infinite($v)) { return $v < 0 ? "-inf" : "inf"; }
    if ($v == 0.0) { return (fdiv(1.0, $v) < 0) ? "-0" : "0"; }
    $neg = $v < 0;
    $a = $neg ? -$v : $v;
    $repr = sprintf("%.16e", $a);
    for ($p = 0; $p <= 16; $p++) {
        $cand = sprintf("%.{$p}e", $a);
        if ((float)$cand === $a) { $repr = $cand; break; }
    }
    $epos = strpos($repr, "e");
    $exp = (int)substr($repr, $epos + 1);
    $mant = str_replace(".", "", substr($repr, 0, $epos));
    $mant = rtrim($mant, "0");
    if ($mant === "") { $mant = "0"; }
    $ndig = strlen($mant);
    if ($exp >= $ndig - 1) {
        $s = $mant . str_repeat("0", $exp - ($ndig - 1));
    } elseif ($exp >= 0) {
        $s = substr($mant, 0, $exp + 1) . "." . substr($mant, $exp + 1);
    } else {
        $s = "0." . str_repeat("0", -$exp - 1) . $mant;
    }
    return $neg ? "-" . $s : $s;
}
