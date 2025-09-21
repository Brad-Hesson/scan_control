use egui::{
    ahash::{HashSet, HashSetExt as _},
    Context, Response,
};

pub struct ResponseGroup {
    ids: HashSet<egui::Id>,
}
impl ResponseGroup {
    pub fn new() -> Self {
        Self {
            ids: HashSet::new(),
        }
    }
    pub fn wrap(&mut self, response: Response) -> Response {
        let ctx = &response.ctx;
        self.ids.insert(response.id);
        self.response(ctx).unwrap()
    }
    pub fn response(&self, ctx: &Context) -> Option<Response> {
        self.ids
            .iter()
            .filter_map(|id| ctx.read_response(*id))
            .reduce(|a, b| a.union(b))
    }
}
pub trait ResponseGroupExt {
    fn synchronize(self, group: &mut ResponseGroup) -> SyncResponse;
}
impl ResponseGroupExt for Response {
    fn synchronize(self, group: &mut ResponseGroup) -> SyncResponse {
        SyncResponse {
            orig: self.clone(),
            sync: group.wrap(self),
        }
    }
}

pub struct SyncResponse {
    pub orig: Response,
    pub sync: Response,
}
