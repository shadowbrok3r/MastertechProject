

pub trait FileEditorAction {
    fn save_file(&mut self);
    fn open_file(&mut self);
    // fn save_file(&mut self);
}