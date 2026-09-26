/// Constructs a [`Value`](crate::Value) from a JSON like literal.
///
/// ```
/// use deser_value::value;
///
/// let id = 42;
/// let value = value!({
///     "id": id,
///     "name": "Jane",
///     "tags": ["admin", null, 1.5],
///     "nested": {"deep": true},
///     // keys can be any value
///     1: "one",
///     (id + 1): "expression",
/// });
/// assert_eq!(value["nested"]["deep"], true);
/// assert_eq!(value[&value!(43)], "expression");
/// ```
///
/// Values and keys can be any expression that converts into a value, keys
/// can also be sequences and maps.  Keys which are expressions made of more
/// than one token (other than a leading minus) have to be put into
/// parentheses.
#[macro_export]
macro_rules! value {
    ($($value:tt)+) => {
        $crate::__value_internal!($($value)+)
    };
}

// This follows the implementation of `serde_json::json!`.
#[macro_export]
#[doc(hidden)]
macro_rules! __value_internal {
    // Sequences: `(@seq [$($elems,)*] $($rest)*)` munches the elements
    // into `$elems`.

    // Done with trailing comma.
    (@seq [$($elems:expr,)*]) => {
        ::std::vec![$($elems,)*]
    };

    // Done without trailing comma.
    (@seq [$($elems:expr),*]) => {
        ::std::vec![$($elems),*]
    };

    // Next element is `null`.
    (@seq [$($elems:expr,)*] null $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!(null)] $($rest)*)
    };

    // Next element is `true`.
    (@seq [$($elems:expr,)*] true $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!(true)] $($rest)*)
    };

    // Next element is `false`.
    (@seq [$($elems:expr,)*] false $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!(false)] $($rest)*)
    };

    // Next element is a sequence.
    (@seq [$($elems:expr,)*] [$($seq:tt)*] $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!([$($seq)*])] $($rest)*)
    };

    // Next element is a map.
    (@seq [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!({$($map)*})] $($rest)*)
    };

    // Next element is an expression followed by comma.
    (@seq [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!($next),] $($rest)*)
    };

    // Last element is an expression with no trailing comma.
    (@seq [$($elems:expr,)*] $last:expr) => {
        $crate::__value_internal!(@seq [$($elems,)* $crate::__value_internal!($last)])
    };

    // Comma after the most recent element.
    (@seq [$($elems:expr),*] , $($rest:tt)*) => {
        $crate::__value_internal!(@seq [$($elems,)*] $($rest)*)
    };

    // Unexpected token after most recent element.
    (@seq [$($elems:expr),*] $unexpected:tt $($rest:tt)*) => {
        $crate::__value_unexpected!($unexpected)
    };

    // Maps: `(@map $map ($($key)*) ($($rest)*) ($($rest)*))` munches the
    // tokens of the next key into `$key` and inserts the entries into the
    // map.  The rest is passed twice so that the first token of it can be
    // used in error messages.

    // Done.
    (@map $map:ident () () ()) => {};

    // Insert the current entry followed by trailing comma.
    (@map $map:ident [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        let _ = $map.insert(($($key)+), $value);
        $crate::__value_internal!(@map $map () ($($rest)*) ($($rest)*));
    };

    // Current entry followed by unexpected token.
    (@map $map:ident [$($key:tt)+] ($value:expr) $unexpected:tt $($rest:tt)*) => {
        $crate::__value_unexpected!($unexpected);
    };

    // Insert the last entry without trailing comma.
    (@map $map:ident [$($key:tt)+] ($value:expr)) => {
        let _ = $map.insert(($($key)+), $value);
    };

    // Next value is `null`.
    (@map $map:ident ($($key:tt)+) (: null $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!(null)) $($rest)*);
    };

    // Next value is `true`.
    (@map $map:ident ($($key:tt)+) (: true $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!(true)) $($rest)*);
    };

    // Next value is `false`.
    (@map $map:ident ($($key:tt)+) (: false $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!(false)) $($rest)*);
    };

    // Next value is a sequence.
    (@map $map:ident ($($key:tt)+) (: [$($seq:tt)*] $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!([$($seq)*])) $($rest)*);
    };

    // Next value is a map.
    (@map $map:ident ($($key:tt)+) (: {$($inner:tt)*} $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!({$($inner)*})) $($rest)*);
    };

    // Next value is an expression followed by comma.
    (@map $map:ident ($($key:tt)+) (: $value:expr , $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!($value)) , $($rest)*);
    };

    // Last value is an expression with no trailing comma.
    (@map $map:ident ($($key:tt)+) (: $value:expr) $copy:tt) => {
        $crate::__value_internal!(@map $map [$($key)+] ($crate::__value_internal!($value)));
    };

    // Missing value for last entry.  Trigger a reasonable error message.
    (@map $map:ident ($($key:tt)+) (:) $copy:tt) => {
        // "unexpected end of macro invocation"
        $crate::__value_internal!();
    };

    // Missing colon and value for last entry.  Trigger a reasonable error
    // message.
    (@map $map:ident ($($key:tt)+) () $copy:tt) => {
        // "unexpected end of macro invocation"
        $crate::__value_internal!();
    };

    // Misplaced colon.  Trigger a reasonable error message.
    (@map $map:ident () (: $($rest:tt)*) ($colon:tt $($copy:tt)*)) => {
        // Takes no arguments so "no rules expected the token `:`".
        $crate::__value_unexpected!($colon);
    };

    // Found a comma inside a key.  Trigger a reasonable error message.
    (@map $map:ident ($($key:tt)*) (, $($rest:tt)*) ($comma:tt $($copy:tt)*)) => {
        // Takes no arguments so "no rules expected the token `,`".
        $crate::__value_unexpected!($comma);
    };

    // Key is fully parenthesized.  This avoids clippy double_parens false
    // positives because the parenthesization may be necessary here.
    (@map $map:ident () (($key:expr) : $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map ($key) (: $($rest)*) (: $($rest)*));
    };

    // Key is a sequence.
    (@map $map:ident () ([$($key:tt)*] : $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map ($crate::__value_internal!([$($key)*])) (: $($rest)*) (: $($rest)*));
    };

    // Key is a map.
    (@map $map:ident () ({$($key:tt)*} : $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map ($crate::__value_internal!({$($key)*})) (: $($rest)*) (: $($rest)*));
    };

    // Key is `null`.
    (@map $map:ident () (null : $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map ($crate::__value_internal!(null)) (: $($rest)*) (: $($rest)*));
    };

    // Refuse to absorb colon token into key expression.
    (@map $map:ident ($($key:tt)*) (: $($unexpected:tt)+) $copy:tt) => {
        $crate::__value_expect_expr_comma!($($unexpected)+);
    };

    // Munch a token into the current key.
    (@map $map:ident ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        $crate::__value_internal!(@map $map ($($key)* $tt) ($($rest)*) ($($rest)*));
    };

    // The main implementation.

    (null) => {
        $crate::Value::null()
    };

    (true) => {
        $crate::Value::from(true)
    };

    (false) => {
        $crate::Value::from(false)
    };

    ([]) => {
        $crate::Value::from($crate::Seq::new())
    };

    ([ $($tt:tt)+ ]) => {
        $crate::Value::from($crate::__value_internal!(@seq [] $($tt)+))
    };

    ({}) => {
        $crate::Value::from($crate::Map::new())
    };

    ({ $($tt:tt)+ }) => {
        $crate::Value::from({
            let mut map = $crate::Map::new();
            $crate::__value_internal!(@map map () ($($tt)+) ($($tt)+));
            map
        })
    };

    // Any value that converts into a value.  Must be below every other rule.
    ($other:expr) => {
        $crate::Value::from($other)
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __value_unexpected {
    () => {};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __value_expect_expr_comma {
    ($e:expr , $($tt:tt)*) => {};
}
