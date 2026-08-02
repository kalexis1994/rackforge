use crate::Input;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorMode {
    #[default]
    Navigate,
    Edit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditorCommand {
    Ignored,
    BeginEdit,
    CommitEdit,
    CancelEdit,
    ExitEditor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EditorState {
    mode: EditorMode,
}

impl EditorState {
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn handle(&mut self, input: Input) -> EditorCommand {
        match (self.mode, input) {
            (EditorMode::Navigate, Input::Button1 | Input::EncoderPress) => {
                self.mode = EditorMode::Edit;
                EditorCommand::BeginEdit
            }
            (EditorMode::Edit, Input::Button1 | Input::EncoderPress) => {
                self.mode = EditorMode::Navigate;
                EditorCommand::CommitEdit
            }
            (EditorMode::Edit, Input::Button4) => {
                self.mode = EditorMode::Navigate;
                EditorCommand::CancelEdit
            }
            (EditorMode::Navigate, Input::Button4) => EditorCommand::ExitEditor,
            _ => EditorCommand::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_begins_and_commits_editing() {
        let mut editor = EditorState::default();
        assert_eq!(editor.handle(Input::Button1), EditorCommand::BeginEdit);
        assert_eq!(editor.mode(), EditorMode::Edit);
        assert_eq!(editor.handle(Input::Button1), EditorCommand::CommitEdit);
        assert_eq!(editor.mode(), EditorMode::Navigate);
    }

    #[test]
    fn encoder_press_has_the_same_commit_semantics_as_ok() {
        let mut editor = EditorState::default();
        assert_eq!(editor.handle(Input::EncoderPress), EditorCommand::BeginEdit);
        assert_eq!(
            editor.handle(Input::EncoderPress),
            EditorCommand::CommitEdit
        );
    }

    #[test]
    fn back_cancels_edit_before_exiting_editor() {
        let mut editor = EditorState::default();
        editor.handle(Input::Button1);
        assert_eq!(editor.handle(Input::Button4), EditorCommand::CancelEdit);
        assert_eq!(editor.mode(), EditorMode::Navigate);
        assert_eq!(editor.handle(Input::Button4), EditorCommand::ExitEditor);
    }
}
