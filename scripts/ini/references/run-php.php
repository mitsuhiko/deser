<?php
// Prints how PHP's parse_ini_string parses files, for scripts/ini/references.
//
// For every file named on stdin (one per line, relative to the data
// directory) it prints a JSON line per scanner mode (raw, normal), with
// sections processed:
//
//   {"file": ..., "reference": "php", "variant": ...,
//    "result": {"entries": [[[key, ...], value], ...]}}
//   or "result": {"error": {"message": ..., "line": ...}}
//
// Entries are the leaves of the returned array with their key path (keys
// before the first section have a path of one key, [] arrays are nested).
// An empty array (an empty section) is a leaf with the value [].  Strings
// that are not UTF-8 are stored as {"hex": ...}.  Run it with an empty
// environment, ${NAME} looks up environment variables.
//
// Usage: php run-php.php <data-dir> < inputs

const VARIANTS = ['raw' => INI_SCANNER_RAW, 'normal' => INI_SCANNER_NORMAL];

function text($value) {
    if (is_string($value) && !mb_check_encoding($value, 'UTF-8')) {
        return ['hex' => bin2hex($value)];
    }
    return $value;
}

function leaves(array $array, array $path, array &$out) {
    foreach ($array as $key => $value) {
        $keyPath = array_merge($path, [text($key)]);
        if (is_array($value) && $value) {
            leaves($value, $keyPath, $out);
        } else {
            $out[] = [$keyPath, is_array($value) ? [] : text($value)];
        }
    }
}

function parse(string $data, int $mode): array {
    $error = null;
    set_error_handler(function ($errno, $message) use (&$error) {
        $error ??= $message;
        return true;
    });
    $result = parse_ini_string($data, true, $mode);
    restore_error_handler();
    if ($result === false) {
        $message = $error ?? 'parse error';
        $out = ['message' => trim(preg_replace('/ in Unknown on line \d+$/', '', $message))];
        if (preg_match('/ on line (\d+)$/', $message, $m)) {
            $out['line'] = (int)$m[1];
        }
        return ['error' => $out];
    }
    $entries = [];
    leaves($result, [], $entries);
    return ['entries' => $entries];
}

$root = $argv[1];
while (($line = fgets(STDIN)) !== false) {
    $path = trim($line);
    if ($path === '') {
        continue;
    }
    $data = file_get_contents("$root/$path");
    foreach (VARIANTS as $variant => $mode) {
        $record = [
            'file' => $path,
            'reference' => 'php',
            'variant' => $variant,
            'result' => parse($data, $mode),
        ];
        echo json_encode($record, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "\n";
    }
}
