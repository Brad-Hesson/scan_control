use std::collections::HashMap;

use egui::Ui;
use uuid::Uuid;

use crate::utils::response_group::SyncResponse;



#[derive(borrow::Partial)]
#[module(crate::components::tree)]
pub struct Tree<T> {
    nodes: HashMap<NodeId, Node<T>>,
    edges: Edges,
}
impl<T> Tree<T> {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Edges::new(),
        }
    }
    pub fn insert(&mut self, node: Node<T>, parent: Option<NodeId>) -> Result<(), InsertError> {
        let id = NodeId::new();
        match parent {
            Some(parent_id) => {
                if !self.nodes.contains_key(&parent_id) {
                    return Err(InsertError::ParentMissing);
                }
                self.edges.insert_edge(parent_id, id);
            }
            None => {
                if !self.nodes.is_empty() {
                    return Err(InsertError::RootOccupied);
                }
            }
        }
        self.nodes.insert(id, node);
        Ok(())
    }
    pub fn remove(&mut self, id: NodeId) {
        if self.nodes.remove(&id).is_none() {
            return;
        };
        if let Some(parent) = self.edges.parent_of(id) {
            self.edges.remove_edge(parent, id);
        }
        let children = self.edges.children_of(id).to_vec();
        for child in children {
            self.remove(child);
        }
    }
    pub fn move_to(&mut self, id: NodeId, parent: NodeId) {
        if !self.contains(id) {
            panic!("node does not exist")
        }
        if !self.contains(parent) {
            panic!("parent does not exist")
        }
        let Some(old_parent) = self.edges.parent_of(id) else {
            panic!("cannot move root node")
        };
        self.edges.remove_edge(old_parent, id);
        self.edges.insert_edge(parent, id);
    }
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }
}

pub struct Node<T> {
    data: T,
}
impl<T> Node<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq)]
pub struct NodeId {
    id: Uuid,
}
impl NodeId {
    fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InsertError {
    #[error("tried to insert root, but a root already exists")]
    RootOccupied,
    #[error("requested parent does not exist")]
    ParentMissing,
}

pub struct Edges {
    parents: HashMap<NodeId, NodeId>,
    children: HashMap<NodeId, Vec<NodeId>>,
}
impl Edges {
    fn new() -> Self {
        Self {
            parents: HashMap::new(),
            children: HashMap::new(),
        }
    }
    fn insert_edge(&mut self, parent: NodeId, child: NodeId) {
        self.children.entry(parent).or_default().push(child);
        self.parents.insert(child, parent);
    }
    fn remove_edge(&mut self, parent: NodeId, child: NodeId) {
        if let Some(children) = self.children.get_mut(&parent) {
            children.retain(|id| *id != child);
        }
        self.parents.remove(&child);
    }
    fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.parents.get(&id).copied()
    }
    fn children_of(&self, id: NodeId) -> &[NodeId] {
        self.children
            .get(&id)
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }
}
