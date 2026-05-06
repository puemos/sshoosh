use std::ops::Range;

use crate::{
    app::Action,
    features::{accounts::model::Role, messages::model::Snapshot},
};

use super::state::{AutocompleteItem, AutocompleteState, UiMode, fuzzy_score};

mod args;
mod autocomplete;
mod parse;
mod registry;
mod specs;
mod tests;

pub(crate) use self::{args::*, autocomplete::*, parse::*, registry::*, specs::*};
