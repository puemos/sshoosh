use std::ops::Range;

use crate::{
    app::Action,
    service::{Role, Snapshot},
};

use super::state::{AutocompleteItem, AutocompleteState, UiMode, fuzzy_score};

include!("commands/registry.rs");
include!("commands/specs.rs");
include!("commands/parse.rs");
include!("commands/autocomplete.rs");
include!("commands/args.rs");
include!("commands/tests.rs");
