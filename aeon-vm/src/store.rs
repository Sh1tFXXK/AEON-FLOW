use crate::program::Program;
use crate::ProgramId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

pub struct ProgramStore {
    programs: RwLock<HashMap<ProgramId, Arc<Program>>>,
}

impl ProgramStore {
    pub fn new() -> Self {
        ProgramStore {
            programs: RwLock::new(HashMap::new()),
        }
    }

    pub fn add(&self, program: Program) -> ProgramId {
        let id = program.id();
        let mut programs = self.programs.write().unwrap();
        programs.insert(id, Arc::new(program));
        id
    }

    pub fn get(&self, id: &ProgramId) -> Option<Arc<Program>> {
        let programs = self.programs.read().unwrap();
        programs.get(id).cloned()
    }

    pub fn has(&self, id: &ProgramId) -> bool {
        let programs = self.programs.read().unwrap();
        programs.contains_key(id)
    }

    pub fn load_from_file(&self, path: &Path) -> std::io::Result<ProgramId> {
        let program = Program::load(path)?;
        Ok(self.add(program))
    }

    // Compatibility wrapper expected by CLI
    pub fn load_file(&self, path: &Path) -> std::io::Result<ProgramId> {
        self.load_from_file(path)
    }

    pub fn count(&self) -> usize {
        let programs = self.programs.read().unwrap();
        programs.len()
    }
}

impl Default for ProgramStore {
    fn default() -> Self {
        Self::new()
    }
}
