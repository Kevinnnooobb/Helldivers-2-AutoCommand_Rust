use crate::H2ACApp;
use crate::wiki_fetcher;

impl H2ACApp {
    pub fn start_wiki_fetch(&mut self) {
        let (rx, has_cache) = wiki_fetcher::start_fetch();
        self.wiki.fetch_rx = Some(rx);
        self.wiki.cache_exists = has_cache;
        self.wiki.fetch_status = "连接中…".into();
    }
}
