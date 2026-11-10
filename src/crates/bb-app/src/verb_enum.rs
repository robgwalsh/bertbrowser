//! A private verb enum for the string-dispatched surfaces (`tools`, `ops_do`, ...): the wire strings
//! stay exactly as they are, written once beside the variant that handles them.

/// `verb_enum! { Name { Variant = "wire.string", ... } }` makes `enum Name` with a `FromStr` whose
/// error is `()`: the caller words the unknown-verb error itself, as it always has.
macro_rules! verb_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum $name { $($variant),+ }

        impl std::str::FromStr for $name {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, ()> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(()),
                }
            }
        }
    };
}
