use crate::domain::Snapshot;

#[derive(Clone, Debug, Default)]
pub struct WorkspaceViewModel {
    pub snapshot: Snapshot,
}
