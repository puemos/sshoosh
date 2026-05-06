use crate::features::messages::model::Snapshot;

#[derive(Clone, Debug, Default)]
pub struct WorkspaceViewModel {
    pub snapshot: Snapshot,
}
