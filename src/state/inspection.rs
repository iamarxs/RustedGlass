use super::*;

pub(super) struct Inspection {
    revision: u64,
    path: PathBuf,
    text: Option<String>,
}

impl State {
    /// Queue Image Info without opening the image from the window procedure.
    pub fn request_image_info(&mut self) -> bool {
        let Some(path) = self.photos.current().filter(|_| !self.navigation_pending) else {
            return false;
        };
        let revision = self.slide_revision;
        if self
            .inspection
            .as_ref()
            .is_some_and(|request| request.revision == revision && request.path == *path)
        {
            return false;
        }
        let queued = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.inspect(revision, path.clone()).is_ok());
        if queued {
            self.inspection = Some(Inspection {
                revision,
                path: path.clone(),
                text: None,
            });
            return false;
        }
        self.notice = Some("Could not queue image information. Try again in a moment.".into());
        true
    }

    pub(super) fn finish_image_info(&mut self, revision: u64, path: &Path, text: String) -> bool {
        let Some(request) = self
            .inspection
            .as_mut()
            .filter(|request| request.revision == revision && request.path == path)
        else {
            return false;
        };
        request.text = Some(text);
        true
    }

    pub fn take_image_info(&mut self) -> Option<String> {
        self.inspection.as_ref()?.text.as_ref()?;
        let request = self.inspection.take()?;
        if self.navigation_pending
            || self.dialog != Dialog::None
            || self.slide_revision != request.revision
            || self.photos.current() != Some(&request.path)
        {
            return None;
        }
        request.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_info_is_delivered_only_for_its_current_selection() {
        let mut state = State::new(crate::test_path("unused-config.json"), &[]);
        let path = PathBuf::from("first.png");
        state.photos.files = vec![path.clone(), PathBuf::from("second.png")];
        for (index, dialog, expected) in [
            (1, Dialog::None, None),
            (0, Dialog::Settings, None),
            (0, Dialog::None, Some("details".to_owned())),
        ] {
            state.inspection = Some(Inspection {
                revision: state.slide_revision,
                path: path.clone(),
                text: None,
            });
            assert!(state.finish_image_info(state.slide_revision, &path, "details".into()));
            state.photos.index = index;
            state.dialog = dialog;
            assert_eq!(state.take_image_info(), expected);
            assert!(state.inspection.is_none());
        }
    }
}
