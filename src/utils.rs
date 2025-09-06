pub trait SelectableMember: Eq {
    fn set_selected(&mut self, selected: bool);

    fn is_selected(&self) -> bool;
}

pub trait SelectableVecExt {
    type Member: SelectableMember;
    fn set_selected_idx(&mut self, index: Option<usize>);
    fn set_selected(&mut self, item: Option<&Self::Member>);
    fn get_selected_index(&self) -> Option<usize>;
    fn get_selected(&self) -> Option<&Self::Member>;
    fn get_selected_mut(&mut self) -> Option<&mut Self::Member>;
}

impl<T: SelectableMember> SelectableVecExt for Vec<T> {
    type Member = T;

    fn set_selected_idx(&mut self, index: Option<usize>) {
        for (i, item) in self.iter_mut().enumerate() {
            item.set_selected(Some(i) == index);
        }
    }

    fn set_selected(&mut self, item: Option<&Self::Member>) {
        for it in self.iter_mut() {
            it.set_selected(Some(&*it) == item);
        }
    }

    fn get_selected_index(&self) -> Option<usize> {
        self.iter()
            .enumerate()
            .find_map(|(i, item)| item.is_selected().then_some(i))
    }

    fn get_selected(&self) -> Option<&Self::Member> {
        self.iter()
            .find_map(|item| item.is_selected().then_some(item))
    }

    fn get_selected_mut(&mut self) -> Option<&mut Self::Member> {
        self.iter_mut()
            .find_map(|item| item.is_selected().then_some(item))
    }
}
