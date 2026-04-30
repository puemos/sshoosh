#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchKind {
    Thread,
    Comment,
    Dm,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub kind: SearchKind,
    pub label: String,
    pub context: String,
    pub snippet: String,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    pub has_more: bool,
}
