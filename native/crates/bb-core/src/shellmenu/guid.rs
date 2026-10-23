//! A COM class id, in the layout Windows uses and the spellings the registry and the settings file
//! use. The hidden list stores `clsid:{…}` lowercase; a registration's key name is sometimes the
//! CLSID written out again, which `parse` is what detects.

use std::fmt;

use windows_sys::Win32::System::Com::CoCreateGuid;
use windows_sys::core::GUID;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    /// A fresh one from Windows (`CoCreateGuid`).
    pub fn random() -> Guid {
        let mut guid = GUID {
            data1: 0,
            data2: 0,
            data3: 0,
            data4: [0; 8],
        };
        // SAFETY: the out pointer is valid for the call.
        unsafe { CoCreateGuid(&mut guid) };
        Guid {
            data1: guid.data1,
            data2: guid.data2,
            data3: guid.data3,
            data4: guid.data4,
        }
    }

    /// Accepts the spellings `Guid.TryParse` did: 32 hex digits, hyphenated, and either of those
    /// in braces or parentheses. Case does not matter.
    pub fn parse(text: &str) -> Option<Guid> {
        let t = text.trim();
        let inner = if (t.starts_with('{') && t.ends_with('}'))
            || (t.starts_with('(') && t.ends_with(')'))
        {
            &t[1..t.len() - 1]
        } else {
            t
        };

        let hex: String = match inner.len() {
            36 => {
                let b = inner.as_bytes();
                if [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') {
                    return None;
                }
                inner.chars().filter(|&c| c != '-').collect()
            }
            32 => inner.to_string(),
            _ => return None,
        };
        if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }

        let part = |from: usize, to: usize| u64::from_str_radix(&hex[from..to], 16).ok();
        let mut data4 = [0u8; 8];
        for (i, byte) in data4.iter_mut().enumerate() {
            *byte = part(16 + i * 2, 18 + i * 2)? as u8;
        }
        Some(Guid {
            data1: part(0, 8)? as u32,
            data2: part(8, 12)? as u16,
            data3: part(12, 16)? as u16,
            data4,
        })
    }

    /// `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`, lowercase, as `ToString("B")` gave it.
    pub fn to_braced(self) -> String {
        format!("{{{self}}}")
    }

    /// 32 lowercase hex digits, as `ToString("N")` gave it.
    pub fn to_simple(self) -> String {
        self.to_string().replace('-', "")
    }
}

impl fmt::Display for Guid {
    /// Hyphenated and lowercase, as `ToString()` gave it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.data4;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.data1, self.data2, self.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        )
    }
}
