<?php
// M6 — PHP front-controller bridge for examples/web/server.phg, runnable under `php -S`.
//
// The handler logic is pure Phorge transpiled to PHP. The SAME handle(Request) -> Response that
// `phg serve` calls natively also runs here under PHP's built-in server — that value unit is what
// round-trips; the superglobal↔Request adapter below is runtime glue (NOT transpiled), exactly as
// `src/serve.rs` is the glue on the native side.
//
// Generate the transpiled handlers next to this file (minus the demo `main()` bootstrap), then
// start a dev server with this script as the router:
//
//   phg transpile server.phg | sed '$d' > web_app.php   # drop the trailing `main();` line
//   php -S 127.0.0.1:8080 server.php
//
// Then: curl -i http://127.0.0.1:8080/greet -H 'Host: phorge.dev'
require __DIR__ . '/web_app.php';

$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';
$path = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?: '/';
$body = file_get_contents('php://input') ?: '';

// Phorge's Request carries raw header lines ("Name: value"); rebuild them from $_SERVER's HTTP_* keys
// so req.header("Host") (and any other lookup) resolves exactly as it does on the native runtime.
$headerLines = [];
foreach ($_SERVER as $k => $v) {
    if (str_starts_with($k, 'HTTP_')) {
        $name = strReplace(' ', '-', ucwords(strtolower(strReplace('_', ' ', substr($k, 5)))));
        $headerLines[] = "$name: $v";
    }
}

$resp = handle(new Request($method, $path, $body, $headerLines));

http_response_code($resp->status);
foreach ($resp->headerLines as $h) {
    header($h);
}
echo $resp->body;
