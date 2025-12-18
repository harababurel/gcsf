//! GCSF -- A VIRTUAL FILE SYSTEM BASED ON GOOGLE DRIVE
#![deny(
    ambiguous_glob_reexports,
    anonymous_parameters,
    array_into_iter,
    asm_sub_register,
    bad_asm_style,
    bare_trait_objects,
    break_with_label_and_loop,
    clashing_extern_declarations,
    coherence_leak_check,
    confusable_idents,
    const_evaluatable_unchecked,
    const_item_mutation,
    dead_code,
    deprecated,
    deprecated_where_clause_location,
    deref_into_dyn_supertrait,
    deref_nullptr,
    drop_bounds,
    dropping_copy_types,
    dropping_references,
    duplicate_macro_attributes,
    dyn_drop,
    ellipsis_inclusive_range_patterns,
    exported_private_dependencies,
    for_loops_over_fallibles,
    forbidden_lint_groups,
    forgetting_copy_types,
    forgetting_references,
    function_item_references,
    improper_ctypes,
    improper_ctypes_definitions,
    incomplete_features,
    inline_no_sanitize,
    invalid_doc_attributes,
    invalid_macro_export_arguments,
    invalid_value,
    irrefutable_let_patterns,
    large_assignments,
    late_bound_lifetime_arguments,
    legacy_derive_helpers,
    map_unit_fn,
    missing_docs,
    mixed_script_confusables,
    named_arguments_used_positionally,
    no_mangle_generic_items,
    non_camel_case_types,
    non_fmt_panics,
    non_shorthand_field_patterns,
    non_snake_case,
    non_upper_case_globals,
    opaque_hidden_inferred_bound,
    overlapping_range_endpoints,
    path_statements,
    redundant_semicolons,
    renamed_and_removed_lints,
    repr_transparent_external_private_fields,
    semicolon_in_expressions_from_macros,
    special_module_name,
    stable_features,
    suspicious_double_ref_op,
    dangling_pointers_from_temporaries,
    trivial_bounds,
    trivial_casts,
    trivial_numeric_casts,
    type_alias_bounds,
    tyvar_behind_raw_pointer,
    uncommon_codepoints,
    unconditional_recursion,
    unexpected_cfgs,
    ungated_async_fn_track_caller,
    uninhabited_static,
    unknown_lints,
    unnameable_test_items,
    unreachable_code,
    unreachable_patterns,
    unsafe_code,
    unstable_features,
    unstable_name_collisions,
    unstable_syntax_pre_expansion,
    unsupported_calling_conventions,
    unused_allocation,
    unused_assignments,
    unused_attributes,
    unused_braces,
    unused_braces,
    unused_comparisons,
    unused_doc_comments,
    unused_features,
    unused_features,
    unused_import_braces,
    unused_imports,
    unused_imports,
    unused_labels,
    unused_labels,
    unused_macros,
    unused_macros,
    unused_must_use,
    unused_mut,
    unused_mut,
    unused_parens,
    unused_parens,
    unused_qualifications,
    unused_unsafe,
    unused_unsafe,
    unused_variables,
    warnings,
    while_true
)]
extern crate failure;
extern crate fuser;
extern crate google_drive3 as drive3;
extern crate id_tree;
extern crate libc;
extern crate mime_sniffer;
#[macro_use]
extern crate log;
#[macro_use]
extern crate maplit;
extern crate lru_time_cache;
extern crate pretty_env_logger;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate time;
#[macro_use]
extern crate lazy_static;

mod gcsf;

pub use crate::gcsf::filesystem::{Gcsf, NullFs};
pub use crate::gcsf::{Config, DriveFacade, FileManager};

#[cfg(test)]
mod tests;
