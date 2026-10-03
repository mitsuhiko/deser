<?php
// Handwritten inputs and values for the PHP serialize test data (see
// generate.php).  `inputs` are passed to unserialize() as they are,
// `values` are serialized first.  The names only identify the cases.

$nest = fn(int $depth, string $inner) => str_repeat('a:1:{i:0;', $depth) . $inner . str_repeat('}', $depth);
$nestObjects = fn(int $depth) => str_repeat('O:1:"A":1:{i:0;', $depth) . 'N;' . str_repeat('}', $depth);

$inputs = [
    // empty and garbage
    'empty' => '',
    'space' => ' ',
    'unknown-type' => 'x:1;',
    'lowercase-null' => 'n;',
    'nul-byte' => "\0",

    // null
    'null' => 'N;',
    'null-no-semicolon' => 'N',
    'null-colon' => 'N:;',
    'null-space' => 'N ;',
    'null-trailing' => 'N;x',
    'null-trailing-space' => "N; ",
    'null-trailing-newline' => "N;\n",
    'null-twice' => 'N;N;',
    'leading-space' => ' N;',

    // booleans
    'bool-true' => 'b:1;',
    'bool-false' => 'b:0;',
    'bool-two' => 'b:2;',
    'bool-minus' => 'b:-1;',
    'bool-plus' => 'b:+1;',
    'bool-leading-zero' => 'b:01;',
    'bool-empty' => 'b:;',
    'bool-text' => 'b:true;',
    'bool-no-semicolon' => 'b:1',
    'bool-space' => 'b: 1;',

    // integers
    'int-zero' => 'i:0;',
    'int-one' => 'i:1;',
    'int-minus-one' => 'i:-1;',
    'int-plus' => 'i:+1;',
    'int-minus-zero' => 'i:-0;',
    'int-plus-zero' => 'i:+0;',
    'int-leading-zeros' => 'i:007;',
    'int-many-leading-zeros' => 'i:' . str_repeat('0', 40) . '1;',
    'int-max' => 'i:9223372036854775807;',
    'int-min' => 'i:-9223372036854775808;',
    'int-max-plus-one' => 'i:9223372036854775808;',
    'int-min-minus-one' => 'i:-9223372036854775809;',
    'int-huge' => 'i:99999999999999999999999999;',
    'int-huge-negative' => 'i:-99999999999999999999999999;',
    'int-empty' => 'i:;',
    'int-sign-only' => 'i:-;',
    'int-double-sign' => 'i:--1;',
    'int-space-before' => 'i: 1;',
    'int-space-after' => 'i:1 ;',
    'int-hex' => 'i:0x10;',
    'int-float' => 'i:1.5;',
    'int-exponent' => 'i:1e3;',
    'int-underscore' => 'i:1_000;',
    'int-no-semicolon' => 'i:1',
    'int-no-colon' => 'i1;',
    'int-uppercase' => 'I:1;',

    // floats
    'float-zero' => 'd:0;',
    'float-zero-point' => 'd:0.0;',
    'float-minus-zero' => 'd:-0;',
    'float-tenth' => 'd:0.1;',
    'float-third' => 'd:0.3333333333333333;',
    'float-integer' => 'd:42;',
    'float-plus' => 'd:+1.5;',
    'float-leading-dot' => 'd:.5;',
    'float-trailing-dot' => 'd:5.;',
    'float-minus-leading-dot' => 'd:-.5;',
    'float-dot-only' => 'd:.;',
    'float-exponent' => 'd:1e3;',
    'float-exponent-upper' => 'd:1E3;',
    'float-exponent-plus' => 'd:1.0E+25;',
    'float-exponent-minus' => 'd:1.0E-25;',
    'float-exponent-no-digits' => 'd:1e;',
    'float-exponent-sign-only' => 'd:1e+;',
    'float-exponent-leading-dot' => 'd:.5e1;',
    'float-max' => 'd:1.7976931348623157E+308;',
    'float-overflow' => 'd:1e400;',
    'float-underflow' => 'd:1e-400;',
    'float-subnormal' => 'd:5.0E-324;',
    'float-many-digits' => 'd:0.1000000000000000055511151231257827021181583404541015625;',
    'float-inf' => 'd:INF;',
    'float-minus-inf' => 'd:-INF;',
    'float-plus-inf' => 'd:+INF;',
    'float-nan' => 'd:NAN;',
    'float-minus-nan' => 'd:-NAN;',
    'float-inf-lowercase' => 'd:inf;',
    'float-nan-lowercase' => 'd:nan;',
    'float-infinity' => 'd:INFINITY;',
    'float-empty' => 'd:;',
    'float-sign-only' => 'd:-;',
    'float-hex' => 'd:0x10;',
    'float-space' => 'd: 1;',
    'float-underscore' => 'd:1_0;',
    'float-no-semicolon' => 'd:1.5',
    'float-leading-zeros' => 'd:007.5;',

    // strings
    'string-empty' => 's:0:"";',
    'string' => 's:5:"hello";',
    'string-utf8' => "s:6:\"\u{e4}\u{f6}\u{fc}\";",
    'string-emoji' => "s:4:\"\u{1F600}\";",
    'string-latin1' => "s:3:\"\xe4\xf6\xfc\";",
    'string-invalid-utf8' => "s:2:\"\xc3\x28\";",
    'string-nul' => "s:3:\"a\0b\";",
    'string-quotes' => 's:3:"a"b";',
    'string-only-quote' => 's:1:""";',
    'string-semicolon' => 's:1:";";',
    'string-newline' => "s:3:\"a\nb\";",
    'string-backslash' => 's:2:"\\\\";',
    'string-too-short' => 's:3:"ab";',
    'string-too-long' => 's:1:"ab";',
    'string-length-past-end' => 's:100:"ab";',
    'string-huge-length' => 's:99999999999999999999:"ab";',
    'string-plus-length' => 's:+1:"a";',
    'string-minus-length' => 's:-1:"a";',
    'string-leading-zero-length' => 's:01:"a";',
    'string-empty-length' => 's::"a";',
    'string-no-quotes' => 's:1:a;',
    'string-single-quotes' => "s:1:'a';",
    'string-no-closing-quote' => 's:1:"a;',
    'string-no-semicolon' => 's:1:"a"',
    'string-truncated' => 's:1:"a',
    'string-space-length' => 's: 1:"a";',
    'string-hex-length' => 's:0x1:"a";',

    // escaped strings
    'escaped-string' => 'S:1:"a";',
    'escaped-string-hex' => 'S:1:"\\61";',
    'escaped-string-hex-upper' => 'S:1:"\\4A";',
    'escaped-string-nul' => 'S:1:"\\00";',
    'escaped-string-high' => 'S:1:"\\ff";',
    'escaped-string-mixed' => 'S:3:"a\\62c";',
    'escaped-string-backslash' => 'S:1:"\\5c";',
    'escaped-string-bad-hex' => 'S:1:"\\zz";',
    'escaped-string-short-hex' => 'S:1:"\\6";',
    'escaped-string-length-counts-escapes' => 'S:3:"\\61";',
    'escaped-string-empty' => 'S:0:"";',

    // arrays
    'array-empty' => 'a:0:{}',
    'array-empty-semicolon' => 'a:0:{};',
    'array-list' => 'a:3:{i:0;i:1;i:1;i:2;i:2;i:3;}',
    'array-list-unordered' => 'a:2:{i:1;s:1:"b";i:0;s:1:"a";}',
    'array-list-gap' => 'a:2:{i:0;i:1;i:2;i:2;}',
    'array-list-starts-at-one' => 'a:2:{i:1;i:1;i:2;i:2;}',
    'array-negative-keys' => 'a:2:{i:-1;i:1;i:-2;i:2;}',
    'array-string-keys' => 'a:2:{s:1:"a";i:1;s:1:"b";i:2;}',
    'array-mixed-keys' => 'a:3:{i:0;i:1;s:1:"a";i:2;i:5;i:3;}',
    'array-empty-string-key' => 'a:1:{s:0:"";i:1;}',
    'array-numeric-string-key' => 'a:1:{s:1:"5";i:1;}',
    'array-numeric-string-key-zero' => 'a:1:{s:1:"0";i:1;}',
    'array-numeric-string-key-negative' => 'a:1:{s:2:"-5";i:1;}',
    'array-numeric-string-key-minus-zero' => 'a:1:{s:2:"-0";i:1;}',
    'array-numeric-string-key-leading-zero' => 'a:1:{s:2:"05";i:1;}',
    'array-numeric-string-key-plus' => 'a:1:{s:2:"+5";i:1;}',
    'array-numeric-string-key-space' => 'a:1:{s:2:" 5";i:1;}',
    'array-numeric-string-key-float' => 'a:1:{s:3:"1.5";i:1;}',
    'array-numeric-string-key-max' => 'a:1:{s:19:"9223372036854775807";i:1;}',
    'array-numeric-string-key-overflow' => 'a:1:{s:19:"9223372036854775808";i:1;}',
    'array-numeric-string-key-min' => 'a:1:{s:20:"-9223372036854775808";i:1;}',
    'array-escaped-string-key' => 'a:1:{S:1:"\\61";i:1;}',
    'array-binary-key' => "a:1:{s:1:\"\xff\";i:1;}",
    'array-duplicate-int-keys' => 'a:2:{i:0;i:1;i:0;i:2;}',
    'array-duplicate-string-keys' => 'a:2:{s:1:"a";i:1;s:1:"a";i:2;}',
    'array-duplicate-numeric-keys' => 'a:2:{i:5;i:1;s:1:"5";i:2;}',
    'array-null-key' => 'a:1:{N;i:1;}',
    'array-bool-key' => 'a:1:{b:1;i:1;}',
    'array-float-key' => 'a:1:{d:1.5;i:1;}',
    'array-array-key' => 'a:1:{a:0:{}i:1;}',
    'array-object-key' => 'a:1:{O:8:"stdClass":0:{}i:1;}',
    'array-nested' => 'a:1:{s:1:"a";a:1:{s:1:"b";a:0:{}}}',
    'array-all-types' => 'a:6:{i:0;N;i:1;b:1;i:2;i:3;i:3;d:0.5;i:4;s:1:"x";i:5;a:0:{}}',
    'array-count-too-small' => 'a:1:{i:0;i:1;i:1;i:2;}',
    'array-count-too-large' => 'a:2:{i:0;i:1;}',
    'array-count-huge' => 'a:99999999:{}',
    'array-count-overflow' => 'a:99999999999999999999:{}',
    'array-count-plus' => 'a:+0:{}',
    'array-count-minus' => 'a:-1:{}',
    'array-count-minus-zero' => 'a:-0:{}',
    'array-count-leading-zero' => 'a:01:{i:0;i:1;}',
    'array-count-empty' => 'a::{}',
    'array-no-count' => 'a:{}',
    'array-no-braces' => 'a:0:',
    'array-no-closing-brace' => 'a:1:{i:0;i:1;',
    'array-missing-value' => 'a:1:{i:0;}',
    'array-space-before-brace' => 'a:0: {}',
    'array-space-inside' => 'a:0:{ }',
    'array-semicolon-before-brace' => 'a:1:{i:0;i:1;;}',
    'array-semicolon-after-entry-brace' => 'a:1:{i:0;a:0:{};}',
    'array-trailing' => 'a:0:{}x',
    'array-trailing-brace' => 'a:0:{}}',
    'array-uppercase' => 'A:0:{}',
    'nested-4095' => $nest(4095, 'N;'),
    'nested-4096' => $nest(4096, 'N;'),
    'nested-4097' => $nest(4097, 'N;'),
    'nested-objects-4096' => $nestObjects(4096),
    'nested-objects-4097' => $nestObjects(4097),

    // objects
    'object-empty' => 'O:8:"stdClass":0:{}',
    'object' => 'O:8:"stdClass":2:{s:1:"a";i:1;s:1:"b";s:1:"x";}',
    'object-user-class' => 'O:3:"Foo":1:{s:3:"bar";i:1;}',
    'object-namespaced' => 'O:7:"Foo\\Bar":0:{}',
    'object-leading-backslash' => 'O:8:"\\Foo\\Bar":0:{}',
    'object-lowercase-class' => 'O:3:"foo":0:{}',
    'object-protected' => "O:3:\"Foo\":1:{s:6:\"\0*\0bar\";i:1;}",
    'object-private' => "O:3:\"Foo\":1:{s:8:\"\0Foo\0bar\";i:1;}",
    'object-private-parent' => "O:3:\"Foo\":1:{s:11:\"\0Parent\0bar\";i:1;}",
    'object-private-and-public' => "O:3:\"Foo\":2:{s:8:\"\0Foo\0bar\";i:1;s:3:\"bar\";i:2;}",
    'object-bad-mangling' => "O:3:\"Foo\":1:{s:4:\"\0bar\";i:1;}",
    'object-bad-mangling-unterminated' => "O:3:\"Foo\":1:{s:5:\"\0Foo2\";i:1;}",
    'object-empty-property' => 'O:3:"Foo":1:{s:0:"";i:1;}',
    'object-nul-property' => "O:3:\"Foo\":1:{s:1:\"\0\";i:1;}",
    'object-int-property' => 'O:3:"Foo":1:{i:0;i:1;}',
    'object-numeric-property' => 'O:3:"Foo":1:{s:1:"0";i:1;}',
    'object-duplicate-property' => 'O:3:"Foo":2:{s:1:"a";i:1;s:1:"a";i:2;}',
    'object-null-property' => 'O:3:"Foo":1:{N;i:1;}',
    'object-incomplete-class-name-property' => 'O:3:"Foo":1:{s:27:"__PHP_Incomplete_Class_Name";s:3:"Bar";}',
    'object-incomplete-class' => 'O:22:"__PHP_Incomplete_Class":0:{}',
    'object-nested' => 'O:3:"Foo":1:{s:1:"a";O:3:"Bar":1:{s:1:"b";a:0:{}}}',
    'object-empty-class' => 'O:0:"":0:{}',
    'object-class-digit' => 'O:3:"1ab":0:{}',
    'object-class-dash' => 'O:3:"a-b":0:{}',
    'object-class-space' => 'O:3:"a b":0:{}',
    'object-class-high-byte' => "O:3:\"\xe4bc\":0:{}",
    'object-class-utf8' => "O:4:\"\u{e4}bc\":0:{}",
    'object-class-double-backslash' => 'O:5:"A\\\\\\\\B":0:{}',
    'object-class-trailing-backslash' => 'O:2:"A\\\\":0:{}',
    'object-class-length-too-short' => 'O:2:"Foo":0:{}',
    'object-class-length-too-long' => 'O:4:"Foo":0:{}',
    'object-count-too-small' => 'O:3:"Foo":0:{s:1:"a";i:1;}',
    'object-count-too-large' => 'O:3:"Foo":2:{s:1:"a";i:1;}',
    'object-count-minus' => 'O:3:"Foo":-1:{}',
    'object-count-plus' => 'O:3:"Foo":+0:{}',
    'object-count-plus-one' => 'O:3:"Foo":+1:{s:1:"a";i:1;}',
    'object-count-empty' => 'O:3:"Foo"::{}',
    'object-count-plus-only' => 'O:3:"Foo":+:{}',
    'object-count-minus-only' => 'O:3:"Foo":-:{}',
    'object-count-minus-zero' => 'O:3:"Foo":-0:{}',
    'object-count-leading-zero' => 'O:3:"Foo":01:{s:1:"a";i:1;}',
    'object-count-double-sign' => 'O:3:"Foo":+-1:{}',
    'object-no-count' => 'O:3:"Foo":{}',
    'object-no-closing-brace' => 'O:3:"Foo":0:{',
    'object-trailing-semicolon' => 'O:3:"Foo":0:{};',
    'object-lowercase' => 'o:3:"Foo":0:{}',

    // custom serialized objects
    'custom-empty' => 'C:3:"Foo":0:{}',
    'custom' => 'C:3:"Foo":5:{hello}',
    'custom-braces' => 'C:3:"Foo":4:{{}{}}',
    'custom-binary' => "C:3:\"Foo\":3:{\0\xff\0}",
    'custom-length-too-short' => 'C:3:"Foo":4:{hello}',
    'custom-length-too-long' => 'C:3:"Foo":6:{hello}',
    'custom-length-minus' => 'C:3:"Foo":-1:{}',
    'custom-in-array' => 'a:2:{i:0;C:3:"Foo":1:{x}i:1;i:2;}',
    'custom-stdclass' => 'C:8:"stdClass":0:{}',

    // enums
    'enum' => 'E:7:"Foo:Bar";',
    'enum-namespaced' => 'E:10:"Ns\\Foo:Bar";',
    'enum-in-array' => 'a:2:{i:0;E:7:"Foo:Bar";i:1;E:7:"Foo:Baz";}',
    'enum-twice' => 'a:2:{i:0;E:7:"Foo:Bar";i:1;E:7:"Foo:Bar";}',
    'enum-no-colon' => 'E:6:"FooBar";',
    'enum-empty-case' => 'E:4:"Foo:";',
    'enum-empty-class' => 'E:4:":Bar";',
    'enum-two-colons' => 'E:11:"Foo:Bar:Baz";',
    'enum-length-too-short' => 'E:6:"Foo:Bar";',
    'enum-length-too-long' => 'E:8:"Foo:Bar";',
    'enum-no-semicolon' => 'E:7:"Foo:Bar"',
    'enum-braces' => 'E:7:"Foo:Bar":0:{}',
    'enum-as-key' => 'a:1:{E:7:"Foo:Bar";i:1;}',

    // references
    'ref-root' => 'r:1;',
    'ref-root-upper' => 'R:1;',
    'ref-zero' => 'a:1:{i:0;r:0;}',
    'ref-upper-zero' => 'a:1:{i:0;R:0;}',
    'ref-to-array' => 'a:1:{i:0;r:1;}',
    'ref-upper-to-array' => 'a:1:{i:0;R:1;}',
    'ref-to-int' => 'a:2:{i:0;i:5;i:1;r:2;}',
    'ref-upper-to-int' => 'a:2:{i:0;i:5;i:1;R:2;}',
    'ref-upper-to-string' => 'a:2:{i:0;s:1:"a";i:1;R:2;}',
    'ref-upper-to-null' => 'a:2:{i:0;N;i:1;R:2;}',
    'ref-upper-to-nested-array' => 'a:2:{i:0;a:1:{i:0;i:1;}i:1;R:2;}',
    'ref-upper-into-nested-array' => 'a:2:{i:0;a:1:{i:0;i:1;}i:1;R:3;}',
    'ref-upper-chain' => 'a:3:{i:0;i:5;i:1;R:2;i:2;R:3;}',
    'ref-upper-chain-first' => 'a:3:{i:0;i:5;i:1;R:2;i:2;R:2;}',
    'ref-upper-numbering' => 'a:4:{i:0;i:1;i:1;R:2;i:2;i:3;i:3;R:4;}',
    'ref-to-object' => 'a:2:{i:0;O:3:"Foo":0:{}i:1;r:2;}',
    'ref-upper-to-object' => 'a:2:{i:0;O:3:"Foo":0:{}i:1;R:2;}',
    'ref-to-object-property' => 'a:2:{i:0;O:3:"Foo":1:{s:1:"a";i:1;}i:1;R:3;}',
    'ref-numbering-after-object' => 'a:3:{i:0;O:3:"Foo":1:{s:1:"a";i:1;}i:1;i:2;i:2;R:4;}',
    'ref-numbering-after-ref' => 'a:3:{i:0;O:3:"Foo":0:{}i:1;r:2;i:2;r:3;}',
    'ref-numbering-after-upper-ref' => 'a:3:{i:0;i:1;i:1;R:2;i:2;R:3;}',
    'ref-object-self' => 'O:3:"Foo":1:{s:4:"self";r:1;}',
    'ref-upper-object-self' => 'O:3:"Foo":1:{s:4:"self";R:1;}',
    'ref-object-cycle' => 'O:3:"Foo":1:{s:1:"a";O:3:"Bar":1:{s:1:"b";r:1;}}',
    'ref-to-enum' => 'a:2:{i:0;E:7:"Foo:Bar";i:1;r:2;}',
    'ref-upper-to-enum' => 'a:2:{i:0;E:7:"Foo:Bar";i:1;R:2;}',
    'ref-to-custom' => 'a:2:{i:0;C:3:"Foo":1:{x}i:1;r:2;}',
    'ref-forward' => 'a:2:{i:0;r:3;i:1;O:3:"Foo":0:{}}',
    'ref-upper-forward' => 'a:2:{i:0;R:3;i:1;i:1;}',
    'ref-out-of-range' => 'a:1:{i:0;r:5;}',
    'ref-upper-out-of-range' => 'a:1:{i:0;R:5;}',
    'ref-minus' => 'a:1:{i:0;r:-1;}',
    'ref-plus' => 'a:2:{i:0;O:3:"Foo":0:{}i:1;r:+2;}',
    'ref-huge' => 'a:1:{i:0;r:99999999999999999999;}',
    'ref-as-key' => 'a:2:{i:0;i:1;r:2;i:2;}',
    'ref-upper-as-key' => 'a:2:{i:0;i:1;R:2;i:2;}',
    'ref-no-semicolon' => 'a:2:{i:0;O:3:"Foo":0:{}i:1;r:2}',
    'ref-keys-not-counted' => 'a:2:{s:1:"a";O:3:"Foo":0:{}s:1:"b";r:2;}',
    'ref-object-keys-not-counted' => 'O:3:"Foo":2:{s:1:"a";i:1;s:1:"b";R:2;}',
    'ref-upper-overwritten-target' => 'a:3:{i:0;i:1;i:1;R:2;i:0;i:2;}',
    'ref-upper-to-overwritten' => 'a:3:{i:0;i:1;i:0;i:2;i:1;R:2;}',
];

$values = [
    'null' => fn() => null,
    'true' => fn() => true,
    'false' => fn() => false,
    'int-max' => fn() => PHP_INT_MAX,
    'int-min' => fn() => PHP_INT_MIN,
    'float-tenth' => fn() => 0.1,
    'float-integer' => fn() => 1.0,
    'float-minus-zero' => fn() => -0.0,
    'float-large' => fn() => 1e100,
    'float-small' => fn() => 1e-100,
    'float-1e15' => fn() => 1e15,
    'float-1e16' => fn() => 1e16,
    'float-max' => fn() => PHP_FLOAT_MAX,
    'float-min' => fn() => PHP_FLOAT_MIN,
    'float-epsilon' => fn() => PHP_FLOAT_EPSILON,
    'float-subnormal' => fn() => 5e-324,
    'float-inf' => fn() => INF,
    'float-minus-inf' => fn() => -INF,
    'float-nan' => fn() => NAN,
    'float-pi' => fn() => M_PI,
    'string-empty' => fn() => '',
    'string-utf8' => fn() => "\u{e4}\u{1F600}",
    'string-binary' => fn() => "\0\xff",
    'list' => fn() => [1, 2, 3],
    'list-unset' => function () {
        $list = [1, 2, 3];
        unset($list[1]);
        return $list;
    },
    'map' => fn() => ['a' => 1, 'b' => [true, null]],
    'numeric-string-keys' => fn() => ['5' => 'a', '05' => 'b', '-5' => 'c', '-0' => 'd'],
    'nested' => fn() => ['a' => ['b' => ['c' => []]]],
    'stdclass' => fn() => (object) ['a' => 1, 'b' => 'x'],
    'stdclass-int-property' => fn() => (object) [0 => 'a'],
    'visibility' => fn() => new DeserVisibility(),
    'visibility-subclass' => fn() => new DeserVisibilityChild(),
    'shared-object' => function () {
        $object = new stdClass();
        return [$object, $object];
    },
    'object-cycle' => function () {
        $object = new stdClass();
        $object->self = $object;
        return $object;
    },
    'reference' => function () {
        $value = 1;
        return ['a' => &$value, 'b' => &$value];
    },
    'reference-to-array' => function () {
        $value = [1, 2];
        return [&$value, &$value];
    },
    'reference-cycle' => function () {
        $value = [];
        $value[0] = &$value;
        return [&$value];
    },
    'reference-and-object' => function () {
        $object = new stdClass();
        $object->a = 1;
        $object->b = &$object->a;
        return [$object, $object];
    },
    'enum' => fn() => DeserSuit::Hearts,
    'enum-list' => fn() => [DeserSuit::Hearts, DeserSuit::Spades, DeserSuit::Hearts],
    'array-object' => fn() => new ArrayObject([1, 2]),
    'spl-object-storage' => function () {
        $storage = new SplObjectStorage();
        $storage[new stdClass()] = 'data';
        return $storage;
    },
    'datetime' => fn() => new DateTimeImmutable('2024-01-02 03:04:05.678', new DateTimeZone('UTC')),
];

class DeserVisibility
{
    public $a = 1;
    protected $b = 2;
    private $c = 3;
}

class DeserVisibilityChild extends DeserVisibility
{
    private $c = 4;
}

enum DeserSuit
{
    case Hearts;
    case Spades;
}

return ['inputs' => $inputs, 'values' => $values];
