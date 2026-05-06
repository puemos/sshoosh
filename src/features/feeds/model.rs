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
pub struct PageRequest {
    pub limit: i64,
    pub cursor: Option<String>,
}

impl PageRequest {
    pub fn first(limit: i64) -> Self {
        Self {
            limit,
            cursor: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    pub next_cursor: Option<String>,
}

impl SearchPage {
    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }
}
