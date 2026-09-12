use crate::stratagems::{self, StratagemRef};
use crate::H2ACApp;

impl H2ACApp {
    pub fn lib_categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self.library.categories.clone();
        for p in &self.plugins.stratagems {
            if !cats.contains(&p.category) {
                cats.push(p.category.clone());
            }
        }
        for cat in self.model.config.category_overrides.values() {
            if !cats.contains(cat) {
                cats.push(cat.clone());
            }
        }
        cats
    }

    pub fn lib_by_category(&self, cat: &str) -> Vec<StratagemRef<'_>> {
        let mut out: Vec<StratagemRef> = stratagems::get_by_category(cat)
            .into_iter()
            .map(StratagemRef::Base)
            .collect();
        for p in &self.plugins.stratagems {
            if p.category == cat {
                out.push(StratagemRef::Plugin(p));
            }
        }
        out
    }

    pub fn lib_search(&self, query: &str) -> Vec<StratagemRef<'_>> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<StratagemRef> = stratagems::search(q)
            .into_iter()
            .map(StratagemRef::Base)
            .collect();
        for p in &self.plugins.stratagems {
            if p.name.contains(q) || p.model.contains(q) {
                out.push(StratagemRef::Plugin(p));
            }
        }
        out
    }
}
