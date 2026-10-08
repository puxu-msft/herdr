use std::{borrow::Cow, io};

const MAX_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;

fn windows_line_endings(text: &str) -> Cow<'_, str> {
    if !text.contains(['\r', '\n']) {
        return Cow::Borrowed(text);
    }
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.push_str("\r\n");
            }
            '\n' => result.push_str("\r\n"),
            other => result.push(other),
        }
    }
    Cow::Owned(result)
}

fn encoded_text(bytes: &[u8]) -> io::Result<Vec<u16>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if text.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "clipboard text contains a NUL character",
        ));
    }
    Ok(windows_line_endings(text)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect())
}

fn decoded_text(bytes: &[u8]) -> io::Result<String> {
    if bytes.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard text has an invalid UTF-16 buffer length",
        ));
    }
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let end = units.iter().position(|&unit| unit == 0).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard text is not NUL-terminated",
        )
    })?;
    let text = String::from_utf16(&units[..end])
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard text exceeds the 1 MiB text limit",
        ));
    }
    Ok(text)
}

fn text_equals(current: &str, bytes: &[u8]) -> bool {
    let Ok(payload) = std::str::from_utf8(bytes) else {
        return false;
    };
    windows_line_endings(payload) == windows_line_endings(current)
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        mem::size_of,
        ptr::{copy_nonoverlapping, null_mut},
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HANDLE, HWND},
        System::{
            Console::GetConsoleWindow,
            DataExchange::{
                CountClipboardFormats, EmptyClipboard, EnumClipboardFormats, GetClipboardOwner,
                GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
                SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            Ole::{CF_LOCALE, CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT},
        },
    };

    use super::super::{clipboard_global_bytes, ClipboardGuard};

    static LAST_WRITE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

    fn open(owner: HWND) -> io::Result<ClipboardGuard> {
        let mut attempt = 0;
        loop {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(ClipboardGuard);
            }
            let err = io::Error::last_os_error();
            if attempt == 9 {
                return Err(err);
            }
            attempt += 1;
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    struct TextMemory(HANDLE);

    impl TextMemory {
        fn new(units: &[u16]) -> io::Result<Self> {
            let size = units.len().checked_mul(size_of::<u16>()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "clipboard text length overflow",
                )
            })?;
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let memory = Self(handle);
            let data = unsafe { GlobalLock(handle) };
            if data.is_null() {
                return Err(io::Error::last_os_error());
            }
            unsafe {
                copy_nonoverlapping(units.as_ptr(), data.cast::<u16>(), units.len());
                GlobalUnlock(handle);
            }
            Ok(memory)
        }
    }

    impl Drop for TextMemory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { GlobalFree(self.0) };
            }
        }
    }

    fn write_with_owner(bytes: &[u8], owner: HWND) -> io::Result<()> {
        let units = encoded_text(bytes)?;
        // Prepare the transferable memory before discarding the old clipboard.
        let mut memory = TextMemory::new(&units)?;
        if owner.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "clipboard writing requires a console window",
            ));
        }
        let clipboard = open(owner)?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(io::Error::last_os_error());
        }
        LAST_WRITE_SEQUENCE.store(0, Ordering::Relaxed);
        if unsafe { SetClipboardData(CF_UNICODETEXT as u32, memory.0) }.is_null() {
            return Err(io::Error::last_os_error());
        }
        // Windows owns the allocation after a successful SetClipboardData.
        memory.0 = null_mut();
        drop(clipboard);
        let sequence = unsafe { GetClipboardSequenceNumber() };
        let sequence = if unsafe { GetClipboardOwner() } == owner {
            sequence
        } else {
            0
        };
        LAST_WRITE_SEQUENCE.store(sequence, Ordering::Relaxed);
        Ok(())
    }

    pub fn write_clipboard(bytes: &[u8]) -> bool {
        match write_with_owner(bytes, unsafe { GetConsoleWindow() }) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, "failed to write Windows text clipboard");
                false
            }
        }
    }

    fn read_locked() -> io::Result<Option<String>> {
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT as u32) } == 0 {
            // Image-only and empty clipboards have no text to paste.
            return Ok(None);
        }
        // UTF-16 can use twice the UTF-8 text limit; allow allocator padding too.
        let bytes = clipboard_global_bytes(
            CF_UNICODETEXT as u32,
            (MAX_CLIPBOARD_TEXT_BYTES + 1) * 2 + 64 * 1024,
        )
        .ok_or_else(|| io::Error::other("Unicode clipboard data is unreadable or oversized"))?;
        decoded_text(&bytes).map(Some)
    }

    pub fn read_clipboard_text() -> Option<String> {
        let result = (|| {
            let _clipboard = open(null_mut())?;
            read_locked()
        })();
        match result {
            Ok(text) => text.filter(|text| !text.is_empty()),
            Err(err) => {
                tracing::warn!(%err, "failed to read Windows text clipboard");
                None
            }
        }
    }

    fn plain_format(format: u32) -> bool {
        [CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT, CF_LOCALE]
            .iter()
            .any(|&candidate| u32::from(candidate) == format)
    }

    fn read_own_plain_text() -> io::Result<Option<String>> {
        let _clipboard = open(null_mut())?;
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence == 0 || sequence != LAST_WRITE_SEQUENCE.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let count = unsafe { CountClipboardFormats() };
        if count == 0 {
            return Ok(None);
        }
        let mut format = 0;
        for _ in 0..count {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                return Err(io::Error::last_os_error());
            }
            if !plain_format(format) {
                return Ok(None);
            }
        }
        read_locked()
    }

    pub fn clipboard_text_matches(bytes: &[u8]) -> Option<bool> {
        match read_own_plain_text() {
            Ok(text) => text.map(|current| text_equals(&current, bytes)),
            Err(err) => {
                tracing::debug!(%err, "Windows clipboard duplicate check unavailable");
                None
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn duplicate_check_rejects_rich_content_formats() {
            for format in [CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT, CF_LOCALE] {
                assert!(plain_format(u32::from(format)));
            }
            assert!(!plain_format(8));
            assert!(!plain_format(0xC000));
        }

        #[test]
        #[ignore = "requires a disposable Windows clipboard; run only in an isolated test environment"]
        fn windows_native_text_clipboard_roundtrip() {
            use windows_sys::Win32::{
                System::LibraryLoader::GetModuleHandleW,
                UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
            };
            struct Owner {
                window: HWND,
                sequence: Option<u32>,
            }
            impl Drop for Owner {
                fn drop(&mut self) {
                    if let Some(sequence) = self.sequence {
                        match open(self.window) {
                            Ok(_clipboard) => {
                                if unsafe { GetClipboardOwner() } == self.window
                                    && unsafe { GetClipboardSequenceNumber() } == sequence
                                    && unsafe { EmptyClipboard() } == 0
                                {
                                    eprintln!("failed to clear the owned test clipboard");
                                }
                            }
                            Err(err) => eprintln!("test clipboard cleanup unavailable: {err}"),
                        }
                    }
                    if unsafe { DestroyWindow(self.window) } == 0 {
                        eprintln!("failed to destroy the test clipboard owner");
                    }
                }
            }
            let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
            let mut owner = Owner {
                window: unsafe {
                    CreateWindowExW(
                        0,
                        class.as_ptr(),
                        class.as_ptr(),
                        0,
                        0,
                        0,
                        0,
                        0,
                        HWND_MESSAGE,
                        null_mut(),
                        GetModuleHandleW(null_mut()),
                        null_mut(),
                    )
                },
                sequence: None,
            };
            assert!(
                !owner.window.is_null(),
                "create an invisible clipboard owner"
            );
            {
                let _clipboard = open(null_mut()).expect("inspect the test clipboard");
                assert_eq!(
                    unsafe { CountClipboardFormats() },
                    0,
                    "save and empty the clipboard before opting into this test"
                );
            }
            let text = "  first\n\n第二行 😀\rthird\r\n";
            write_with_owner(text.as_bytes(), owner.window).expect("write text");
            owner.sequence = Some(unsafe { GetClipboardSequenceNumber() });
            assert_eq!(
                read_clipboard_text().as_deref(),
                Some("  first\r\n\r\n第二行 😀\r\nthird\r\n")
            );
            assert_eq!(clipboard_text_matches(text.as_bytes()), Some(true));
            // A paste is allowed to read text independently of our dedup marker.
            LAST_WRITE_SEQUENCE.store(0, Ordering::Relaxed);
            assert!(read_clipboard_text().is_some());
            assert_eq!(clipboard_text_matches(text.as_bytes()), None);
            {
                let _clipboard = open(owner.window).expect("add a rich clipboard format");
                let format_name: Vec<u16> = "HTML Format".encode_utf16().chain([0]).collect();
                let format = unsafe {
                    windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW(
                        format_name.as_ptr(),
                    )
                };
                assert_ne!(format, 0);
                let mut memory = TextMemory::new(&[b'x'.into(), 0]).expect("rich format memory");
                assert!(!unsafe { SetClipboardData(format, memory.0) }.is_null());
                memory.0 = null_mut();
            }
            let sequence = unsafe { GetClipboardSequenceNumber() };
            owner.sequence = Some(sequence);
            LAST_WRITE_SEQUENCE.store(sequence, Ordering::Relaxed);
            assert!(
                read_clipboard_text().is_some(),
                "rich clipboard can contain text"
            );
            assert_eq!(clipboard_text_matches(text.as_bytes()), None);
        }
    }
}

#[cfg(windows)]
pub use native::{clipboard_text_matches, read_clipboard_text, write_clipboard};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_encoding_canonicalizes_all_newlines_and_preserves_unicode() {
        let encoded = encoded_text("one\ntwo\r\n三 😀\rfour\n\n".as_bytes()).expect("encode");
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&encoded[..encoded.len() - 1]).expect("UTF-16"),
            "one\r\ntwo\r\n三 😀\r\nfour\r\n\r\n"
        );
    }

    #[test]
    fn clipboard_encoding_rejects_invalid_text_before_native_writes() {
        assert!(encoded_text(&[0xff]).is_err());
        assert!(encoded_text(b"before\0after").is_err());
    }

    #[test]
    fn clipboard_decoder_respects_the_terminator_and_rejects_malformed_utf16() {
        let units = encoded_text("é e\u{301} 日本語 😀".as_bytes()).expect("encode");
        let mut bytes: Vec<_> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        bytes.extend_from_slice(&[0xff, 0xff, 0xff]);
        assert_eq!(
            decoded_text(&bytes).expect("decode"),
            "é e\u{301} 日本語 😀"
        );
        assert!(decoded_text(&[1]).is_err());
        assert!(decoded_text(&[1, 0]).is_err());
        assert!(decoded_text(&[0, 0xd8, 0, 0]).is_err());
        assert_eq!(decoded_text(&[0, 0]).expect("empty"), "");
    }

    #[test]
    fn duplicate_comparison_uses_the_same_line_ending_contract_as_writing() {
        assert!(text_equals("hello", b"hello"));
        assert!(text_equals("a\r\nb", b"a\nb"));
        assert!(text_equals("a\nb", b"a\rb"));
        assert!(!text_equals("hello ", b"hello"));
        assert!(!text_equals("hello", b"world"));
        assert!(!text_equals("hello", &[0xff]));
    }
}
