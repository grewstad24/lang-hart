use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_hart() -> *const ();
}

pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_hart) };

pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

