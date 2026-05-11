use automerge::{transaction::Transactable, AutoCommit, ObjType, ReadDoc};

pub type CID = [u8; 32];

pub struct CollabDoc {
    pub change_count: u64,
    doc: AutoCommit,
    pub cid: CID,
}

impl CollabDoc {
    pub fn new(content: &str) -> Self {
        let mut doc = AutoCommit::new();
        let text = doc.put_object(automerge::ROOT, "content", ObjType::Text).expect("content");
        doc.splice_text(&text, 0, 0, content).expect("splice");
        let bytes = doc.save();
        let cid = *blake3::hash(&bytes).as_bytes();
        Self { doc, cid, change_count: 0 }
    }

    pub fn merge(&mut self, changes: &[u8]) -> Result<(), automerge::AutomergeError> {
        let mut other = AutoCommit::load(changes)?;
        self.doc.merge(&mut other)?;
        self.change_count += 1;
        self.refresh_cid();
        Ok(())
    }

    pub fn content(&self) -> String {
        let (_, obj) = self.doc.get(automerge::ROOT, "content").ok().flatten().unwrap();
        self.doc.text(&obj).unwrap_or_default()
    }

    pub fn insert(&mut self, pos: usize, text: &str) {
        if let Some((_, obj)) = self.doc.get(automerge::ROOT, "content").ok().flatten() {
            let _ = self.doc.splice_text(&obj, pos, 0, text);
            self.change_count += 1;
            self.refresh_cid();
        }
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    fn refresh_cid(&mut self) {
        self.cid = *blake3::hash(&self.doc.save()).as_bytes();
    }
}


impl CollabDoc {
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, automerge::AutomergeError> {
        let doc = AutoCommit::load(bytes)?;
        let cid = *blake3::hash(bytes).as_bytes();
        Ok(Self { doc, cid, change_count: 0 })
    }

    pub fn snapshot_bytes(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn should_compact(&self, threshold: u64) -> bool {
        self.change_count >= threshold
    }
}
