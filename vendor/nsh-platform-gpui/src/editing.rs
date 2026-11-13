#![allow(missing_docs)]

use std::rc::Rc;

#[derive(Clone)]
pub struct InputIdentity {
    pub surface: u64,
    pub generation: u64,
    pub node: u32,
    pub focus_epoch: u64,
    pub(crate) mount: Rc<()>,
}

impl InputIdentity {
    pub fn same_mount(&self, other: &Self) -> bool {
        self.surface == other.surface
            && self.generation == other.generation
            && self.node == other.node
            && Rc::ptr_eq(&self.mount, &other.mount)
    }

    pub fn same_focus(&self, other: &Self) -> bool {
        self.same_mount(other) && self.focus_epoch == other.focus_epoch
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputMutation {
    pub range_utf16: (u32, u32),
    pub inserted: String,
}

#[derive(Clone)]
pub struct InputEdit {
    pub identity: InputIdentity,
    pub text: String,
    pub anchor_utf16: u32,
    pub head_utf16: u32,
    pub composing: bool,
    pub native_edit_serial: u64,
    pub mutation: Option<InputMutation>,
}

#[derive(Clone, Copy, Debug)]
pub struct InputKeyContext {
    pub native_edit_serial: u64,
    pub first_visual_row: bool,
    pub viewport_top: bool,
    pub selection_collapsed: bool,
}

pub enum PasteDecision {
    Reject,
    Insert(String),
}

#[derive(Clone)]
pub enum InputPastePayload {
    Native(NativeClipboardPaste),
    Synthetic(nsh_core::PasteEvent),
}

#[derive(Clone)]
pub struct NativeClipboardPaste {
    original: Option<gpui::ClipboardItem>,
    text: String,
    uris: Vec<String>,
}

#[derive(Clone, Copy)]
pub enum InputPasteFile<'a> {
    Native(&'a gpui::Image),
    Synthetic(&'a nsh_core::PastedFile),
}

impl InputPasteFile<'_> {
    pub fn name(&self) -> &str {
        match self {
            Self::Synthetic(file) => &file.name,
            Self::Native(_) => match self.mime() {
                "image/jpeg" => "image.jpg",
                "image/webp" => "image.webp",
                "image/gif" => "image.gif",
                "image/bmp" => "image.bmp",
                "image/tiff" => "image.tiff",
                _ => "image.png",
            },
        }
    }

    pub fn mime(&self) -> &str {
        match self {
            Self::Synthetic(file) => &file.mime,
            Self::Native(image) => {
                nsh_core::platform::clipboard::image_mime(&image.bytes).unwrap_or("image/png")
            }
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Native(image) => &image.bytes,
            Self::Synthetic(file) => &file.data,
        }
    }

    pub fn capacity(&self) -> usize {
        match self {
            Self::Native(image) => image.bytes.capacity(),
            Self::Synthetic(file) => file.data.capacity(),
        }
    }
}

impl InputPastePayload {
    pub fn native(mut original: Option<gpui::ClipboardItem>) -> Self {
        if let Some(item) = &mut original {
            for entry in &mut item.entries {
                if let gpui::ClipboardEntry::Image(image) = entry
                    && image.bytes.capacity() != image.bytes.len()
                {
                    image.bytes = std::mem::take(&mut image.bytes)
                        .into_boxed_slice()
                        .into_vec();
                }
            }
        }
        let text = original
            .as_ref()
            .and_then(|item| item.text())
            .unwrap_or_default();
        let uris = original
            .iter()
            .flat_map(|item| item.entries())
            .flat_map(|entry| match entry {
                gpui::ClipboardEntry::ExternalPaths(paths) => paths.paths(),
                _ => &[],
            })
            .filter(|path| path.is_absolute())
            .map(|path| nsh_core::platform::clipboard::file_url(&path.to_string_lossy()))
            .collect();
        Self::Native(NativeClipboardPaste {
            original,
            text,
            uris,
        })
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Native(native) => &native.text,
            Self::Synthetic(paste) => &paste.text,
        }
    }

    pub fn html(&self) -> &str {
        match self {
            Self::Native(_) => "",
            Self::Synthetic(paste) => &paste.html,
        }
    }

    pub fn uris(&self) -> &[String] {
        match self {
            Self::Native(native) => &native.uris,
            Self::Synthetic(paste) => &paste.uris,
        }
    }

    pub fn original(&self) -> Option<&gpui::ClipboardItem> {
        match self {
            Self::Native(native) => native.original.as_ref(),
            Self::Synthetic(_) => None,
        }
    }

    pub fn take_original(&mut self) -> Option<gpui::ClipboardItem> {
        match self {
            Self::Native(native) => native.original.take(),
            Self::Synthetic(_) => None,
        }
    }

    pub fn files(&self) -> impl Iterator<Item = InputPasteFile<'_>> + Clone {
        let native = self
            .original()
            .into_iter()
            .flat_map(|item| item.entries())
            .filter_map(|entry| match entry {
                gpui::ClipboardEntry::Image(image) => Some(InputPasteFile::Native(image)),
                _ => None,
            });
        let synthetic = match self {
            Self::Synthetic(paste) => paste.files.as_slice(),
            Self::Native(_) => &[],
        };
        native.chain(synthetic.iter().map(InputPasteFile::Synthetic))
    }

    pub fn is_empty(&self) -> bool {
        self.text().is_empty()
            && self.html().is_empty()
            && self.uris().is_empty()
            && self.files().next().is_none()
    }

    pub fn normalized_storage(&self) -> Option<usize> {
        let (text, html, uris, files) = match self {
            Self::Native(native) => (&native.text, None, &native.uris, None),
            Self::Synthetic(paste) => (
                &paste.text,
                Some(&paste.html),
                &paste.uris,
                Some(&paste.files),
            ),
        };
        let mut bytes = std::mem::size_of::<Self>()
            .checked_add(std::mem::size_of::<usize>().checked_mul(2)?)?
            .checked_add(text.capacity())?
            .checked_add(html.map_or(0, String::capacity))?
            .checked_add(uris.capacity().checked_mul(std::mem::size_of::<String>())?)?;
        for uri in uris {
            bytes = bytes.checked_add(uri.capacity())?;
        }
        if let Some(files) = files {
            bytes = bytes.checked_add(
                files
                    .capacity()
                    .checked_mul(std::mem::size_of::<nsh_core::PastedFile>())?,
            )?;
            for file in files {
                bytes = bytes
                    .checked_add(file.name.capacity())?
                    .checked_add(file.mime.capacity())?;
            }
        }
        Some(bytes)
    }
}

pub struct InputPaste<'a> {
    pub before: &'a InputEdit,
    pub payload: &'a Rc<InputPastePayload>,
}

pub struct InputPasteCompletion {
    pub edit: InputEdit,
    pub payload: Rc<InputPastePayload>,
}

#[derive(Clone, Copy, Debug)]
pub struct InputKey {
    pub virtual_key: u16,
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
    pub down: bool,
    pub repeat: bool,
    pub context: Option<InputKeyContext>,
}

pub type InputKeyPolicy = Rc<dyn Fn(InputKey, InputIdentity) -> bool>;
pub type InputEditObserver = Rc<dyn Fn(InputEdit)>;
pub type InputFocusObserver = Rc<dyn Fn(InputIdentity, bool)>;

#[derive(Clone)]
pub struct InputHooks {
    pub key: InputKeyPolicy,
    pub edit: InputEditObserver,
    pub focus: InputFocusObserver,
    pub paste_begin: Rc<dyn Fn(InputEdit) -> bool>,
    pub paste: Rc<dyn Fn(InputPaste<'_>) -> PasteDecision>,
    pub paste_complete: Rc<dyn Fn(Option<InputPasteCompletion>)>,
}
