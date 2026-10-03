<?php
// Generates the PHP serialize test data for deser-php.
//
// This runs inside the official PHP docker image (see
// scripts/update-php-test-data.sh).  It collects inputs and records how
// PHP's unserialize() reads them:
//
// * the inputs of cases.php and the serialization of its values
// * serialized looking string literals in the .phpt tests of php-src and
//   the strings that the expected output shows in full
// * the unserialize fuzzer corpus of php-src
//
// Every input is run in its own PHP process (so that crashes and fatal
// errors do not end the run and enums can be declared per input) with
// `allowed_classes => false`: objects are not instantiated and no magic
// methods run, which is the view of a data format.
//
// Usage: php generate.php <php-src> <cases.php> <output.json>
//        php generate.php --worker   (reads one input from stdin)

const MAX_INPUT = 131072;
const PATTERN = '/^(N;|[bidrR]:|[sSaOCE]:[-+]?\d)/';

if (($argv[1] ?? null) === '--worker') {
    $result = run_case(stream_get_contents(STDIN));
    // deeply nested values are left out, their result is only success
    $json = json_encode($result, 0, 512);
    if ($json === false && json_last_error() === JSON_ERROR_DEPTH) {
        unset($result['value']);
        $result['value_omitted'] = true;
        $json = json_encode($result, JSON_THROW_ON_ERROR);
    }
    echo $json;
    exit(0);
}

[$_, $phpSrc, $casesFile, $output] = $argv;

$inputs = [];
$add = function (string $input, string $source) use (&$inputs) {
    if (strlen($input) > MAX_INPUT) {
        return;
    }
    if (!isset($inputs[$input])) {
        $inputs[$input] = [];
    }
    if (!in_array($source, $inputs[$input], true)) {
        $inputs[$input][] = $source;
    }
};

$cases = (fn() => require $casesFile)();
foreach ($cases['inputs'] as $name => $input) {
    $add($input, "deser:$name");
}
foreach ($cases['values'] as $name => $make) {
    $add(serialize($make()), "deser:value:$name");
}

$files = [];
$iter = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($phpSrc, FilesystemIterator::SKIP_DOTS));
foreach ($iter as $file) {
    if (str_ends_with($file->getPathname(), '.phpt')) {
        $files[] = $file->getPathname();
    }
}
sort($files);
foreach ($files as $path) {
    $source = 'php-src/' . substr($path, strlen(rtrim($phpSrc, '/')) + 1);
    $phpt = file_get_contents($path);
    if (strpos($phpt, 'unserialize') === false) {
        continue;
    }
    $sections = parse_phpt($phpt);
    foreach (['FILE', 'FILEEOF'] as $name) {
        if (isset($sections[$name])) {
            foreach (harvest_literals($sections[$name]) as $input) {
                $add($input, $source);
            }
        }
    }
    foreach (['EXPECT', 'EXPECTF'] as $name) {
        if (isset($sections[$name])) {
            foreach (harvest_dumped_strings($sections[$name]) as $input) {
                $add($input, $source);
            }
        }
    }
}

$corpus = glob("$phpSrc/sapi/fuzzer/corpus/unserialize/*");
sort($corpus);
foreach ($corpus as $path) {
    $add(file_get_contents($path), 'php-src/sapi/fuzzer/corpus/unserialize/' . basename($path));
}

$lines = [];
foreach ($inputs as $input => $sources) {
    $input = (string) $input;
    $case = ['sources' => $sources];
    $case += encode_bytes('input', $input);
    $case += run_worker($input);
    // diagnostics can quote the input, they are not exact
    $lines[] = json_encode($case, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE, 1024);
}
file_put_contents($output, "[\n" . implode(",\n", $lines) . "\n]\n");
fprintf(STDERR, "%d cases written to %s\n", count($lines), $output);

/** Splits a .phpt file into its sections. */
function parse_phpt(string $phpt): array
{
    $sections = [];
    $current = null;
    foreach (preg_split('/(?<=\n)/', $phpt) as $line) {
        if (preg_match('/^--([A-Z_]+)--\r?\n?$/', $line, $m)) {
            $current = $m[1];
            $sections[$current] = '';
        } elseif ($current !== null) {
            $sections[$current] .= $line;
        }
    }
    return $sections;
}

/**
 * Returns the values of constant string expressions (literals joined with
 * `.`) that look like serialized data.
 */
function harvest_literals(string $code): array
{
    $tokens = @token_get_all($code);
    $found = [];
    $run = [];
    $flush = function () use (&$run, &$found) {
        // a run ends with a literal, not with a dangling `.`
        while ($run && end($run) === '.') {
            array_pop($run);
        }
        if ($run) {
            try {
                $value = eval('return ' . implode('', $run) . ';');
                if (is_string($value) && preg_match(PATTERN, $value)) {
                    $found[] = $value;
                }
            } catch (Throwable) {
            }
        }
        $run = [];
    };
    for ($i = 0; $i < count($tokens); $i++) {
        $token = $tokens[$i];
        if (is_array($token) && $token[0] === T_CONSTANT_ENCAPSED_STRING) {
            if ($run && end($run) !== '.') {
                $flush();
            }
            $run[] = $token[1];
        } elseif (is_array($token) && $token[0] === T_START_HEREDOC) {
            // only heredocs and nowdocs without interpolation
            $text = $token[1];
            $constant = true;
            for ($i++; $i < count($tokens); $i++) {
                $inner = $tokens[$i];
                $text .= is_array($inner) ? $inner[1] : $inner;
                if (is_array($inner) && $inner[0] === T_END_HEREDOC) {
                    break;
                }
                if (!is_array($inner) || $inner[0] !== T_ENCAPSED_AND_WHITESPACE) {
                    $constant = false;
                }
            }
            if ($run && end($run) !== '.') {
                $flush();
            }
            if ($constant) {
                $run[] = $text . "\n";
            } else {
                $flush();
            }
        } elseif ($token === '.' && $run) {
            $run[] = '.';
        } elseif (is_array($token) && in_array($token[0], [T_WHITESPACE, T_COMMENT], true)) {
            continue;
        } else {
            $flush();
        }
    }
    $flush();
    return $found;
}

/**
 * Returns the strings that var_dump() shows in full in the expected output
 * (`string(N) "..."` where the text is N bytes) and that look like
 * serialized data.
 */
function harvest_dumped_strings(string $expect): array
{
    $found = [];
    preg_match_all('/string\((\d+)\) "/', $expect, $matches, PREG_OFFSET_CAPTURE);
    foreach ($matches[0] as $index => [$match, $offset]) {
        $length = (int) $matches[1][$index][0];
        $start = $offset + strlen($match);
        if (($expect[$start + $length] ?? null) !== '"') {
            continue;
        }
        $value = substr($expect, $start, $length);
        // %s and friends of EXPECTF do not stand for themselves
        if (preg_match(PATTERN, $value) && !preg_match('/%[sSaAwidxfcer0]/', $value)) {
            $found[] = $value;
        }
    }
    return $found;
}

/** Runs an input in a fresh PHP process. */
function run_worker(string $input): array
{
    $proc = proc_open(
        [PHP_BINARY, '-d', 'display_errors=stderr', __FILE__, '--worker'],
        [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']],
        $pipes,
    );
    fwrite($pipes[0], $input);
    fclose($pipes[0]);
    $stdout = stream_get_contents($pipes[1]);
    $stderr = stream_get_contents($pipes[2]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    $status = proc_close($proc);
    $result = json_decode($stdout, true, 1024);
    if ($status !== 0 || !is_array($result)) {
        return ['fatal' => trim($stderr) !== '' ? trim($stderr) : "exit status $status"];
    }
    return $result;
}

/** Unserializes an input and describes the result. */
function run_case(string $input): array
{
    // E: needs the enum to exist, declare the referenced cases
    $enums = [];
    preg_match_all('/E:\d+:"([A-Za-z_\\\\][A-Za-z0-9_\\\\]*):([A-Za-z_][A-Za-z0-9_]*)";/', $input, $matches, PREG_SET_ORDER);
    foreach ($matches as [$_, $class, $case]) {
        $enums[ltrim($class, '\\')][$case] = true;
    }
    foreach ($enums as $class => $caseNames) {
        $pos = strrpos($class, '\\');
        $namespace = $pos === false ? '' : 'namespace ' . substr($class, 0, $pos) . ';';
        $short = $pos === false ? $class : substr($class, $pos + 1);
        $body = implode(' ', array_map(fn($c) => "case $c;", array_keys($caseNames)));
        try {
            eval("$namespace enum $short { $body }");
        } catch (Throwable) {
        }
    }

    $diagnostics = [];
    set_error_handler(function ($no, $message) use (&$diagnostics) {
        $diagnostics[] = preg_replace('/^unserialize\(\): /', '', $message);
        return true;
    });
    $exception = null;
    try {
        $value = unserialize($input, ['allowed_classes' => false]);
    } catch (Throwable $e) {
        $exception = get_class($e) . ': ' . $e->getMessage();
    }
    restore_error_handler();

    $result = [];
    $errorOffset = null;
    foreach ($diagnostics as $message) {
        if (preg_match('/^Error at offset (\d+) of \d+ bytes/', $message, $m)) {
            $errorOffset = (int) $m[1];
        }
    }
    if ($exception !== null || $errorOffset !== null) {
        $result['error'] = true;
        if ($errorOffset !== null) {
            $result['error_offset'] = $errorOffset;
        }
        if ($exception !== null) {
            $diagnostics[] = $exception;
        }
    } else {
        $state = ['objects' => [], 'refs' => []];
        $result['value'] = dump_value($value, $state);
        $serialized = serialize($value);
        if ($serialized === $input) {
            $result['canonical'] = true;
        } else {
            $result += encode_bytes('reserialized', $serialized);
        }
    }
    if ($diagnostics) {
        $result['diagnostics'] = $diagnostics;
    }
    return $result;
}

/**
 * Describes a value as JSON:
 *
 * * `["null"]`, `["bool", true]`, `["int", 1]`
 * * `["float", "0.1"]` with the text of serialize() (`INF`, `NAN`, `-0`, ...)
 * * `["string", "text"]` or `["bytes", "hex"]` if it is not valid UTF-8
 * * `["array", [[key, value], ...]]` with keys `["int", n]` or strings
 * * `["object", id, "Class", [[key, value], ...]]` with the mangled
 *   property names (`\0*\0name` for protected, `\0Class\0name` for private)
 *   and `["objref", id]` when the same object appears again (class names
 *   which are not valid UTF-8 are given as `{"hex": "..."}`)
 * * `["enum", "Class", "Case"]`
 *
 * Values nested too deeply to be written as JSON are left out (the result
 * has `value_omitted` instead of `value`).
 *
 * Values that are PHP references (`&`) are wrapped: the first appearance
 * as `["refdef", id, value]`, all others as `["ref", id]`.
 */
function dump_value(mixed $value, array &$state): array
{
    return match (true) {
        $value === null => ['null'],
        is_bool($value) => ['bool', $value],
        is_int($value) => ['int', $value],
        is_float($value) => ['float', substr(serialize($value), 2, -1)],
        is_string($value) => dump_string($value),
        is_array($value) => ['array', dump_entries($value, $state)],
        $value instanceof UnitEnum => ['enum', class_name(get_class($value)), $value->name],
        is_object($value) => dump_object($value, $state),
        default => throw new LogicException('cannot dump ' . get_debug_type($value)),
    };
}

function dump_string(string $value): array
{
    return preg_match('//u', $value) ? ['string', $value] : ['bytes', bin2hex($value)];
}

function dump_object(object $value, array &$state): array
{
    $id = spl_object_id($value);
    if (isset($state['objects'][$id])) {
        return ['objref', $state['objects'][$id]];
    }
    $number = count($state['objects']);
    $state['objects'][$id] = $number;
    $class = get_class($value);
    $vars = get_mangled_object_vars($value);
    if ($value instanceof __PHP_Incomplete_Class && is_string($vars['__PHP_Incomplete_Class_Name'] ?? null)) {
        $class = $vars['__PHP_Incomplete_Class_Name'];
        unset($vars['__PHP_Incomplete_Class_Name']);
    }
    return ['object', $number, class_name($class), dump_entries($vars, $state)];
}

/** Returns a class name, `{"hex": ...}` if it is not valid UTF-8. */
function class_name(string $name): string|array
{
    return preg_match('//u', $name) ? $name : ['hex' => bin2hex($name)];
}

function dump_entries(array $entries, array &$state): array
{
    $out = [];
    foreach (array_keys($entries) as $key) {
        $dumpedKey = is_int($key) ? ['int', $key] : dump_string($key);
        $ref = ReflectionReference::fromArrayElement($entries, $key);
        if ($ref === null) {
            $out[] = [$dumpedKey, dump_value($entries[$key], $state)];
            continue;
        }
        $refId = $ref->getId();
        if (isset($state['refs'][$refId])) {
            $out[] = [$dumpedKey, ['ref', $state['refs'][$refId]]];
            continue;
        }
        $number = count($state['refs']);
        $state['refs'][$refId] = $number;
        $out[] = [$dumpedKey, ['refdef', $number, dump_value($entries[$key], $state)]];
    }
    return $out;
}

/** Stores bytes under `$name` if they are UTF-8, as hex under `${name}_hex` otherwise. */
function encode_bytes(string $name, string $bytes): array
{
    return preg_match('//u', $bytes) ? [$name => $bytes] : ["{$name}_hex" => bin2hex($bytes)];
}
